#include "args.h"

#include <math.h>
#include <string.h>

#define LWH_ARGS_ERROR lwh_args_error_quark()

static GQuark lwh_args_error_quark(void)
{
    return g_quark_from_static_string("lively-web-host-args");
}

static gboolean parse_int_strict(const gchar *text, gint *out)
{
    gchar *end = NULL;
    gint64 v;

    if (text == NULL || *text == '\0')
        return FALSE;
    v = g_ascii_strtoll(text, &end, 10);
    if (end == NULL || *end != '\0' || v < G_MININT || v > G_MAXINT)
        return FALSE;
    *out = (gint)v;
    return TRUE;
}

static gboolean parse_span(LwhArgs *a, const gchar *text, GError **error)
{
    gchar **parts = g_strsplit(text, ",", -1);
    gint values[6];
    gboolean ok = g_strv_length(parts) == 6;

    for (gint i = 0; ok && i < 6; i++)
        ok = parse_int_strict(parts[i], &values[i]);
    g_strfreev(parts);

    if (!ok) {
        g_set_error(error, LWH_ARGS_ERROR, 0,
                    "--span expects six integers X,Y,W,H,VW,VH (got \"%s\")", text);
        return FALSE;
    }
    if (values[2] <= 0 || values[3] <= 0 || values[4] <= 0 || values[5] <= 0) {
        g_set_error(error, LWH_ARGS_ERROR, 0,
                    "--span sizes W,H,VW,VH must be positive (got \"%s\")", text);
        return FALSE;
    }
    a->span = TRUE;
    a->span_x = values[0];
    a->span_y = values[1];
    a->span_w = values[2];
    a->span_h = values[3];
    a->span_vw = values[4];
    a->span_vh = values[5];
    return TRUE;
}

static gboolean parse_windowed(LwhArgs *a, const gchar *text, GError **error)
{
    gchar **parts = g_strsplit(text, "x", -1);
    gboolean ok = g_strv_length(parts) == 2 &&
                  parse_int_strict(parts[0], &a->window_w) &&
                  parse_int_strict(parts[1], &a->window_h) &&
                  a->window_w > 0 && a->window_h > 0;

    g_strfreev(parts);
    if (!ok) {
        g_set_error(error, LWH_ARGS_ERROR, 0, "--windowed expects WIDTHxHEIGHT with positive sizes (got \"%s\")", text);
        return FALSE;
    }
    a->windowed = TRUE;
    return TRUE;
}

static gboolean looks_like_url(const gchar *target)
{
    return g_regex_match_simple("^[A-Za-z][A-Za-z0-9+.-]*://", target, 0, 0);
}

gboolean lwh_args_parse(LwhArgs *a, int argc, char **argv, GError **error)
{
    gchar *layer = NULL, *span = NULL, *type = NULL, *scheme = NULL, *windowed = NULL;
    gchar **rest = NULL;
    gchar **argv_copy;
    GOptionContext *ctx;
    gboolean ok;

    memset(a, 0, sizeof(*a));
    a->scale = NAN;

    GOptionEntry entries[] = {
        { "output", 0, 0, G_OPTION_ARG_STRING, &a->output,
          "Wayland output name as reported by lively-wl-monitor (required)", "NAME" },
        { "namespace", 0, 0, G_OPTION_ARG_STRING, &a->ns,
          "layer-shell namespace (default \"lively-wallpaper\")", "NAME" },
        { "layer", 0, 0, G_OPTION_ARG_STRING, &layer,
          "background|bottom (default background)", "LAYER" },
        { "span", 0, 0, G_OPTION_ARG_STRING, &span,
          "Span mode: this output covers X,Y,W,H of a VWxVH virtual screen", "X,Y,W,H,VW,VH" },
        { "interactive", 0, 0, G_OPTION_ARG_NONE, &a->interactive,
          "Give the surface a full input region", NULL },
        { "windowed", 0, 0, G_OPTION_ARG_STRING, &windowed,
          "Preview mode: render in a normal window of this size instead of a layer surface", "WxH" },
        { "title", 0, 0, G_OPTION_ARG_STRING, &a->title,
          "Window title in windowed mode (default \"Lively Wallpaper\")", "TEXT" },
        { "property", 0, 0, G_OPTION_ARG_FILENAME, &a->property,
          "Path to the wallpaper's LivelyProperties.json copy", "PATH" },
        { "volume", 0, 0, G_OPTION_ARG_INT, &a->volume,
          "Initial volume 0-100 (default 0)", "N" },
        { "verbose", 0, 0, G_OPTION_ARG_NONE, &a->verbose,
          "Log every stdin line and internal event to stderr", NULL },
        { "type", 0, 0, G_OPTION_ARG_STRING, &type,
          "local|online (default: online when the target has a URL scheme, else local)", "TYPE" },
        { "debug", 0, 0, G_OPTION_ARG_INT, &a->debug_port,
          "Enable developer extras, context menu and the remote inspector on PORT", "PORT" },
        { "color-scheme", 0, 0, G_OPTION_ARG_STRING, &scheme,
          "dark|light preferred colour scheme", "SCHEME" },
        { "pause-media", 0, 0, G_OPTION_ARG_NONE, &a->pause_media,
          "Pause <video>/<audio> elements on cmd_suspend", NULL },
        { "audio", 0, 0, G_OPTION_ARG_NONE, &a->audio,
          "Forward lsp_audio frames to livelyAudioListener", NULL },
        { "sysinfo", 0, 0, G_OPTION_ARG_NONE, &a->sysinfo,
          "Wallpaper uses livelySystemInformation", NULL },
        { "nowplaying", 0, 0, G_OPTION_ARG_NONE, &a->nowplaying,
          "Wallpaper uses livelyCurrentTrack", NULL },
        { "pause-event", 0, 0, G_OPTION_ARG_NONE, &a->pause_event,
          "Call livelyWallpaperPlaybackChanged on suspend/resume", NULL },
        { "scale", 0, 0, G_OPTION_ARG_DOUBLE, &a->scale,
          "Zoom level so CSS pixels match the output's logical pixels", "F" },
        { "user-data", 0, 0, G_OPTION_ARG_FILENAME, &a->user_data,
          "Website data directory (cache, local storage)", "DIR" },
        { G_OPTION_REMAINING, 0, 0, G_OPTION_ARG_FILENAME_ARRAY, &rest, NULL, "<path-or-url>" },
        { NULL, 0, 0, 0, NULL, NULL, NULL }
    };

    ctx = g_option_context_new("<path-or-url> - Lively HTML wallpaper host");
    g_option_context_add_main_entries(ctx, entries, NULL);
    argv_copy = g_strdupv(argv);
    ok = g_option_context_parse_strv(ctx, &argv_copy, error);
    g_strfreev(argv_copy);
    g_option_context_free(ctx);
    (void)argc;

    if (ok && windowed != NULL)
        ok = parse_windowed(a, windowed, error);
    if (ok && !a->windowed && (a->output == NULL || *a->output == '\0')) {
        g_set_error(error, LWH_ARGS_ERROR, 0, "--output NAME is required (or use --windowed WxH)");
        ok = FALSE;
    }
    if (ok && a->title == NULL)
        a->title = g_strdup("Lively Wallpaper");
    if (ok) {
        if (a->ns == NULL)
            a->ns = g_strdup("lively-wallpaper");
        if (layer == NULL || g_strcmp0(layer, "background") == 0) {
            a->layer_bottom = FALSE;
        } else if (g_strcmp0(layer, "bottom") == 0) {
            a->layer_bottom = TRUE;
        } else {
            g_set_error(error, LWH_ARGS_ERROR, 0, "--layer must be background or bottom (got \"%s\")", layer);
            ok = FALSE;
        }
    }
    if (ok && span != NULL && !a->windowed)
        ok = parse_span(a, span, error);
    if (ok && a->windowed) {
        /* A preview window always takes input; layer placement does not apply. */
        a->interactive = TRUE;
        a->span = FALSE;
    }
    if (ok && (a->volume < 0 || a->volume > 100)) {
        g_set_error(error, LWH_ARGS_ERROR, 0, "--volume must be between 0 and 100 (got %d)", a->volume);
        ok = FALSE;
    }
    if (ok && (a->debug_port < 0 || a->debug_port > 65535)) {
        g_set_error(error, LWH_ARGS_ERROR, 0, "--debug PORT must be between 1 and 65535 (got %d)", a->debug_port);
        ok = FALSE;
    }
    if (ok) {
        if (scheme == NULL) {
            a->scheme = LWH_SCHEME_AUTO;
        } else if (g_strcmp0(scheme, "dark") == 0) {
            a->scheme = LWH_SCHEME_DARK;
        } else if (g_strcmp0(scheme, "light") == 0) {
            a->scheme = LWH_SCHEME_LIGHT;
        } else {
            g_set_error(error, LWH_ARGS_ERROR, 0, "--color-scheme must be dark or light (got \"%s\")", scheme);
            ok = FALSE;
        }
    }
    if (ok) {
        a->scale_given = !isnan(a->scale);
        if (a->scale_given && !(a->scale > 0.0 && isfinite(a->scale))) {
            g_set_error(error, LWH_ARGS_ERROR, 0, "--scale must be a positive number");
            ok = FALSE;
        }
    }
    if (ok) {
        if (rest == NULL || rest[0] == NULL) {
            g_set_error(error, LWH_ARGS_ERROR, 0, "missing <path-or-url> argument");
            ok = FALSE;
        } else if (rest[1] != NULL) {
            g_set_error(error, LWH_ARGS_ERROR, 0, "exactly one <path-or-url> argument is expected");
            ok = FALSE;
        } else {
            a->target = g_strdup(rest[0]);
        }
    }
    if (ok) {
        if (type == NULL) {
            a->type = looks_like_url(a->target) ? LWH_PAGE_ONLINE : LWH_PAGE_LOCAL;
        } else if (g_strcmp0(type, "local") == 0) {
            a->type = LWH_PAGE_LOCAL;
        } else if (g_strcmp0(type, "online") == 0) {
            a->type = LWH_PAGE_ONLINE;
        } else {
            g_set_error(error, LWH_ARGS_ERROR, 0, "--type must be local or online (got \"%s\")", type);
            ok = FALSE;
        }
    }

    g_free(layer);
    g_free(span);
    g_free(windowed);
    g_free(type);
    g_free(scheme);
    g_strfreev(rest);
    if (!ok)
        lwh_args_clear(a);
    return ok;
}

void lwh_args_clear(LwhArgs *a)
{
    g_free(a->output);
    g_free(a->ns);
    g_free(a->title);
    g_free(a->property);
    g_free(a->user_data);
    g_free(a->target);
    memset(a, 0, sizeof(*a));
}
