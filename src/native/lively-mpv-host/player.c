/*
 * libmpv: options, render context, events, scaler and span maths, properties, screenshots, commands.
 *
 * The render context is created with MPV_RENDER_PARAM_ADVANCED_CONTROL so that mpv renders
 * screenshots on the GPU (the software path cannot convert hardware-decoded frames). That mode
 * forbids the render thread from ever waiting for the mpv core, and this process is single
 * threaded, so every property write, property read and command below uses the asynchronous
 * client API; results arrive as mpv events and are handled in player_handle_events().
 */
#include "app.h"

#include <EGL/egl.h>
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <unistd.h>

#define SCREENSHOT_FILE_TIMEOUT_SEC 5

/* ---- wakeup plumbing (called from mpv threads; only the pipe is touched) ---- */

static void signal_pipe(struct app *a)
{
    for (;;) {
        ssize_t r = write(a->wakeup_pipe[1], "w", 1);
        if (r == 1)
            return;
        if (r < 0 && errno == EINTR)
            continue;
        if (r < 0 && errno == EAGAIN)
            return; /* pipe full: a wakeup is already pending */
        fprintf(stderr, "lively-mpv-host: wakeup pipe write failed: %s\n", strerror(errno));
        return;
    }
}

static void on_mpv_wakeup(void *data)
{
    signal_pipe(data);
}

static void on_render_update(void *data)
{
    signal_pipe(data);
}

static void *get_proc_address(void *ctx, const char *name)
{
    (void)ctx;
    return (void *)eglGetProcAddress(name);
}

/* ---- asynchronous property helpers ---- */

static void remember_set(struct app *a, uint64_t token, const char *desc)
{
    struct pending_set *p = calloc(1, sizeof *p);
    if (!p) {
        app_error(a, "out of memory tracking property set %s", desc);
        return;
    }
    p->desc = strdup(desc);
    if (!p->desc) {
        app_error(a, "out of memory tracking property set %s", desc);
        free(p);
        return;
    }
    p->token = token;
    p->next = a->pending_sets;
    a->pending_sets = p;
}

/* Removes the pending entry for token and returns its description (caller frees), or NULL. */
static char *take_set_desc(struct app *a, uint64_t token)
{
    for (struct pending_set **link = &a->pending_sets; *link; link = &(*link)->next) {
        if ((*link)->token == token) {
            struct pending_set *p = *link;
            *link = p->next;
            char *desc = p->desc;
            free(p);
            return desc;
        }
    }
    return NULL;
}

static void request_readback(struct app *a, const char *name)
{
    if (!a->verbose)
        return;
    uint64_t token = ++a->next_token;
    int r = mpv_get_property_async(a->mpv, token, name, MPV_FORMAT_STRING);
    if (r < 0)
        app_error(a, "read back of %s failed: %s", name, mpv_error_string(r));
}

static int queue_set(struct app *a, const char *name, mpv_format format, void *data, const char *desc)
{
    uint64_t token = ++a->next_token;
    int r = mpv_set_property_async(a->mpv, token, name, format, data);
    if (r < 0) {
        app_error(a, "set %s failed: %s", desc, mpv_error_string(r));
        return r;
    }
    remember_set(a, token, desc);
    request_readback(a, name);
    return 0;
}

static int set_prop_string(struct app *a, const char *name, const char *value)
{
    char desc[512];
    snprintf(desc, sizeof desc, "%s=%s", name, value);
    char *string = (char *)value; /* mpv copies the value */
    return queue_set(a, name, MPV_FORMAT_STRING, &string, desc);
}

static int set_prop_double(struct app *a, const char *name, double value)
{
    char desc[256];
    snprintf(desc, sizeof desc, "%s=%.6f", name, value);
    return queue_set(a, name, MPV_FORMAT_DOUBLE, &value, desc);
}

static int set_prop_int(struct app *a, const char *name, int64_t value)
{
    char desc[256];
    snprintf(desc, sizeof desc, "%s=%lld", name, (long long)value);
    return queue_set(a, name, MPV_FORMAT_INT64, &value, desc);
}

static int set_prop_flag(struct app *a, const char *name, bool value)
{
    char desc[256];
    snprintf(desc, sizeof desc, "%s=%s", name, value ? "true" : "false");
    int flag = value ? 1 : 0;
    return queue_set(a, name, MPV_FORMAT_FLAG, &flag, desc);
}

static void on_set_property_reply(struct app *a, const mpv_event *ev)
{
    char *desc = take_set_desc(a, ev->reply_userdata);
    if (ev->error < 0)
        app_error(a, "set %s failed: %s", desc ? desc : "(unknown property)", mpv_error_string(ev->error));
    else
        app_log(a, "set %s ok", desc ? desc : "(unknown property)");
    free(desc);
}

static void on_get_property_reply(struct app *a, const mpv_event *ev)
{
    const mpv_event_property *prop = ev->data;
    if (ev->error < 0 || prop->format != MPV_FORMAT_STRING || !prop->data)
        app_log(a, "mpv property %s could not be read back: %s", prop->name, mpv_error_string(ev->error));
    else
        app_log(a, "mpv property %s is now %s", prop->name, *(char **)prop->data);
}

/* ---- scaler and span maths ---- */

static const char *scaler_name(enum scaler scaler)
{
    switch (scaler) {
    case SCALER_NONE: return "none";
    case SCALER_FILL: return "fill";
    case SCALER_UNIFORM: return "uniform";
    case SCALER_UNIFORM_FILL: return "uniformFill";
    }
    return "unknown";
}

/*
 * Span mode: fit the video into the virtual screen for the chosen scaler, then place the fitted
 * rectangle so that this output's buffer shows exactly its slice. mpv ignores video-scale-x/y and
 * video-pan-x/y when keepaspect=no (verified against mpv 0.40: the video always fills the window),
 * so the placement uses keepaspect=yes with video-unscaled=yes, which makes mpv's base rectangle
 * exactly dwidth x dheight; video-scale-x/y then set the rectangle size and video-pan-x/y its
 * position (mpv formula: dst_start = (window - size) / 2 + pan * size).
 */
static void apply_span(struct app *a)
{
    if (!a->configured) {
        app_log(a, "span: surface not configured yet, deferring");
        return;
    }
    if (a->disp_w <= 0 || a->disp_h <= 0) {
        app_log(a, "span: video size not known yet, deferring");
        return;
    }
    const double vw = a->span.vw, vh = a->span.vh;
    const double dw = (double)a->disp_w, dh = (double)a->disp_h;
    double fw, fh;
    switch (a->scaler) {
    case SCALER_FILL:
        fw = vw;
        fh = vh;
        break;
    case SCALER_UNIFORM: {
        double s = fmin(vw / dw, vh / dh);
        fw = dw * s;
        fh = dh * s;
        break;
    }
    case SCALER_UNIFORM_FILL: {
        double s = fmax(vw / dw, vh / dh);
        fw = dw * s;
        fh = dh * s;
        break;
    }
    case SCALER_NONE:
    default:
        fw = dw;
        fh = dh;
        break;
    }
    const double fx = (vw - fw) / 2.0;
    const double fy = (vh - fh) / 2.0;

    const double bw = (double)wl_buffer_width(a);
    const double bh = (double)wl_buffer_height(a);
    const double kx = bw / a->span.w;
    const double ky = bh / a->span.h;
    const double tx = (fx - a->span.x) * kx;
    const double ty = (fy - a->span.y) * ky;
    const double tw = fw * kx;
    const double th = fh * ky;

    /* mpv truncates size * scale to an integer; the 0.02 px margin keeps the truncation from losing a pixel. */
    const double scale_x = (tw + 0.02) / dw;
    const double scale_y = (th + 0.02) / dh;
    const double pan_x = (tx - (bw - tw) / 2.0) / tw;
    const double pan_y = (ty - (bh - th) / 2.0) / th;

    app_log(a, "span %s: video %lldx%lld fitted to %.1fx%.1f at %.1f,%.1f in %.0fx%.0f; slice %.1f,%.1f %.1fx%.1f in %.0fx%.0f px; "
            "scale %.6f,%.6f pan %.6f,%.6f",
            scaler_name(a->scaler), (long long)a->disp_w, (long long)a->disp_h, fw, fh, fx, fy, vw, vh,
            tx, ty, tw, th, bw, bh, scale_x, scale_y, pan_x, pan_y);

    set_prop_string(a, "keepaspect", "yes");
    set_prop_string(a, "video-unscaled", "yes");
    set_prop_double(a, "panscan", 0.0);
    set_prop_double(a, "video-zoom", 0.0);
    set_prop_double(a, "video-align-x", 0.0);
    set_prop_double(a, "video-align-y", 0.0);
    set_prop_double(a, "video-scale-x", scale_x);
    set_prop_double(a, "video-scale-y", scale_y);
    set_prop_double(a, "video-pan-x", pan_x);
    set_prop_double(a, "video-pan-y", pan_y);
}

/* Mirrors VideoMpvPlayer.UpdateScaler in the Windows core. */
static void apply_scaler(struct app *a)
{
    if (a->span.enabled) {
        apply_span(a);
        return;
    }
    app_log(a, "scaler %s", scaler_name(a->scaler));
    switch (a->scaler) {
    case SCALER_NONE:
        set_prop_string(a, "keepaspect", "yes");
        set_prop_string(a, "video-unscaled", "yes");
        break;
    case SCALER_FILL:
        set_prop_string(a, "video-unscaled", "no");
        set_prop_string(a, "keepaspect", "no");
        break;
    case SCALER_UNIFORM:
        set_prop_string(a, "panscan", "0.0");
        set_prop_string(a, "video-unscaled", "no");
        set_prop_string(a, "keepaspect", "yes");
        break;
    case SCALER_UNIFORM_FILL:
        set_prop_string(a, "video-unscaled", "no");
        set_prop_string(a, "keepaspect", "yes");
        set_prop_string(a, "panscan", "1.0");
        break;
    }
}

void player_set_scaler(struct app *a, enum scaler scaler)
{
    a->scaler = scaler;
    apply_scaler(a);
}

static bool path_has_gif_extension(const char *path)
{
    const char *dot = strrchr(path, '.');
    return dot && strcasecmp(dot, ".gif") == 0;
}

/* Applies everything that depends on the video size and the buffer size (nearest scaling, span slice). */
static void apply_video_geometry(struct app *a)
{
    if (a->disp_w <= 0 || a->disp_h <= 0 || !a->configured)
        return;
    if (a->image || a->is_gif) {
        if (a->video_w < (int64_t)wl_buffer_width(a) && a->video_h < (int64_t)wl_buffer_height(a)) {
            app_log(a, "image smaller than the output, using nearest-neighbour scaling");
            set_prop_string(a, "scale", "nearest");
        }
    }
    if (a->span.enabled)
        apply_span(a);
}

static int64_t node_map_int(const mpv_node *map, const char *key, int64_t fallback)
{
    for (int i = 0; i < map->u.list->num; i++) {
        if (strcmp(map->u.list->keys[i], key) == 0 && map->u.list->values[i].format == MPV_FORMAT_INT64)
            return map->u.list->values[i].u.int64;
    }
    return fallback;
}

/* Observed video-out-params: the size mpv actually renders (after filters), aspect-corrected size and rotation. */
static void on_video_out_params(struct app *a, const mpv_event_property *prop)
{
    if (prop->format != MPV_FORMAT_NODE || !prop->data) {
        a->video_w = a->video_h = a->disp_w = a->disp_h = 0;
        app_log(a, "no video parameters (no video track yet)");
        return;
    }
    const mpv_node *map = prop->data;
    if (map->format != MPV_FORMAT_NODE_MAP) {
        app_log(a, "video-out-params is not a map");
        return;
    }
    int64_t w = node_map_int(map, "w", 0);
    int64_t h = node_map_int(map, "h", 0);
    int64_t dw = node_map_int(map, "dw", 0);
    int64_t dh = node_map_int(map, "dh", 0);
    int64_t rotate = node_map_int(map, "rotate", 0);
    if (w <= 0 || h <= 0) {
        app_log(a, "video-out-params without a size");
        return;
    }
    if (dw <= 0 || dh <= 0) {
        dw = w;
        dh = h;
    }
    rotate = ((rotate % 360) + 360) % 360;
    if (rotate % 180 == 90) {
        int64_t tmp = dw;
        dw = dh;
        dh = tmp;
    }
    bool changed = w != a->video_w || h != a->video_h || dw != a->disp_w || dh != a->disp_h;
    a->video_w = w;
    a->video_h = h;
    a->disp_w = dw;
    a->disp_h = dh;
    app_log(a, "video %lldx%lld, display size %lldx%lld, rotation %lld%s", (long long)w, (long long)h,
            (long long)dw, (long long)dh, (long long)rotate, changed ? "" : " (unchanged)");
    if (changed)
        apply_video_geometry(a);
}

static void on_file_format(struct app *a, const mpv_event_property *prop)
{
    const char *format = (prop->format == MPV_FORMAT_STRING && prop->data) ? *(char **)prop->data : NULL;
    a->is_gif = (format && strcmp(format, "gif") == 0) || path_has_gif_extension(a->media);
    app_log(a, "file format %s%s", format ? format : "(none)", a->is_gif ? " (gif)" : "");
}

void player_surface_resized(struct app *a)
{
    apply_video_geometry(a);
}

/* ---- LivelyProperties.json ---- */

int player_set_slider(struct app *a, const char *name, double value, double step)
{
    /* mpv is strongly typed: whole-step sliders drive integer properties (same rule as the Windows core). */
    if (fmod(step, 1.0) == 0.0)
        return set_prop_int(a, name, (int64_t)llround(value));
    return set_prop_double(a, name, value);
}

int player_set_checkbox(struct app *a, const char *name, bool value)
{
    return set_prop_flag(a, name, value);
}

static char *read_whole_file(struct app *a, const char *path)
{
    FILE *f = fopen(path, "rb");
    if (!f) {
        app_error(a, "cannot open %s: %s", path, strerror(errno));
        return NULL;
    }
    char *buf = NULL;
    size_t len = 0, cap = 0;
    for (;;) {
        if (len + 4096 + 1 > cap) {
            size_t ncap = cap ? cap * 2 : 8192;
            char *nbuf = realloc(buf, ncap);
            if (!nbuf) {
                app_error(a, "out of memory reading %s", path);
                free(buf);
                fclose(f);
                return NULL;
            }
            buf = nbuf;
            cap = ncap;
        }
        size_t n = fread(buf + len, 1, cap - len - 1, f);
        len += n;
        if (n == 0) {
            if (ferror(f)) {
                app_error(a, "read error on %s: %s", path, strerror(errno));
                free(buf);
                fclose(f);
                return NULL;
            }
            break;
        }
    }
    if (fclose(f) != 0)
        app_error(a, "close %s: %s", path, strerror(errno));
    buf[len] = '\0';
    return buf;
}

int player_apply_properties(struct app *a)
{
    if (!a->property_path)
        return 0;
    char *text = read_whole_file(a, a->property_path);
    if (!text)
        return -1;
    cJSON *root = cJSON_Parse(text);
    free(text);
    if (!root || !cJSON_IsObject(root)) {
        app_error(a, "%s is not a JSON object of controls", a->property_path);
        cJSON_Delete(root);
        return -1;
    }
    app_log(a, "applying %s", a->property_path);
    int failures = 0;
    cJSON *control = NULL;
    cJSON_ArrayForEach(control, root) {
        const char *name = control->string;
        if (!name || !cJSON_IsObject(control)) {
            app_error(a, "property file: control without a name or body, skipped");
            failures++;
            continue;
        }
        const cJSON *type = cJSON_GetObjectItemCaseSensitive(control, "type");
        const cJSON *value = cJSON_GetObjectItemCaseSensitive(control, "value");
        if (!cJSON_IsString(type)) {
            app_error(a, "property file: control %s has no type, skipped", name);
            failures++;
            continue;
        }
        if (strcmp(type->valuestring, "slider") == 0) {
            const cJSON *step = cJSON_GetObjectItemCaseSensitive(control, "step");
            if (!cJSON_IsNumber(value)) {
                app_error(a, "property file: slider %s has no numeric value, skipped", name);
                failures++;
                continue;
            }
            double step_value = cJSON_IsNumber(step) ? step->valuedouble : 1.0;
            if (player_set_slider(a, name, value->valuedouble, step_value) < 0)
                failures++;
        } else if (strcmp(type->valuestring, "checkbox") == 0) {
            if (!cJSON_IsBool(value)) {
                app_error(a, "property file: checkbox %s has no boolean value, skipped", name);
                failures++;
                continue;
            }
            if (player_set_checkbox(a, name, cJSON_IsTrue(value)) < 0)
                failures++;
        } else if (strcmp(type->valuestring, "scalerDropdown") == 0) {
            if (!cJSON_IsNumber(value) || value->valuedouble < SCALER_NONE || value->valuedouble > SCALER_UNIFORM_FILL) {
                app_error(a, "property file: scalerDropdown %s has an invalid value, skipped", name);
                failures++;
                continue;
            }
            player_set_scaler(a, (enum scaler)(int)value->valuedouble);
        } else {
            /* Like the Windows mpv player, only slider, checkbox and scalerDropdown drive mpv. */
            app_log(a, "property file: %s control %s does not map to mpv, ignored", type->valuestring, name);
        }
    }
    cJSON_Delete(root);
    return failures ? -1 : 0;
}

/* ---- simple commands ---- */

int player_set_pause(struct app *a, bool paused)
{
    return set_prop_flag(a, "pause", paused);
}

int player_set_volume(struct app *a, int volume)
{
    return set_prop_int(a, "volume", volume);
}

static int queue_loadfile(struct app *a)
{
    const char *args[] = { "loadfile", a->media, NULL };
    a->file_loaded = false;
    uint64_t token = ++a->next_token;
    int r = mpv_command_async(a->mpv, token, args);
    if (r < 0)
        app_error(a, "loadfile %s failed: %s", a->media, mpv_error_string(r));
    else
        app_log(a, "loading %s (token %llu)", a->media, (unsigned long long)token);
    return r;
}

int player_reload(struct app *a)
{
    return queue_loadfile(a);
}

/* ---- JSON -> mpv_node for host_mpv_command ---- */

static void node_free(mpv_node *n)
{
    switch (n->format) {
    case MPV_FORMAT_STRING:
        free(n->u.string);
        break;
    case MPV_FORMAT_NODE_ARRAY:
    case MPV_FORMAT_NODE_MAP:
        if (n->u.list) {
            for (int i = 0; i < n->u.list->num; i++)
                node_free(&n->u.list->values[i]);
            if (n->u.list->keys) {
                for (int i = 0; i < n->u.list->num; i++)
                    free(n->u.list->keys[i]);
                free(n->u.list->keys);
            }
            free(n->u.list->values);
            free(n->u.list);
        }
        break;
    default:
        break;
    }
    n->format = MPV_FORMAT_NONE;
}

static int node_from_json(struct app *a, const cJSON *j, mpv_node *n)
{
    memset(n, 0, sizeof *n);
    if (cJSON_IsString(j)) {
        n->u.string = strdup(j->valuestring);
        if (!n->u.string) {
            app_error(a, "out of memory building mpv command");
            return -1;
        }
        n->format = MPV_FORMAT_STRING;
        return 0;
    }
    if (cJSON_IsBool(j)) {
        n->format = MPV_FORMAT_FLAG;
        n->u.flag = cJSON_IsTrue(j) ? 1 : 0;
        return 0;
    }
    if (cJSON_IsNumber(j)) {
        double v = j->valuedouble;
        if (v == floor(v) && fabs(v) < 9007199254740992.0) {
            n->format = MPV_FORMAT_INT64;
            n->u.int64 = (int64_t)v;
        } else {
            n->format = MPV_FORMAT_DOUBLE;
            n->u.double_ = v;
        }
        return 0;
    }
    if (cJSON_IsNull(j)) {
        n->format = MPV_FORMAT_NONE;
        return 0;
    }
    if (cJSON_IsArray(j) || cJSON_IsObject(j)) {
        bool is_map = cJSON_IsObject(j);
        int count = cJSON_GetArraySize(j);
        mpv_node_list *list = calloc(1, sizeof *list);
        if (!list) {
            app_error(a, "out of memory building mpv command");
            return -1;
        }
        n->format = is_map ? MPV_FORMAT_NODE_MAP : MPV_FORMAT_NODE_ARRAY;
        n->u.list = list;
        list->values = calloc(count > 0 ? (size_t)count : 1, sizeof(mpv_node));
        list->keys = is_map ? calloc(count > 0 ? (size_t)count : 1, sizeof(char *)) : NULL;
        if (!list->values || (is_map && !list->keys)) {
            app_error(a, "out of memory building mpv command");
            node_free(n);
            return -1;
        }
        int i = 0;
        const cJSON *child = NULL;
        cJSON_ArrayForEach(child, j) {
            if (node_from_json(a, child, &list->values[i]) < 0) {
                list->num = i;
                node_free(n);
                return -1;
            }
            if (is_map) {
                list->keys[i] = strdup(child->string ? child->string : "");
                if (!list->keys[i]) {
                    app_error(a, "out of memory building mpv command");
                    list->num = i + 1;
                    node_free(n);
                    return -1;
                }
            }
            i++;
        }
        list->num = i;
        return 0;
    }
    app_error(a, "unsupported JSON value in mpv command");
    return -1;
}

/*
 * mpv's core "set"/"set_property" command only takes string arguments; typed values such as
 * ["set_property","pause",true] work over mpv's JSON IPC because the IPC layer routes
 * set_property/get_property to the client property API. The core sends that IPC form, so the
 * same routing is done here.
 */
static int run_property_command(struct app *a, const char *verb, const cJSON *command, int count)
{
    const cJSON *name = cJSON_GetArrayItem(command, 1);
    if (!cJSON_IsString(name)) {
        app_error(a, "host_mpv_command: %s needs a property name", verb);
        return -1;
    }
    if (strcmp(verb, "get_property") == 0) {
        if (count != 2) {
            app_error(a, "host_mpv_command: get_property takes exactly one argument");
            return -1;
        }
        uint64_t token = ++a->next_token;
        int r = mpv_get_property_async(a->mpv, token, name->valuestring, MPV_FORMAT_STRING);
        if (r < 0)
            app_error(a, "host_mpv_command: get_property %s rejected: %s", name->valuestring, mpv_error_string(r));
        return r;
    }
    if (count != 3) {
        app_error(a, "host_mpv_command: set_property takes a name and a value");
        return -1;
    }
    mpv_node value;
    if (node_from_json(a, cJSON_GetArrayItem(command, 2), &value) < 0)
        return -1;
    char desc[512];
    char *json = cJSON_PrintUnformatted(cJSON_GetArrayItem(command, 2));
    snprintf(desc, sizeof desc, "%s=%s (host_mpv_command)", name->valuestring, json ? json : "?");
    free(json);
    int r = queue_set(a, name->valuestring, MPV_FORMAT_NODE, &value, desc);
    node_free(&value);
    return r;
}

int player_run_command(struct app *a, const cJSON *command)
{
    if (!cJSON_IsArray(command) && !cJSON_IsObject(command)) {
        app_error(a, "host_mpv_command: Command must be a JSON array (or named-argument object)");
        return -1;
    }
    if (cJSON_IsArray(command)) {
        int count = cJSON_GetArraySize(command);
        const cJSON *verb = cJSON_GetArrayItem(command, 0);
        if (count >= 1 && cJSON_IsString(verb) &&
            (strcmp(verb->valuestring, "set_property") == 0 || strcmp(verb->valuestring, "get_property") == 0))
            return run_property_command(a, verb->valuestring, command, count);
    }
    mpv_node node;
    if (node_from_json(a, command, &node) < 0)
        return -1;
    uint64_t token = ++a->next_token;
    int r = mpv_command_node_async(a->mpv, token, &node);
    node_free(&node);
    if (r < 0) {
        app_error(a, "host_mpv_command rejected: %s", mpv_error_string(r));
        return r;
    }
    app_log(a, "host_mpv_command queued (token %llu)", (unsigned long long)token);
    return 0;
}

/* ---- screenshots ---- */

static struct timespec deadline_from_now(int seconds)
{
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    now.tv_sec += seconds;
    return now;
}

static bool deadline_passed(const struct timespec *deadline)
{
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    return now.tv_sec > deadline->tv_sec || (now.tv_sec == deadline->tv_sec && now.tv_nsec >= deadline->tv_nsec);
}

static const char *screenshot_format_name(enum shot_format format)
{
    switch (format) {
    case SHOT_JPEG: return "jpg";
    case SHOT_PNG: return "png";
    case SHOT_WEBP: return "webp";
    case SHOT_BMP: return "bmp";
    }
    return "png";
}

static int start_screenshot(struct app *a, struct shot_request *req);

static void finish_screenshot(struct app *a, struct shot_request *req, bool success)
{
    app_log(a, "screenshot %s: %s", req->path, success ? "ok" : "failed");
    ipc_send_screenshot(a, req->path, success);
    a->shots = req->next;
    free(req->path);
    free(req);
    if (a->shots && !a->shots->started && a->running)
        start_screenshot(a, a->shots);
}

static int start_screenshot(struct app *a, struct shot_request *req)
{
    req->started = true;
    req->token = ++a->next_token;
    int r;
    if (req->format == SHOT_BMP) {
        /* mpv has no BMP encoder: fetch the raw frame and write the BMP ourselves on reply. */
        const char *args[] = { "screenshot-raw", NULL };
        r = mpv_command_async(a->mpv, req->token, args);
    } else {
        /* Requests run in order on the mpv core, so the format is set before the command runs. */
        r = set_prop_string(a, "screenshot-format", screenshot_format_name(req->format));
        if (r >= 0) {
            const char *args[] = { "screenshot-to-file", req->path, NULL };
            r = mpv_command_async(a->mpv, req->token, args);
        }
    }
    if (r < 0) {
        app_error(a, "screenshot command failed: %s", mpv_error_string(r));
        finish_screenshot(a, req, false);
        return r;
    }
    app_log(a, "screenshot %s (%s) queued, token %llu", req->path, screenshot_format_name(req->format),
            (unsigned long long)req->token);
    return 0;
}

int player_screenshot(struct app *a, const char *path, enum shot_format format)
{
    struct shot_request *req = calloc(1, sizeof *req);
    if (!req) {
        app_error(a, "out of memory queuing screenshot");
        ipc_send_screenshot(a, path, false);
        return -1;
    }
    req->path = strdup(path);
    if (!req->path) {
        app_error(a, "out of memory queuing screenshot");
        free(req);
        ipc_send_screenshot(a, path, false);
        return -1;
    }
    req->format = format;
    struct shot_request **tail = &a->shots;
    while (*tail)
        tail = &(*tail)->next;
    *tail = req;
    if (a->shots == req)
        return start_screenshot(a, req);
    app_log(a, "screenshot %s queued behind %s", path, a->shots->path);
    return 0;
}

static void put_le16(unsigned char *p, uint16_t v)
{
    p[0] = (unsigned char)(v & 0xff);
    p[1] = (unsigned char)((v >> 8) & 0xff);
}

static void put_le32(unsigned char *p, uint32_t v)
{
    p[0] = (unsigned char)(v & 0xff);
    p[1] = (unsigned char)((v >> 8) & 0xff);
    p[2] = (unsigned char)((v >> 16) & 0xff);
    p[3] = (unsigned char)((v >> 24) & 0xff);
}

/* Writes the screenshot-raw result (bgr0/bgra/rgba, 4 bytes per pixel) as a 24-bit bottom-up BMP. */
static int write_bmp(struct app *a, const char *path, const mpv_node *result)
{
    if (result->format != MPV_FORMAT_NODE_MAP) {
        app_error(a, "screenshot-raw returned no image map");
        return -1;
    }
    int64_t w = 0, h = 0, stride = 0;
    const char *format = NULL;
    const mpv_byte_array *data = NULL;
    for (int i = 0; i < result->u.list->num; i++) {
        const char *key = result->u.list->keys[i];
        const mpv_node *v = &result->u.list->values[i];
        if (strcmp(key, "w") == 0 && v->format == MPV_FORMAT_INT64)
            w = v->u.int64;
        else if (strcmp(key, "h") == 0 && v->format == MPV_FORMAT_INT64)
            h = v->u.int64;
        else if (strcmp(key, "stride") == 0 && v->format == MPV_FORMAT_INT64)
            stride = v->u.int64;
        else if (strcmp(key, "format") == 0 && v->format == MPV_FORMAT_STRING)
            format = v->u.string;
        else if (strcmp(key, "data") == 0 && v->format == MPV_FORMAT_BYTE_ARRAY)
            data = v->u.ba;
    }
    if (w <= 0 || h <= 0 || stride < w * 4 || !format || !data || data->size < (size_t)(stride * h)) {
        app_error(a, "screenshot-raw returned an incomplete image (%lldx%lld, stride %lld, format %s)",
                  (long long)w, (long long)h, (long long)stride, format ? format : "none");
        return -1;
    }
    int ir, ig, ib;
    if (strcmp(format, "bgr0") == 0 || strcmp(format, "bgra") == 0) {
        ib = 0; ig = 1; ir = 2;
    } else if (strcmp(format, "rgba") == 0 || strcmp(format, "rgb0") == 0) {
        ir = 0; ig = 1; ib = 2;
    } else {
        app_error(a, "screenshot-raw pixel format %s is not 8-bit RGB", format);
        return -1;
    }
    const size_t row_bytes = ((size_t)w * 3 + 3) & ~(size_t)3;
    const uint32_t image_size = (uint32_t)(row_bytes * (size_t)h);
    unsigned char header[54] = { 'B', 'M' };
    put_le32(header + 2, 54 + image_size);
    put_le32(header + 10, 54);
    put_le32(header + 14, 40);
    put_le32(header + 18, (uint32_t)w);
    put_le32(header + 22, (uint32_t)h);
    put_le16(header + 26, 1);
    put_le16(header + 28, 24);
    put_le32(header + 30, 0);
    put_le32(header + 34, image_size);
    put_le32(header + 38, 2835);
    put_le32(header + 42, 2835);

    unsigned char *row = calloc(row_bytes, 1);
    if (!row) {
        app_error(a, "out of memory writing BMP");
        return -1;
    }
    FILE *f = fopen(path, "wb");
    if (!f) {
        app_error(a, "cannot create %s: %s", path, strerror(errno));
        free(row);
        return -1;
    }
    bool ok = fwrite(header, 1, sizeof header, f) == sizeof header;
    const unsigned char *pixels = data->data;
    for (int64_t y = h - 1; ok && y >= 0; y--) {
        const unsigned char *src = pixels + (size_t)y * (size_t)stride;
        for (int64_t x = 0; x < w; x++) {
            row[x * 3 + 0] = src[x * 4 + ib];
            row[x * 3 + 1] = src[x * 4 + ig];
            row[x * 3 + 2] = src[x * 4 + ir];
        }
        ok = fwrite(row, 1, row_bytes, f) == row_bytes;
    }
    free(row);
    if (!ok) {
        app_error(a, "write error on %s: %s", path, strerror(errno));
        fclose(f);
        return -1;
    }
    if (fclose(f) != 0) {
        app_error(a, "close %s: %s", path, strerror(errno));
        return -1;
    }
    return 0;
}

bool player_screenshot_waiting(const struct app *a)
{
    return a->shots && a->shots->command_done;
}

void player_poll_screenshots(struct app *a)
{
    if (!player_screenshot_waiting(a))
        return;
    struct shot_request *req = a->shots;
    struct stat st;
    if (stat(req->path, &st) == 0 && S_ISREG(st.st_mode) && st.st_size > 0) {
        finish_screenshot(a, req, true);
        return;
    }
    if (deadline_passed(&req->deadline)) {
        app_error(a, "screenshot file %s did not appear within %d s", req->path, SCREENSHOT_FILE_TIMEOUT_SEC);
        finish_screenshot(a, req, false);
    }
}

static void on_command_reply(struct app *a, const mpv_event *ev)
{
    struct shot_request *req = a->shots;
    if (req && req->started && !req->command_done && ev->reply_userdata == req->token) {
        if (ev->error < 0) {
            app_error(a, "screenshot %s failed: %s", req->path, mpv_error_string(ev->error));
            finish_screenshot(a, req, false);
            return;
        }
        if (req->format == SHOT_BMP) {
            const mpv_event_command *cmd = ev->data;
            if (write_bmp(a, req->path, &cmd->result) < 0) {
                finish_screenshot(a, req, false);
                return;
            }
        }
        req->command_done = true;
        req->deadline = deadline_from_now(SCREENSHOT_FILE_TIMEOUT_SEC);
        player_poll_screenshots(a);
        return;
    }
    if (ev->error < 0)
        app_error(a, "mpv command (token %llu) failed: %s", (unsigned long long)ev->reply_userdata,
                  mpv_error_string(ev->error));
    else
        app_log(a, "mpv command (token %llu) completed", (unsigned long long)ev->reply_userdata);
}

/* ---- events ---- */

static void on_log_message(struct app *a, const mpv_event_log_message *msg)
{
    size_t len = strlen(msg->text);
    while (len > 0 && (msg->text[len - 1] == '\n' || msg->text[len - 1] == '\r'))
        len--;
    char *line = NULL;
    int n = asprintf(&line, "[%s] %.*s", msg->prefix, (int)len, msg->text);
    if (n < 0 || !line) {
        app_error(a, "out of memory forwarding mpv log line");
        return;
    }
    int category = msg->log_level <= MPV_LOG_LEVEL_ERROR ? 1 : 0;
    ipc_send_console(a, line, category);
    free(line);
}

static void on_file_loaded(struct app *a)
{
    a->file_loaded = true;
    app_log(a, "file loaded");
    if (player_apply_properties(a) < 0)
        app_error(a, "some Lively properties could not be applied");
    apply_scaler(a);
    apply_video_geometry(a);
    ipc_send_wploaded(a, true);
}

void player_handle_events(struct app *a)
{
    if (!a->mpv)
        return;
    for (;;) {
        mpv_event *ev = mpv_wait_event(a->mpv, 0);
        if (ev->event_id == MPV_EVENT_NONE)
            break;
        switch (ev->event_id) {
        case MPV_EVENT_LOG_MESSAGE:
            on_log_message(a, ev->data);
            break;
        case MPV_EVENT_FILE_LOADED:
            on_file_loaded(a);
            break;
        case MPV_EVENT_END_FILE: {
            const mpv_event_end_file *end = ev->data;
            if (end->reason == MPV_END_FILE_REASON_ERROR) {
                app_error(a, "playback of %s failed: %s", a->media, mpv_error_string(end->error));
                ipc_send_wploaded(a, false);
                app_quit(a, HOST_EXIT_MEDIA);
            } else if (end->reason == MPV_END_FILE_REASON_QUIT) {
                app_log(a, "mpv quit");
                app_quit(a, HOST_EXIT_OK);
            } else {
                app_log(a, "end of file (reason %d)", (int)end->reason);
            }
            break;
        }
        case MPV_EVENT_SHUTDOWN:
            app_log(a, "mpv shutdown");
            app_quit(a, HOST_EXIT_OK);
            break;
        case MPV_EVENT_PROPERTY_CHANGE: {
            const mpv_event_property *prop = ev->data;
            if (strcmp(prop->name, "video-out-params") == 0)
                on_video_out_params(a, prop);
            else if (strcmp(prop->name, "file-format") == 0)
                on_file_format(a, prop);
            break;
        }
        case MPV_EVENT_SET_PROPERTY_REPLY:
            on_set_property_reply(a, ev);
            break;
        case MPV_EVENT_GET_PROPERTY_REPLY:
            on_get_property_reply(a, ev);
            break;
        case MPV_EVENT_COMMAND_REPLY:
            on_command_reply(a, ev);
            break;
        default:
            break;
        }
        if (!a->running)
            break;
    }
}

void player_handle_update(struct app *a)
{
    if (!a->render)
        return;
    uint64_t flags = mpv_render_context_update(a->render);
    if (flags & MPV_RENDER_UPDATE_FRAME)
        wl_request_redraw(a);
}

/* ---- init / shutdown ---- */

static int set_option(struct app *a, const char *name, const char *value)
{
    int r = mpv_set_option_string(a->mpv, name, value);
    if (r < 0)
        app_error(a, "mpv option %s=%s failed: %s", name, value, mpv_error_string(r));
    else
        app_log(a, "mpv option %s=%s", name, value);
    return r;
}

int player_init(struct app *a)
{
    if (pipe2(a->wakeup_pipe, O_CLOEXEC | O_NONBLOCK) < 0) {
        app_error(a, "pipe2 failed: %s", strerror(errno));
        return HOST_EXIT_RENDERER;
    }
    a->mpv = mpv_create();
    if (!a->mpv) {
        app_error(a, "mpv_create failed");
        return HOST_EXIT_RENDERER;
    }

    char volume[16];
    snprintf(volume, sizeof volume, "%d", a->volume);
    int failures = 0;
    failures += set_option(a, "vo", "libmpv") < 0;
    failures += set_option(a, "volume", volume) < 0;
    failures += set_option(a, "loop-file", a->image ? "no" : "inf") < 0;
    failures += set_option(a, "keep-open", "yes") < 0;
    failures += set_option(a, "input-default-bindings", "no") < 0;
    failures += set_option(a, "osc", "no") < 0;
    failures += set_option(a, "hwdec", a->hwdec) < 0;
    failures += set_option(a, "audio-client-name", "Lively Wallpaper") < 0;
    failures += set_option(a, "terminal", "no") < 0;
    failures += set_option(a, "idle", "yes") < 0;
    failures += set_option(a, "ytdl", a->ytdl_format ? "yes" : "no") < 0;
    if (a->ytdl_format)
        failures += set_option(a, "ytdl-format", a->ytdl_format) < 0;
    if (a->image)
        failures += set_option(a, "image-display-duration", "inf") < 0;
    if (a->config_dir) {
        failures += set_option(a, "config", "yes") < 0;
        failures += set_option(a, "config-dir", a->config_dir) < 0;
    } else {
        failures += set_option(a, "config", "no") < 0;
    }
    if (a->has_speed) {
        char speed[32];
        snprintf(speed, sizeof speed, "%.6g", a->speed);
        failures += set_option(a, "speed", speed) < 0;
    }
    if (failures) {
        app_error(a, "%d mpv startup options were rejected", failures);
        return HOST_EXIT_RENDERER;
    }
    int r = mpv_request_log_messages(a->mpv, "info");
    if (r < 0) {
        app_error(a, "mpv_request_log_messages failed: %s", mpv_error_string(r));
        return HOST_EXIT_RENDERER;
    }
    r = mpv_initialize(a->mpv);
    if (r < 0) {
        app_error(a, "mpv_initialize failed: %s", mpv_error_string(r));
        return HOST_EXIT_RENDERER;
    }
    r = mpv_observe_property(a->mpv, 0, "video-out-params", MPV_FORMAT_NODE);
    if (r >= 0)
        r = mpv_observe_property(a->mpv, 0, "file-format", MPV_FORMAT_STRING);
    if (r < 0) {
        app_error(a, "mpv_observe_property failed: %s", mpv_error_string(r));
        return HOST_EXIT_RENDERER;
    }
    mpv_set_wakeup_callback(a->mpv, on_mpv_wakeup, a);

    mpv_opengl_init_params gl_params = { .get_proc_address = get_proc_address, .get_proc_address_ctx = NULL };
    int advanced_control = 1;
    mpv_render_param params[] = {
        { MPV_RENDER_PARAM_API_TYPE, (void *)MPV_RENDER_API_TYPE_OPENGL },
        { MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, &gl_params },
        { MPV_RENDER_PARAM_WL_DISPLAY, a->display },
        { MPV_RENDER_PARAM_ADVANCED_CONTROL, &advanced_control },
        { MPV_RENDER_PARAM_INVALID, NULL },
    };
    r = mpv_render_context_create(&a->render, a->mpv, params);
    if (r < 0) {
        app_error(a, "mpv_render_context_create failed: %s", mpv_error_string(r));
        a->render = NULL;
        return HOST_EXIT_RENDERER;
    }
    mpv_render_context_set_update_callback(a->render, on_render_update, a);

    apply_scaler(a);
    if (queue_loadfile(a) < 0)
        return HOST_EXIT_MEDIA;
    return 0;
}

void player_shutdown(struct app *a)
{
    while (a->shots) {
        struct shot_request *req = a->shots;
        a->shots = req->next;
        free(req->path);
        free(req);
    }
    while (a->pending_sets) {
        struct pending_set *p = a->pending_sets;
        a->pending_sets = p->next;
        free(p->desc);
        free(p);
    }
    if (a->render) {
        /* Must happen before mpv_terminate_destroy and while the GL context is still current. */
        mpv_render_context_free(a->render);
        a->render = NULL;
    }
    if (a->mpv) {
        mpv_terminate_destroy(a->mpv);
        a->mpv = NULL;
    }
    for (int i = 0; i < 2; i++) {
        if (a->wakeup_pipe[i] >= 0) {
            if (close(a->wakeup_pipe[i]) < 0)
                app_error(a, "close wakeup pipe: %s", strerror(errno));
            a->wakeup_pipe[i] = -1;
        }
    }
}
