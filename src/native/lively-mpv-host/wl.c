/* Wayland registry, output selection, layer surface / xdg toplevel and EGL plumbing. */
#include "app.h"

#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define WINDOW_APP_ID "lively-mpv-host"

static const char *egl_error_name(EGLint code)
{
    switch (code) {
    case EGL_SUCCESS: return "EGL_SUCCESS";
    case EGL_NOT_INITIALIZED: return "EGL_NOT_INITIALIZED";
    case EGL_BAD_ACCESS: return "EGL_BAD_ACCESS";
    case EGL_BAD_ALLOC: return "EGL_BAD_ALLOC";
    case EGL_BAD_ATTRIBUTE: return "EGL_BAD_ATTRIBUTE";
    case EGL_BAD_CONFIG: return "EGL_BAD_CONFIG";
    case EGL_BAD_CONTEXT: return "EGL_BAD_CONTEXT";
    case EGL_BAD_CURRENT_SURFACE: return "EGL_BAD_CURRENT_SURFACE";
    case EGL_BAD_DISPLAY: return "EGL_BAD_DISPLAY";
    case EGL_BAD_MATCH: return "EGL_BAD_MATCH";
    case EGL_BAD_NATIVE_PIXMAP: return "EGL_BAD_NATIVE_PIXMAP";
    case EGL_BAD_NATIVE_WINDOW: return "EGL_BAD_NATIVE_WINDOW";
    case EGL_BAD_PARAMETER: return "EGL_BAD_PARAMETER";
    case EGL_BAD_SURFACE: return "EGL_BAD_SURFACE";
    case EGL_CONTEXT_LOST: return "EGL_CONTEXT_LOST";
    default: return "unknown EGL error";
    }
}

static const char *egl_last_error(void)
{
    return egl_error_name(eglGetError());
}

static uint32_t min_version(uint32_t advertised, uint32_t wanted)
{
    return advertised < wanted ? advertised : wanted;
}

uint32_t wl_buffer_width(const struct app *a)
{
    return a->surface_w * (uint32_t)a->buffer_scale;
}

uint32_t wl_buffer_height(const struct app *a)
{
    return a->surface_h * (uint32_t)a->buffer_scale;
}

static struct output *find_output(struct app *a, const struct wl_output *wl_output)
{
    for (struct output *out = a->outputs; out; out = out->next) {
        if (out->wl_output == wl_output)
            return out;
    }
    return NULL;
}

static struct output *find_output_by_xdg(struct app *a, const struct zxdg_output_v1 *xdg_output)
{
    for (struct output *out = a->outputs; out; out = out->next) {
        if (out->xdg_output == xdg_output)
            return out;
    }
    return NULL;
}

/* Layer mode renders at the selected output's scale; a toplevel follows the outputs it overlaps. */
static int32_t desired_buffer_scale(const struct app *a)
{
    if (!a->windowed)
        return a->output->scale < 1 ? 1 : a->output->scale;
    int32_t scale = 1;
    for (const struct output *out = a->outputs; out; out = out->next) {
        if (out->entered && out->scale > scale)
            scale = out->scale;
    }
    return scale;
}

static void destroy_output(struct app *a, struct output *out)
{
    if (out->xdg_output)
        zxdg_output_v1_destroy(out->xdg_output);
    if (out->wl_output) {
        if (a->wl_output_version >= WL_OUTPUT_RELEASE_SINCE_VERSION)
            wl_output_release(out->wl_output);
        else
            wl_output_destroy(out->wl_output);
    }
    free(out->name);
    free(out->xdg_name);
    free(out);
}

static void unlink_output(struct app *a, struct output *out)
{
    struct output **link = &a->outputs;
    while (*link && *link != out)
        link = &(*link)->next;
    if (*link)
        *link = out->next;
    out->next = NULL;
}

static void apply_buffer_scale_change(struct app *a)
{
    if (!a->configured)
        return;
    int32_t scale = desired_buffer_scale(a);
    if (scale == a->buffer_scale)
        return;
    app_log(a, "buffer scale changed %d -> %d", a->buffer_scale, scale);
    a->buffer_scale = scale;
    wl_surface_set_buffer_scale(a->surface, a->buffer_scale);
    wl_egl_window_resize(a->egl_window, (int)wl_buffer_width(a), (int)wl_buffer_height(a), 0, 0);
    player_surface_resized(a);
    wl_request_redraw(a);
}

/* ---- wl_output ---- */

static void output_geometry(void *data, struct wl_output *wl_output, int32_t x, int32_t y,
                            int32_t physical_width, int32_t physical_height, int32_t subpixel,
                            const char *make, const char *model, int32_t transform)
{
    (void)data; (void)wl_output; (void)x; (void)y; (void)physical_width; (void)physical_height;
    (void)subpixel; (void)make; (void)model; (void)transform;
}

static void output_mode(void *data, struct wl_output *wl_output, uint32_t flags, int32_t width,
                        int32_t height, int32_t refresh)
{
    struct app *a = data;
    struct output *out = find_output(a, wl_output);
    (void)refresh;
    if (!out || !(flags & WL_OUTPUT_MODE_CURRENT))
        return;
    out->pixel_w = width;
    out->pixel_h = height;
}

static void output_done(void *data, struct wl_output *wl_output)
{
    struct app *a = data;
    struct output *out = find_output(a, wl_output);
    if (!out)
        return;
    if (out == a->output || (a->windowed && out->entered))
        apply_buffer_scale_change(a);
}

static void output_scale(void *data, struct wl_output *wl_output, int32_t factor)
{
    struct app *a = data;
    struct output *out = find_output(a, wl_output);
    if (!out)
        return;
    out->scale = factor < 1 ? 1 : factor;
}

static void output_name(void *data, struct wl_output *wl_output, const char *name)
{
    struct app *a = data;
    struct output *out = find_output(a, wl_output);
    if (!out)
        return;
    free(out->name);
    out->name = strdup(name);
    if (!out->name)
        app_error(a, "out of memory storing output name");
}

static void output_description(void *data, struct wl_output *wl_output, const char *description)
{
    (void)data; (void)wl_output; (void)description;
}

static const struct wl_output_listener output_listener = {
    .geometry = output_geometry,
    .mode = output_mode,
    .done = output_done,
    .scale = output_scale,
    .name = output_name,
    .description = output_description,
};

/* ---- zxdg_output_v1 ---- */

static void xdg_output_logical_position(void *data, struct zxdg_output_v1 *xdg_output, int32_t x, int32_t y)
{
    struct app *a = data;
    struct output *out = find_output_by_xdg(a, xdg_output);
    if (!out)
        return;
    out->logical_x = x;
    out->logical_y = y;
}

static void xdg_output_logical_size(void *data, struct zxdg_output_v1 *xdg_output, int32_t width, int32_t height)
{
    struct app *a = data;
    struct output *out = find_output_by_xdg(a, xdg_output);
    if (!out)
        return;
    out->logical_w = width;
    out->logical_h = height;
}

static void xdg_output_done(void *data, struct zxdg_output_v1 *xdg_output)
{
    (void)data; (void)xdg_output;
}

static void xdg_output_name(void *data, struct zxdg_output_v1 *xdg_output, const char *name)
{
    struct app *a = data;
    struct output *out = find_output_by_xdg(a, xdg_output);
    if (!out)
        return;
    free(out->xdg_name);
    out->xdg_name = strdup(name);
    if (!out->xdg_name)
        app_error(a, "out of memory storing xdg output name");
}

static void xdg_output_description(void *data, struct zxdg_output_v1 *xdg_output, const char *description)
{
    (void)data; (void)xdg_output; (void)description;
}

static const struct zxdg_output_v1_listener xdg_output_listener = {
    .logical_position = xdg_output_logical_position,
    .logical_size = xdg_output_logical_size,
    .done = xdg_output_done,
    .name = xdg_output_name,
    .description = xdg_output_description,
};

static int attach_xdg_output(struct app *a, struct output *out)
{
    out->xdg_output = zxdg_output_manager_v1_get_xdg_output(a->xdg_output_manager, out->wl_output);
    if (!out->xdg_output) {
        app_error(a, "zxdg_output_manager_v1.get_xdg_output failed");
        return -1;
    }
    zxdg_output_v1_add_listener(out->xdg_output, &xdg_output_listener, a);
    return 0;
}

/* ---- xdg_wm_base ---- */

static void wm_base_ping(void *data, struct xdg_wm_base *wm_base, uint32_t serial)
{
    (void)data;
    xdg_wm_base_pong(wm_base, serial);
}

static const struct xdg_wm_base_listener wm_base_listener = {
    .ping = wm_base_ping,
};

/* ---- registry ---- */

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version)
{
    struct app *a = data;
    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        a->compositor_version = min_version(version, 4);
        a->compositor = wl_registry_bind(registry, name, &wl_compositor_interface, a->compositor_version);
    } else if (strcmp(interface, zwlr_layer_shell_v1_interface.name) == 0) {
        a->layer_shell_version = min_version(version, 4);
        a->layer_shell = wl_registry_bind(registry, name, &zwlr_layer_shell_v1_interface, a->layer_shell_version);
    } else if (strcmp(interface, zxdg_output_manager_v1_interface.name) == 0) {
        a->xdg_output_manager_version = min_version(version, 3);
        a->xdg_output_manager = wl_registry_bind(registry, name, &zxdg_output_manager_v1_interface,
                                                 a->xdg_output_manager_version);
    } else if (strcmp(interface, xdg_wm_base_interface.name) == 0) {
        a->wm_base_version = min_version(version, 6);
        a->wm_base = wl_registry_bind(registry, name, &xdg_wm_base_interface, a->wm_base_version);
        if (a->wm_base)
            xdg_wm_base_add_listener(a->wm_base, &wm_base_listener, a);
    } else if (strcmp(interface, wl_output_interface.name) == 0) {
        if (a->output) {
            /* An output that appears after selection is a display change; the core restarts hosts for that. */
            app_log(a, "ignoring output global %u advertised after selection", name);
            return;
        }
        struct output *out = calloc(1, sizeof *out);
        if (!out) {
            app_error(a, "out of memory allocating output");
            return;
        }
        a->wl_output_version = min_version(version, 4);
        out->global_name = name;
        out->scale = 1;
        out->wl_output = wl_registry_bind(registry, name, &wl_output_interface, a->wl_output_version);
        if (!out->wl_output) {
            app_error(a, "wl_registry.bind(wl_output) failed");
            free(out);
            return;
        }
        wl_output_add_listener(out->wl_output, &output_listener, a);
        out->next = a->outputs;
        a->outputs = out;
        if (a->xdg_output_manager && attach_xdg_output(a, out) < 0)
            app_error(a, "xdg output for global %u unavailable", name);
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry, uint32_t name)
{
    struct app *a = data;
    (void)registry;
    if (a->output && a->output->global_name == name) {
        app_error(a, "output %s was removed", a->output_name);
        app_quit(a, HOST_EXIT_NO_OUTPUT);
        return;
    }
    for (struct output *out = a->outputs; out; out = out->next) {
        if (out->global_name == name) {
            unlink_output(a, out);
            destroy_output(a, out);
            if (a->windowed)
                apply_buffer_scale_change(a);
            return;
        }
    }
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

int wl_init(struct app *a)
{
    a->display = wl_display_connect(NULL);
    if (!a->display) {
        app_error(a, "cannot connect to the Wayland display: %s", strerror(errno));
        return HOST_EXIT_NO_PROTOCOL;
    }
    a->registry = wl_display_get_registry(a->display);
    if (!a->registry) {
        app_error(a, "wl_display.get_registry failed");
        return HOST_EXIT_NO_PROTOCOL;
    }
    wl_registry_add_listener(a->registry, &registry_listener, a);
    if (wl_display_roundtrip(a->display) < 0) {
        app_error(a, "Wayland roundtrip failed: %s", strerror(errno));
        return HOST_EXIT_NO_PROTOCOL;
    }
    if (!a->compositor) {
        app_error(a, "compositor does not provide wl_compositor");
        return HOST_EXIT_NO_PROTOCOL;
    }
    if (a->compositor_version < WL_SURFACE_SET_BUFFER_SCALE_SINCE_VERSION) {
        app_error(a, "wl_compositor version %u lacks wl_surface.set_buffer_scale", a->compositor_version);
        return HOST_EXIT_NO_PROTOCOL;
    }
    if (a->windowed) {
        if (!a->wm_base) {
            app_error(a, "compositor does not provide xdg_wm_base");
            return HOST_EXIT_NO_PROTOCOL;
        }
    } else {
        if (!a->layer_shell) {
            app_error(a, "compositor does not provide zwlr_layer_shell_v1");
            return HOST_EXIT_NO_PROTOCOL;
        }
        if (!a->xdg_output_manager) {
            app_error(a, "compositor does not provide zxdg_output_manager_v1");
            return HOST_EXIT_NO_PROTOCOL;
        }
    }
    if (a->xdg_output_manager) {
        for (struct output *out = a->outputs; out; out = out->next) {
            if (!out->xdg_output && attach_xdg_output(a, out) < 0)
                return HOST_EXIT_NO_PROTOCOL;
        }
    }
    if (wl_display_roundtrip(a->display) < 0) {
        app_error(a, "Wayland roundtrip failed: %s", strerror(errno));
        return HOST_EXIT_NO_PROTOCOL;
    }
    if (a->windowed) {
        app_log(a, "windowed mode: %ux%u \"%s\"", a->window_w, a->window_h, a->title);
        return 0;
    }

    for (struct output *out = a->outputs; out; out = out->next) {
        bool match = (out->xdg_name && strcmp(out->xdg_name, a->output_name) == 0) ||
                     (out->name && strcmp(out->name, a->output_name) == 0);
        if (match) {
            a->output = out;
            break;
        }
    }
    if (!a->output) {
        app_error(a, "output \"%s\" not found", a->output_name);
        for (struct output *out = a->outputs; out; out = out->next) {
            app_error(a, "  available: %s", out->xdg_name ? out->xdg_name : (out->name ? out->name : "(unnamed)"));
        }
        return HOST_EXIT_NO_OUTPUT;
    }
    app_log(a, "using output %s: logical %dx%d at %d,%d, %dx%d px, scale %d",
            a->output_name, a->output->logical_w, a->output->logical_h, a->output->logical_x,
            a->output->logical_y, a->output->pixel_w, a->output->pixel_h, a->output->scale);
    return 0;
}

/* ---- EGL ---- */

int egl_init(struct app *a)
{
    a->egl_display = eglGetPlatformDisplay(EGL_PLATFORM_WAYLAND_KHR, a->display, NULL);
    if (a->egl_display == EGL_NO_DISPLAY) {
        app_error(a, "eglGetPlatformDisplay failed: %s", egl_last_error());
        return HOST_EXIT_RENDERER;
    }
    EGLint major = 0, minor = 0;
    if (!eglInitialize(a->egl_display, &major, &minor)) {
        app_error(a, "eglInitialize failed: %s", egl_last_error());
        return HOST_EXIT_RENDERER;
    }
    if (!eglBindAPI(EGL_OPENGL_ES_API)) {
        app_error(a, "eglBindAPI(EGL_OPENGL_ES_API) failed: %s", egl_last_error());
        return HOST_EXIT_RENDERER;
    }
    static const EGLint config_attribs[] = {
        EGL_SURFACE_TYPE, EGL_WINDOW_BIT,
        EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE, 8,
        EGL_GREEN_SIZE, 8,
        EGL_BLUE_SIZE, 8,
        EGL_NONE,
    };
    EGLConfig configs[64];
    EGLint count = 0;
    if (!eglChooseConfig(a->egl_display, config_attribs, configs, 64, &count) || count < 1) {
        app_error(a, "eglChooseConfig found no RGB888 window config: %s", egl_last_error());
        return HOST_EXIT_RENDERER;
    }
    /* The wallpaper is opaque; prefer a config without an alpha channel. */
    a->egl_config = configs[0];
    for (EGLint i = 0; i < count; i++) {
        EGLint alpha = 0;
        if (eglGetConfigAttrib(a->egl_display, configs[i], EGL_ALPHA_SIZE, &alpha) && alpha == 0) {
            a->egl_config = configs[i];
            break;
        }
    }
    static const EGLint versions[] = { 3, 2 };
    EGLint chosen_version = 0;
    for (size_t i = 0; i < sizeof versions / sizeof versions[0]; i++) {
        const EGLint context_attribs[] = { EGL_CONTEXT_CLIENT_VERSION, versions[i], EGL_NONE };
        a->egl_context = eglCreateContext(a->egl_display, a->egl_config, EGL_NO_CONTEXT, context_attribs);
        if (a->egl_context != EGL_NO_CONTEXT) {
            chosen_version = versions[i];
            break;
        }
    }
    if (a->egl_context == EGL_NO_CONTEXT) {
        app_error(a, "eglCreateContext failed for OpenGL ES 3 and 2: %s", egl_last_error());
        return HOST_EXIT_RENDERER;
    }
    if (!eglMakeCurrent(a->egl_display, EGL_NO_SURFACE, EGL_NO_SURFACE, a->egl_context)) {
        app_error(a, "eglMakeCurrent (surfaceless) failed: %s", egl_last_error());
        return HOST_EXIT_RENDERER;
    }
    app_log(a, "EGL %d.%d initialised, OpenGL ES %d context", major, minor, chosen_version);
    return 0;
}

/* ---- rendering ---- */

static void frame_done(void *data, struct wl_callback *callback, uint32_t time)
{
    struct app *a = data;
    (void)time;
    wl_callback_destroy(callback);
    if (a->frame_callback == callback)
        a->frame_callback = NULL;
    if (a->redraw_needed && a->running)
        wl_render(a);
}

static const struct wl_callback_listener frame_listener = {
    .done = frame_done,
};

void wl_render(struct app *a)
{
    if (!a->configured || a->egl_surface == EGL_NO_SURFACE || !a->render)
        return;
    int width = (int)wl_buffer_width(a);
    int height = (int)wl_buffer_height(a);
    mpv_opengl_fbo fbo = { .fbo = 0, .w = width, .h = height, .internal_format = 0 };
    int flip_y = 1;
    int block_for_target_time = 0;
    mpv_render_param params[] = {
        { MPV_RENDER_PARAM_OPENGL_FBO, &fbo },
        { MPV_RENDER_PARAM_FLIP_Y, &flip_y },
        { MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME, &block_for_target_time },
        { MPV_RENDER_PARAM_INVALID, NULL },
    };
    glViewport(0, 0, width, height);
    int err = mpv_render_context_render(a->render, params);
    if (err < 0)
        app_error(a, "mpv_render_context_render: %s", mpv_error_string(err));

    if (a->frame_callback)
        wl_callback_destroy(a->frame_callback);
    a->frame_callback = wl_surface_frame(a->surface);
    if (!a->frame_callback) {
        app_error(a, "wl_surface.frame failed");
    } else {
        wl_callback_add_listener(a->frame_callback, &frame_listener, a);
    }
    a->redraw_needed = false;

    if (!eglSwapBuffers(a->egl_display, a->egl_surface)) {
        app_error(a, "eglSwapBuffers failed: %s", egl_last_error());
        if (a->frame_callback) {
            wl_callback_destroy(a->frame_callback);
            a->frame_callback = NULL;
        }
        return;
    }
    mpv_render_context_report_swap(a->render);
}

void wl_request_redraw(struct app *a)
{
    a->redraw_needed = true;
    if (a->configured && !a->frame_callback)
        wl_render(a);
}

/* Shared by the layer surface and the xdg toplevel: apply a configured size, then draw and commit. */
static void surface_configured(struct app *a, uint32_t width, uint32_t height)
{
    if (width == 0 || height == 0) {
        app_error(a, "surface configured with no usable size");
        app_quit(a, HOST_EXIT_RENDERER);
        return;
    }
    a->surface_w = width;
    a->surface_h = height;
    a->buffer_scale = desired_buffer_scale(a);
    wl_surface_set_buffer_scale(a->surface, a->buffer_scale);

    struct wl_region *opaque = wl_compositor_create_region(a->compositor);
    if (!opaque) {
        app_error(a, "wl_compositor.create_region failed");
        app_quit(a, HOST_EXIT_RENDERER);
        return;
    }
    wl_region_add(opaque, 0, 0, (int32_t)width, (int32_t)height);
    wl_surface_set_opaque_region(a->surface, opaque);
    wl_region_destroy(opaque);

    int buffer_w = (int)wl_buffer_width(a);
    int buffer_h = (int)wl_buffer_height(a);
    if (!a->egl_window) {
        a->egl_window = wl_egl_window_create(a->surface, buffer_w, buffer_h);
        if (!a->egl_window) {
            app_error(a, "wl_egl_window_create failed");
            app_quit(a, HOST_EXIT_RENDERER);
            return;
        }
        a->egl_surface = eglCreatePlatformWindowSurface(a->egl_display, a->egl_config, a->egl_window, NULL);
        if (a->egl_surface == EGL_NO_SURFACE) {
            app_error(a, "eglCreatePlatformWindowSurface failed: %s", egl_last_error());
            app_quit(a, HOST_EXIT_RENDERER);
            return;
        }
        if (!eglMakeCurrent(a->egl_display, a->egl_surface, a->egl_surface, a->egl_context)) {
            app_error(a, "eglMakeCurrent failed: %s", egl_last_error());
            app_quit(a, HOST_EXIT_RENDERER);
            return;
        }
        /* Pacing is done with wl_surface.frame; a blocking swap would stall the event loop. */
        if (!eglSwapInterval(a->egl_display, 0))
            app_error(a, "eglSwapInterval(0) failed: %s", egl_last_error());
    } else {
        wl_egl_window_resize(a->egl_window, buffer_w, buffer_h, 0, 0);
    }
    app_log(a, "configured %ux%u logical, %dx%d px (scale %d)", width, height, buffer_w, buffer_h, a->buffer_scale);
    a->configured = true;
    player_surface_resized(a);
    wl_render(a);
    if (!a->hwnd_sent) {
        a->hwnd_sent = true;
        ipc_send_hwnd(a);
    }
}

/* ---- layer surface ---- */

static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *layer_surface,
                                    uint32_t serial, uint32_t width, uint32_t height)
{
    struct app *a = data;
    zwlr_layer_surface_v1_ack_configure(layer_surface, serial);
    if (width == 0)
        width = a->output->logical_w > 0 ? (uint32_t)a->output->logical_w : (uint32_t)a->output->pixel_w;
    if (height == 0)
        height = a->output->logical_h > 0 ? (uint32_t)a->output->logical_h : (uint32_t)a->output->pixel_h;
    surface_configured(a, width, height);
}

static void layer_surface_closed(void *data, struct zwlr_layer_surface_v1 *layer_surface)
{
    struct app *a = data;
    (void)layer_surface;
    app_error(a, "layer surface closed by the compositor");
    app_quit(a, HOST_EXIT_NO_OUTPUT);
}

static const struct zwlr_layer_surface_v1_listener layer_surface_listener = {
    .configure = layer_surface_configure,
    .closed = layer_surface_closed,
};

static int create_layer_surface(struct app *a)
{
    if (!a->interactive) {
        struct wl_region *empty = wl_compositor_create_region(a->compositor);
        if (!empty) {
            app_error(a, "wl_compositor.create_region failed");
            return HOST_EXIT_RENDERER;
        }
        wl_surface_set_input_region(a->surface, empty);
        wl_region_destroy(empty);
    }
    a->layer_surface = zwlr_layer_shell_v1_get_layer_surface(a->layer_shell, a->surface, a->output->wl_output,
                                                             a->layer, a->layer_namespace);
    if (!a->layer_surface) {
        app_error(a, "zwlr_layer_shell_v1.get_layer_surface failed");
        return HOST_EXIT_RENDERER;
    }
    zwlr_layer_surface_v1_set_size(a->layer_surface, 0, 0);
    zwlr_layer_surface_v1_set_anchor(a->layer_surface,
                                     ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP | ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
                                     ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT | ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
    zwlr_layer_surface_v1_set_exclusive_zone(a->layer_surface, -1);
    zwlr_layer_surface_v1_set_keyboard_interactivity(a->layer_surface,
                                                     ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
    zwlr_layer_surface_v1_add_listener(a->layer_surface, &layer_surface_listener, a);
    wl_surface_commit(a->surface);
    app_log(a, "layer surface created on layer %u, namespace \"%s\", input %s", (unsigned)a->layer,
            a->layer_namespace, a->interactive ? "enabled" : "pass-through");
    return 0;
}

/* ---- xdg toplevel (windowed mode) ---- */

static void surface_enter(void *data, struct wl_surface *surface, struct wl_output *wl_output)
{
    struct app *a = data;
    struct output *out = find_output(a, wl_output);
    (void)surface;
    if (!out)
        return;
    out->entered = true;
    app_log(a, "window entered output %s (scale %d)", out->name ? out->name : "?", out->scale);
    apply_buffer_scale_change(a);
}

static void surface_leave(void *data, struct wl_surface *surface, struct wl_output *wl_output)
{
    struct app *a = data;
    struct output *out = find_output(a, wl_output);
    (void)surface;
    if (!out)
        return;
    out->entered = false;
    app_log(a, "window left output %s", out->name ? out->name : "?");
    apply_buffer_scale_change(a);
}

static void surface_preferred_buffer_scale(void *data, struct wl_surface *surface, int32_t factor)
{
    (void)data; (void)surface; (void)factor;
}

static void surface_preferred_buffer_transform(void *data, struct wl_surface *surface, uint32_t transform)
{
    (void)data; (void)surface; (void)transform;
}

static const struct wl_surface_listener surface_listener = {
    .enter = surface_enter,
    .leave = surface_leave,
    .preferred_buffer_scale = surface_preferred_buffer_scale,
    .preferred_buffer_transform = surface_preferred_buffer_transform,
};

static void xdg_surface_configure(void *data, struct xdg_surface *xdg_surface, uint32_t serial)
{
    struct app *a = data;
    xdg_surface_ack_configure(xdg_surface, serial);
    uint32_t width = a->pending_w ? a->pending_w : (a->surface_w ? a->surface_w : a->window_w);
    uint32_t height = a->pending_h ? a->pending_h : (a->surface_h ? a->surface_h : a->window_h);
    surface_configured(a, width, height);
}

static const struct xdg_surface_listener xdg_surface_listener = {
    .configure = xdg_surface_configure,
};

static void toplevel_configure(void *data, struct xdg_toplevel *toplevel, int32_t width, int32_t height,
                               struct wl_array *states)
{
    struct app *a = data;
    (void)toplevel; (void)states;
    a->pending_w = width > 0 ? (uint32_t)width : 0;
    a->pending_h = height > 0 ? (uint32_t)height : 0;
}

static void toplevel_close(void *data, struct xdg_toplevel *toplevel)
{
    struct app *a = data;
    (void)toplevel;
    app_log(a, "window closed");
    app_quit(a, HOST_EXIT_OK);
}

static void toplevel_configure_bounds(void *data, struct xdg_toplevel *toplevel, int32_t width, int32_t height)
{
    (void)data; (void)toplevel; (void)width; (void)height;
}

static void toplevel_wm_capabilities(void *data, struct xdg_toplevel *toplevel, struct wl_array *capabilities)
{
    (void)data; (void)toplevel; (void)capabilities;
}

static const struct xdg_toplevel_listener toplevel_listener = {
    .configure = toplevel_configure,
    .close = toplevel_close,
    .configure_bounds = toplevel_configure_bounds,
    .wm_capabilities = toplevel_wm_capabilities,
};

static int create_toplevel(struct app *a)
{
    wl_surface_add_listener(a->surface, &surface_listener, a);
    a->xdg_surface = xdg_wm_base_get_xdg_surface(a->wm_base, a->surface);
    if (!a->xdg_surface) {
        app_error(a, "xdg_wm_base.get_xdg_surface failed");
        return HOST_EXIT_RENDERER;
    }
    xdg_surface_add_listener(a->xdg_surface, &xdg_surface_listener, a);
    a->xdg_toplevel = xdg_surface_get_toplevel(a->xdg_surface);
    if (!a->xdg_toplevel) {
        app_error(a, "xdg_surface.get_toplevel failed");
        return HOST_EXIT_RENDERER;
    }
    xdg_toplevel_add_listener(a->xdg_toplevel, &toplevel_listener, a);
    xdg_toplevel_set_title(a->xdg_toplevel, a->title);
    xdg_toplevel_set_app_id(a->xdg_toplevel, WINDOW_APP_ID);
    wl_surface_commit(a->surface);
    app_log(a, "toplevel created: %ux%u, title \"%s\", app_id %s", a->window_w, a->window_h, a->title, WINDOW_APP_ID);
    return 0;
}

int wl_create_surface(struct app *a)
{
    a->surface = wl_compositor_create_surface(a->compositor);
    if (!a->surface) {
        app_error(a, "wl_compositor.create_surface failed");
        return HOST_EXIT_RENDERER;
    }
    return a->windowed ? create_toplevel(a) : create_layer_surface(a);
}

void wl_shutdown(struct app *a)
{
    if (a->egl_display != EGL_NO_DISPLAY) {
        if (!eglMakeCurrent(a->egl_display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT))
            app_error(a, "eglMakeCurrent(none) failed: %s", egl_last_error());
        if (a->egl_surface != EGL_NO_SURFACE && !eglDestroySurface(a->egl_display, a->egl_surface))
            app_error(a, "eglDestroySurface failed: %s", egl_last_error());
        a->egl_surface = EGL_NO_SURFACE;
        if (a->egl_context != EGL_NO_CONTEXT && !eglDestroyContext(a->egl_display, a->egl_context))
            app_error(a, "eglDestroyContext failed: %s", egl_last_error());
        a->egl_context = EGL_NO_CONTEXT;
        if (!eglTerminate(a->egl_display))
            app_error(a, "eglTerminate failed: %s", egl_last_error());
        a->egl_display = EGL_NO_DISPLAY;
    }
    if (a->egl_window) {
        wl_egl_window_destroy(a->egl_window);
        a->egl_window = NULL;
    }
    if (a->frame_callback) {
        wl_callback_destroy(a->frame_callback);
        a->frame_callback = NULL;
    }
    if (a->layer_surface) {
        zwlr_layer_surface_v1_destroy(a->layer_surface);
        a->layer_surface = NULL;
    }
    if (a->xdg_toplevel) {
        xdg_toplevel_destroy(a->xdg_toplevel);
        a->xdg_toplevel = NULL;
    }
    if (a->xdg_surface) {
        xdg_surface_destroy(a->xdg_surface);
        a->xdg_surface = NULL;
    }
    if (a->surface) {
        wl_surface_destroy(a->surface);
        a->surface = NULL;
    }
    while (a->outputs) {
        struct output *out = a->outputs;
        a->outputs = out->next;
        destroy_output(a, out);
    }
    a->output = NULL;
    if (a->wm_base) {
        xdg_wm_base_destroy(a->wm_base);
        a->wm_base = NULL;
    }
    if (a->xdg_output_manager) {
        zxdg_output_manager_v1_destroy(a->xdg_output_manager);
        a->xdg_output_manager = NULL;
    }
    if (a->layer_shell) {
        if (a->layer_shell_version >= ZWLR_LAYER_SHELL_V1_DESTROY_SINCE_VERSION)
            zwlr_layer_shell_v1_destroy(a->layer_shell);
        else
            wl_proxy_destroy((struct wl_proxy *)a->layer_shell);
        a->layer_shell = NULL;
    }
    if (a->compositor) {
        wl_compositor_destroy(a->compositor);
        a->compositor = NULL;
    }
    if (a->registry) {
        wl_registry_destroy(a->registry);
        a->registry = NULL;
    }
    if (a->display) {
        if (wl_display_flush(a->display) < 0 && errno != EAGAIN)
            app_log(a, "final Wayland flush failed: %s", strerror(errno));
        wl_display_disconnect(a->display);
        a->display = NULL;
    }
}
