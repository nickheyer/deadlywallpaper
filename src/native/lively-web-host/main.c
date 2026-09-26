/*
 * lively-web-host: renders an HTML/URL wallpaper with WebKitGTK inside a
 * wlr-layer-shell surface (via gtk-layer-shell) on one Wayland output and
 * speaks the JSON line protocol from src/native/PROTOCOL.md on stdin/stdout.
 *
 * Behaviour mirrors Lively.Player.WebView2/Form1.cs.
 */

#include <gdk/gdkwayland.h>
#include <gio/gio.h>
#include <glib-unix.h>
#include <gtk-layer-shell.h>
#include <gtk/gtk.h>
#include <json-glib/json-glib.h>
#include <stdio.h>
#include <string.h>
#include <webkit2/webkit2.h>

#include "args.h"
#include "ipc.h"
#include "output_match.h"
#include "props.h"
#include "stream_util.h"

enum {
    EXIT_OK = 0,
    EXIT_BAD_ARGS = 2,
    EXIT_NO_PROTOCOL = 3,
    EXIT_NO_OUTPUT = 4,
    EXIT_RENDERER = 5,
    EXIT_LOAD = 6
};

typedef struct {
    LwhArgs args;
    gchar *page_path;              /* canonical path of a local page, else NULL */
    gchar *page_dir;               /* directory of a local page, else NULL */
    gboolean video_stream;         /* target is a YouTube link (Form1.isVideoStream) */

    GdkDisplay *display;
    GdkMonitor *monitor;
    GtkWidget *window;
    GtkWidget *overlay;
    GtkWidget *image;
    WebKitWebContext *context;
    WebKitWebView *view;
    GtkWidget *inspector_window;

    gboolean load_failed;
    gboolean load_cancelled;

    gboolean paused;
    cairo_surface_t *pause_surface;
    gchar *last_nowplaying;        /* JSON of the last lsp_nowplaying Info */

    GIOChannel *stdin_channel;
    guint stdin_watch;
    gboolean quitting;
    int exit_code;
} App;

static const gchar console_bridge_script[] =
    "(function () {\n"
    "  if (!window.webkit || !window.webkit.messageHandlers || !window.webkit.messageHandlers.lively) return;\n"
    "  var handler = window.webkit.messageHandlers.lively;\n"
    "  function format(value) {\n"
    "    if (typeof value === 'string') return value;\n"
    "    if (value === undefined) return 'undefined';\n"
    "    if (value instanceof Error) return value.name + ': ' + value.message;\n"
    "    try { var s = JSON.stringify(value); return s === undefined ? String(value) : s; } catch (e) { return String(value); }\n"
    "  }\n"
    "  function post(level, args) {\n"
    "    try {\n"
    "      var parts = [];\n"
    "      for (var i = 0; i < args.length; i++) parts.push(format(args[i]));\n"
    "      handler.postMessage(JSON.stringify({ level: level, text: parts.join(' ') }));\n"
    "    } catch (e) {}\n"
    "  }\n"
    "  var console = window.console;\n"
    "  ['log', 'info', 'debug', 'warn', 'error'].forEach(function (level) {\n"
    "    var original = console[level];\n"
    "    console[level] = function () {\n"
    "      post(level, arguments);\n"
    "      if (typeof original === 'function') return original.apply(console, arguments);\n"
    "    };\n"
    "  });\n"
    "  window.addEventListener('error', function (ev) {\n"
    "    post('error', [ev.message + ' (' + ev.filename + ':' + ev.lineno + ')']);\n"
    "  });\n"
    "  window.addEventListener('unhandledrejection', function (ev) {\n"
    "    post('error', ['Unhandled promise rejection: ' + format(ev.reason)]);\n"
    "  });\n"
    "})();\n";

static const gchar pause_media_script[] =
    "document.querySelectorAll('video, audio').forEach(mediaElement => mediaElement.pause());";
static const gchar play_media_script[] =
    "document.querySelectorAll('video, audio').forEach(mediaElement => mediaElement.play());";

/* ------------------------------------------------------------------------- */
/* logging / exit                                                             */

static void logv(App *app, const gchar *format, ...) G_GNUC_PRINTF(2, 3);

static void logv(App *app, const gchar *format, ...)
{
    va_list ap;

    if (!app->args.verbose)
        return;
    va_start(ap, format);
    fputs("[lively-web-host] ", stderr);
    vfprintf(stderr, format, ap);
    fputc('\n', stderr);
    va_end(ap);
}

static void request_exit(App *app, int code, const gchar *reason)
{
    if (app->quitting)
        return;
    app->quitting = TRUE;
    app->exit_code = code;
    logv(app, "exiting with code %d: %s", code, reason);
    if (app->stdin_watch != 0) {
        g_source_remove(app->stdin_watch);
        app->stdin_watch = 0;
    }
    gtk_main_quit();
}

/* ------------------------------------------------------------------------- */
/* JavaScript bridge                                                          */

static void on_js_done(GObject *source, GAsyncResult *result, gpointer user_data)
{
    App *app = user_data;
    GError *error = NULL;
    JSCValue *value = webkit_web_view_evaluate_javascript_finish(WEBKIT_WEB_VIEW(source), result, &error);

    if (value == NULL) {
        logv(app, "javascript: %s", error != NULL ? error->message : "unknown error");
        g_clear_error(&error);
        return;
    }
    g_object_unref(value);
}

/* Equivalent of CoreWebView2Extensions.ExecuteScriptFunctionAsync, guarded as PROTOCOL.md requires. */
static void js_call(App *app, const gchar *function, const gchar *json_arguments)
{
    gchar *script = g_strdup_printf("if (typeof %s === 'function') { %s(%s); }",
                                    function, function, json_arguments);

    logv(app, "js: %s", script);
    webkit_web_view_evaluate_javascript(app->view, script, -1, NULL, NULL, NULL, on_js_done, app);
    g_free(script);
}

static gchar *json_string(const gchar *text)
{
    JsonNode *node = json_node_new(text != NULL ? JSON_NODE_VALUE : JSON_NODE_NULL);
    gchar *out;

    if (text != NULL)
        json_node_set_string(node, text);
    out = json_to_string(node, FALSE);
    json_node_unref(node);
    return out;
}

static void js_call_name_value(App *app, const gchar *function, const gchar *name, JsonNode *value)
{
    gchar *name_json = json_string(name);
    gchar *value_json = json_to_string(value, FALSE);
    gchar *arguments = g_strconcat(name_json, ", ", value_json, NULL);

    js_call(app, function, arguments);
    g_free(arguments);
    g_free(value_json);
    g_free(name_json);
}

static void js_call_node(App *app, const gchar *function, JsonNode *value)
{
    gchar *value_json = json_to_string(value, FALSE);

    js_call(app, function, value_json);
    g_free(value_json);
}

static void js_run(App *app, const gchar *script)
{
    logv(app, "js: %s", script);
    webkit_web_view_evaluate_javascript(app->view, script, -1, NULL, NULL, NULL, on_js_done, app);
}

static void playback_changed(App *app, gboolean is_paused)
{
    js_call(app, "livelyWallpaperPlaybackChanged", is_paused ? "{\"IsPaused\":true}" : "{\"IsPaused\":false}");
}

/* ------------------------------------------------------------------------- */
/* LivelyProperties.json                                                      */

static void apply_property(const gchar *name, JsonNode *value, gpointer user_data)
{
    js_call_name_value(user_data, "livelyPropertyListener", name, value);
}

/* Form1.RestoreLivelyProperties */
static void restore_properties(App *app)
{
    GError *error = NULL;

    if (!lwh_props_load(app->args.property, app->page_dir, apply_property, app, &error)) {
        lwh_ipc_send_console(LWH_CONSOLE_ERROR, "%s", error->message);
        g_error_free(error);
    }
}

/* ------------------------------------------------------------------------- */
/* snapshots                                                                  */

/* Captures what this output currently shows: the web view widget is drawn into
 * an image surface the size of the output (in span mode only its (X,Y,W,H)
 * slice), at the window's scale factor so the GtkImage shows it at physical
 * resolution. Drawing the widget reads the composited frame back from WebKit's
 * GL texture, so WebGL, video and canvas content are included. */
static cairo_surface_t *capture_view_surface(App *app)
{
    int out_w = gtk_widget_get_allocated_width(app->overlay);
    int out_h = gtk_widget_get_allocated_height(app->overlay);
    int scale_factor = gtk_widget_get_scale_factor(app->window);
    cairo_surface_t *dst;
    cairo_t *cr;

    if (out_w <= 0 || out_h <= 0 || !gtk_widget_is_drawable(GTK_WIDGET(app->view)))
        return NULL;
    dst = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, out_w * scale_factor, out_h * scale_factor);
    if (cairo_surface_status(dst) != CAIRO_STATUS_SUCCESS) {
        cairo_surface_destroy(dst);
        return NULL;
    }
    cairo_surface_set_device_scale(dst, scale_factor, scale_factor);
    cr = cairo_create(dst);
    if (app->args.span)
        cairo_translate(cr, -app->args.span_x, -app->args.span_y);
    gtk_widget_draw(GTK_WIDGET(app->view), cr);
    cairo_destroy(cr);
    cairo_surface_flush(dst);
    return dst;
}

static void show_pause_snapshot(App *app)
{
    cairo_surface_t *display = capture_view_surface(app);

    if (display == NULL) {
        lwh_ipc_send_console(LWH_CONSOLE_ERROR, "Failed to capture the page for pause");
    } else {
        if (app->pause_surface != NULL)
            cairo_surface_destroy(app->pause_surface);
        app->pause_surface = display;
        gtk_image_set_from_surface(GTK_IMAGE(app->image), display);
        gtk_widget_show(app->image);
        logv(app, "pause snapshot shown (%dx%d px)", cairo_image_surface_get_width(display),
             cairo_image_surface_get_height(display));
    }
    /* Hiding the widget makes WebKit treat the page as hidden: rAF and timers stop. */
    gtk_widget_hide(GTK_WIDGET(app->view));
}

/* Form1: cmd_suspend */
static void handle_suspend(App *app)
{
    if (app->paused)
        return;
    if (app->args.pause_media || app->video_stream)
        js_run(app, pause_media_script);
    if (app->args.pause_event)
        playback_changed(app, TRUE);
    app->paused = TRUE;
    show_pause_snapshot(app);
}

/* Form1: cmd_resume */
static void handle_resume(App *app)
{
    if (!app->paused)
        return;
    app->paused = FALSE;
    gtk_widget_show(GTK_WIDGET(app->view));
    gtk_widget_hide(app->image);
    gtk_image_clear(GTK_IMAGE(app->image));
    if (app->pause_surface != NULL) {
        cairo_surface_destroy(app->pause_surface);
        app->pause_surface = NULL;
    }
    if (app->args.pause_media || app->video_stream)
        js_run(app, play_media_script);
    if (app->args.pause_event)
        playback_changed(app, FALSE);
    if (app->args.nowplaying)
        js_call(app, "livelyCurrentTrack", app->last_nowplaying != NULL ? app->last_nowplaying : "null");
}

/* ------------------------------------------------------------------------- */
/* screenshots                                                                */

static gboolean pixbuf_can_write(const gchar *format_name)
{
    GSList *formats = gdk_pixbuf_get_formats();
    gboolean writable = FALSE;

    for (GSList *l = formats; l != NULL; l = l->next) {
        GdkPixbufFormat *format = l->data;
        gchar *name = gdk_pixbuf_format_get_name(format);

        if (g_strcmp0(name, format_name) == 0 && gdk_pixbuf_format_is_writable(format))
            writable = TRUE;
        g_free(name);
    }
    g_slist_free(formats);
    return writable;
}

static GdkPixbuf *flatten_on_white(GdkPixbuf *src)
{
    int w = gdk_pixbuf_get_width(src);
    int h = gdk_pixbuf_get_height(src);
    GdkPixbuf *dst = gdk_pixbuf_new(GDK_COLORSPACE_RGB, FALSE, 8, w, h);

    if (dst == NULL)
        return NULL;
    gdk_pixbuf_fill(dst, 0xffffffff);
    gdk_pixbuf_composite(src, dst, 0, 0, w, h, 0, 0, 1, 1, GDK_INTERP_NEAREST, 255);
    return dst;
}

static gboolean save_surface(cairo_surface_t *surface, gint format, const gchar *file_path, GError **error)
{
    int w = cairo_image_surface_get_width(surface);
    int h = cairo_image_surface_get_height(surface);
    GdkPixbuf *pixbuf;
    const gchar *type;
    gboolean flatten = FALSE;
    gboolean ok;

    switch (format) {
    case 0:
        type = "jpeg";
        flatten = TRUE;
        break;
    case 1:
        type = "png";
        break;
    case 2:
        type = pixbuf_can_write("webp") ? "webp" : "png";
        break;
    case 3:
        type = "bmp";
        flatten = TRUE;
        break;
    default:
        g_set_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT, "Unknown screenshot format %d", format);
        return FALSE;
    }
    if (file_path == NULL || *file_path == '\0') {
        g_set_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT, "cmd_screenshot without FilePath");
        return FALSE;
    }

    pixbuf = gdk_pixbuf_get_from_surface(surface, 0, 0, w, h);
    if (pixbuf == NULL) {
        g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "Could not read the snapshot pixels");
        return FALSE;
    }
    if (flatten && gdk_pixbuf_get_has_alpha(pixbuf)) {
        GdkPixbuf *flat = flatten_on_white(pixbuf);

        if (flat == NULL) {
            g_object_unref(pixbuf);
            g_set_error(error, G_IO_ERROR, G_IO_ERROR_FAILED, "Out of memory while flattening the snapshot");
            return FALSE;
        }
        g_object_unref(pixbuf);
        pixbuf = flat;
    }
    if (strcmp(type, "jpeg") == 0)
        ok = gdk_pixbuf_save(pixbuf, file_path, type, error, "quality", "90", NULL);
    else
        ok = gdk_pixbuf_save(pixbuf, file_path, type, error, NULL);
    g_object_unref(pixbuf);
    return ok;
}

/* Form1: cmd_screenshot */
static void handle_screenshot(App *app, gint format, const gchar *file_path)
{
    gchar *file_name = file_path != NULL ? g_path_get_basename(file_path) : NULL;
    cairo_surface_t *display;
    gboolean owned = FALSE;
    gboolean success = FALSE;
    GError *error = NULL;

    if (app->paused && app->pause_surface != NULL) {
        /* While paused the picture on screen is the pause snapshot. */
        display = app->pause_surface;
    } else {
        display = capture_view_surface(app);
        owned = TRUE;
    }
    if (display == NULL)
        g_set_error(&error, G_IO_ERROR, G_IO_ERROR_FAILED, "the web view is not drawable");
    else
        success = save_surface(display, format, file_path, &error);
    if (success)
        logv(app, "screenshot saved to %s", file_path);
    else
        lwh_ipc_send_console(LWH_CONSOLE_ERROR, "Failed to capture screenshot: %s", error->message);
    lwh_ipc_send_screenshot(file_name, success);
    if (owned && display != NULL)
        cairo_surface_destroy(display);
    g_clear_error(&error);
    g_free(file_name);
}

/* ------------------------------------------------------------------------- */
/* WebKit signals                                                             */

static void on_wploaded_sentinel_done(GObject *source, GAsyncResult *result, gpointer user_data)
{
    App *app = user_data;
    GError *error = NULL;
    JSCValue *value = webkit_web_view_evaluate_javascript_finish(WEBKIT_WEB_VIEW(source), result, &error);

    if (value != NULL)
        g_object_unref(value);
    g_clear_error(&error);
    if (app->quitting)
        return;
    lwh_ipc_send_wploaded(TRUE);
}

static void on_load_changed(WebKitWebView *view, WebKitLoadEvent event, gpointer user_data)
{
    App *app = user_data;
    GdkRGBA white = { 1.0, 1.0, 1.0, 1.0 };

    switch (event) {
    case WEBKIT_LOAD_STARTED:
        logv(app, "load started: %s", webkit_web_view_get_uri(view));
        app->load_failed = FALSE;
        app->load_cancelled = FALSE;
        break;
    case WEBKIT_LOAD_REDIRECTED:
        logv(app, "load redirected: %s", webkit_web_view_get_uri(view));
        break;
    case WEBKIT_LOAD_COMMITTED:
        logv(app, "load committed: %s", webkit_web_view_get_uri(view));
        break;
    case WEBKIT_LOAD_FINISHED:
        logv(app, "load finished: %s", webkit_web_view_get_uri(view));
        if (app->load_failed) {
            lwh_ipc_send_wploaded(FALSE);
            request_exit(app, EXIT_LOAD, "page failed to load");
            break;
        }
        if (app->load_cancelled) {
            /* Superseded by another navigation; its own FINISHED follows. */
            app->load_cancelled = FALSE;
            break;
        }
        webkit_web_view_set_background_color(view, &white);
        restore_properties(app);
        /* Scripts run in order, so this completes after every property call above. */
        webkit_web_view_evaluate_javascript(view, "void 0;", -1, NULL, NULL, NULL, on_wploaded_sentinel_done, app);
        break;
    }
}

static gboolean on_load_failed(G_GNUC_UNUSED WebKitWebView *view, G_GNUC_UNUSED WebKitLoadEvent event,
                               gchar *failing_uri, GError *error, gpointer user_data)
{
    App *app = user_data;

    if (g_error_matches(error, WEBKIT_NETWORK_ERROR, WEBKIT_NETWORK_ERROR_CANCELLED)) {
        logv(app, "load cancelled: %s", failing_uri);
        app->load_cancelled = TRUE;
        return TRUE;
    }
    lwh_ipc_send_console(LWH_CONSOLE_ERROR, "WebView navigation failed: %s (%s)", error->message, failing_uri);
    app->load_failed = TRUE;
    return TRUE;
}

static void on_web_process_terminated(G_GNUC_UNUSED WebKitWebView *view, WebKitWebProcessTerminationReason reason,
                                      G_GNUC_UNUSED gpointer user_data)
{
    const gchar *text;

    switch (reason) {
    case WEBKIT_WEB_PROCESS_CRASHED:
        text = "crashed";
        break;
    case WEBKIT_WEB_PROCESS_EXCEEDED_MEMORY_LIMIT:
        text = "exceeded memory limit";
        break;
    case WEBKIT_WEB_PROCESS_TERMINATED_BY_API:
        text = "terminated by API";
        break;
    default:
        text = "unknown reason";
        break;
    }
    lwh_ipc_send_console(LWH_CONSOLE_ERROR, "WebKit web process failed: %s", text);
}

/* Form1.CoreWebView2_NewWindowRequested: user initiated pop-ups go to the default browser. */
static GtkWidget *on_create(G_GNUC_UNUSED WebKitWebView *view, WebKitNavigationAction *action, gpointer user_data)
{
    App *app = user_data;
    WebKitURIRequest *request = webkit_navigation_action_get_request(action);
    const gchar *uri = request != NULL ? webkit_uri_request_get_uri(request) : NULL;

    if (uri != NULL && webkit_navigation_action_is_user_gesture(action)) {
        gchar *argv[] = { "xdg-open", (gchar *)uri, NULL };
        GError *error = NULL;

        logv(app, "opening pop-up in the default browser: %s", uri);
        if (!g_spawn_async(NULL, argv, NULL,
                           G_SPAWN_SEARCH_PATH | G_SPAWN_STDOUT_TO_DEV_NULL | G_SPAWN_STDERR_TO_DEV_NULL,
                           NULL, NULL, NULL, &error)) {
            lwh_ipc_send_console(LWH_CONSOLE_ERROR, "xdg-open failed: %s", error->message);
            g_error_free(error);
        }
    } else {
        logv(app, "pop-up cancelled: %s", uri != NULL ? uri : "(no uri)");
    }
    return NULL;
}

/* Form1.CoreWebView2_DownloadStarting: downloads are cancelled. */
static void on_download_started(G_GNUC_UNUSED WebKitWebContext *context, WebKitDownload *download, gpointer user_data)
{
    App *app = user_data;
    WebKitURIRequest *request = webkit_download_get_request(download);

    logv(app, "download cancelled: %s", request != NULL ? webkit_uri_request_get_uri(request) : "(no uri)");
    webkit_download_cancel(download);
}

static gboolean on_context_menu(G_GNUC_UNUSED WebKitWebView *view, G_GNUC_UNUSED WebKitContextMenu *menu,
                                G_GNUC_UNUSED GdkEvent *event, G_GNUC_UNUSED WebKitHitTestResult *hit,
                                gpointer user_data)
{
    App *app = user_data;

    return app->args.debug_port == 0;
}

static void on_script_message(G_GNUC_UNUSED WebKitUserContentManager *manager, WebKitJavascriptResult *result,
                              gpointer user_data)
{
    App *app = user_data;
    JSCValue *value = webkit_javascript_result_get_js_value(result);
    gchar *raw = jsc_value_to_string(value);
    JsonParser *parser = json_parser_new();
    const gchar *level = "log";
    const gchar *text = raw;

    if (json_parser_load_from_data(parser, raw, -1, NULL)) {
        JsonNode *root = json_parser_get_root(parser);

        if (root != NULL && JSON_NODE_HOLDS_OBJECT(root)) {
            JsonObject *obj = json_node_get_object(root);

            if (json_object_has_member(obj, "level") && !json_object_get_null_member(obj, "level"))
                level = json_object_get_string_member(obj, "level");
            if (json_object_has_member(obj, "text") && !json_object_get_null_member(obj, "text"))
                text = json_object_get_string_member(obj, "text");
        }
    }
    if (g_strcmp0(level, "log") == 0)
        lwh_ipc_send_console(LWH_CONSOLE_CONSOLE, "%s", text);
    else
        lwh_ipc_send_console(LWH_CONSOLE_CONSOLE, "%s: %s", level, text);
    (void)app;
    g_object_unref(parser);
    g_free(raw);
}

/* ------------------------------------------------------------------------- */
/* inspector (--debug)                                                        */

static gboolean on_inspector_window_delete(GtkWidget *window, G_GNUC_UNUSED GdkEvent *event,
                                           G_GNUC_UNUSED gpointer user_data)
{
    gtk_widget_hide(window);
    return TRUE;
}

/* Host the inspector in a normal toplevel instead of inside the wallpaper surface. */
static gboolean on_inspector_attach(WebKitWebInspector *inspector, gpointer user_data)
{
    App *app = user_data;
    GtkWidget *inspector_view = GTK_WIDGET(webkit_web_inspector_get_web_view(inspector));
    GtkWidget *parent;

    if (inspector_view == NULL)
        return FALSE;
    if (app->inspector_window == NULL) {
        app->inspector_window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
        gtk_window_set_title(GTK_WINDOW(app->inspector_window), "Lively Web Inspector");
        gtk_window_set_default_size(GTK_WINDOW(app->inspector_window), 1000, 700);
        g_signal_connect(app->inspector_window, "delete-event", G_CALLBACK(on_inspector_window_delete), app);
    }
    parent = gtk_widget_get_parent(inspector_view);
    if (parent != app->inspector_window) {
        if (parent != NULL)
            gtk_container_remove(GTK_CONTAINER(parent), inspector_view);
        gtk_container_add(GTK_CONTAINER(app->inspector_window), inspector_view);
    }
    gtk_widget_show_all(app->inspector_window);
    return TRUE;
}

static gboolean on_inspector_detach(G_GNUC_UNUSED WebKitWebInspector *inspector, gpointer user_data)
{
    App *app = user_data;

    if (app->inspector_window != NULL)
        gtk_widget_hide(app->inspector_window);
    return FALSE;
}

static void on_inspector_closed(G_GNUC_UNUSED WebKitWebInspector *inspector, gpointer user_data)
{
    App *app = user_data;

    if (app->inspector_window != NULL) {
        gtk_widget_destroy(app->inspector_window);
        app->inspector_window = NULL;
    }
}

/* ------------------------------------------------------------------------- */
/* window signals                                                             */

static void on_window_realize(GtkWidget *window, gpointer user_data)
{
    App *app = user_data;

    (void)window;
    lwh_ipc_send_hwnd();
    if (app->args.debug_port != 0)
        webkit_web_inspector_show(webkit_web_view_get_inspector(app->view));
}

static void on_window_destroy(G_GNUC_UNUSED GtkWidget *window, gpointer user_data)
{
    App *app = user_data;

    app->window = NULL;
    if (app->args.windowed)
        request_exit(app, EXIT_OK, "preview window closed");
    else
        request_exit(app, EXIT_NO_OUTPUT, "layer surface closed by the compositor");
}

static void on_monitor_removed(G_GNUC_UNUSED GdkDisplay *display, GdkMonitor *monitor, gpointer user_data)
{
    App *app = user_data;

    if (monitor == app->monitor)
        request_exit(app, EXIT_NO_OUTPUT, "output disappeared");
}

/* ------------------------------------------------------------------------- */
/* stdin protocol                                                             */

static const gchar *string_member(JsonObject *obj, const gchar *key)
{
    JsonNode *node = json_object_get_member(obj, key);

    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_STRING)
        return NULL;
    return json_node_get_string(node);
}

static gboolean number_member(JsonObject *obj, const gchar *key, gdouble *out)
{
    JsonNode *node = json_object_get_member(obj, key);
    GType t;

    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node))
        return FALSE;
    t = json_node_get_value_type(node);
    if (t != G_TYPE_INT64 && t != G_TYPE_DOUBLE)
        return FALSE;
    *out = json_node_get_double(node);
    return TRUE;
}

static gboolean boolean_member(JsonObject *obj, const gchar *key)
{
    JsonNode *node = json_object_get_member(obj, key);

    if (node == NULL || !JSON_NODE_HOLDS_VALUE(node) || json_node_get_value_type(node) != G_TYPE_BOOLEAN)
        return FALSE;
    return json_node_get_boolean(node);
}

static JsonNode *string_member_node(JsonObject *obj, const gchar *key)
{
    const gchar *s = string_member(obj, key);
    JsonNode *node = json_node_new(s != NULL ? JSON_NODE_VALUE : JSON_NODE_NULL);

    if (s != NULL)
        json_node_set_string(node, s);
    return node;
}

static JsonNode *member_or_null(JsonObject *obj, const gchar *key)
{
    JsonNode *node = json_object_get_member(obj, key);

    return node != NULL ? json_node_copy(node) : json_node_new(JSON_NODE_NULL);
}

static void property_message(App *app, JsonObject *obj, JsonNode *value)
{
    js_call_name_value(app, "livelyPropertyListener", string_member(obj, "Name"), value);
    json_node_unref(value);
}

static void handle_message(App *app, JsonObject *obj)
{
    gdouble number = 0.0;
    JsonNode *node;
    gint type;

    if (!number_member(obj, "Type", &number)) {
        logv(app, "ignoring message without an integer Type");
        return;
    }
    type = (gint)number;

    switch (type) {
    case LWH_CMD_RELOAD:
        webkit_web_view_reload(app->view);
        break;
    case LWH_CMD_CLOSE:
        request_exit(app, EXIT_OK, "cmd_close");
        break;
    case LWH_CMD_SCREENSHOT: {
        gdouble format = 0.0;

        number_member(obj, "Format", &format);
        handle_screenshot(app, (gint)format, string_member(obj, "FilePath"));
        break;
    }
    case LWH_CMD_SUSPEND:
        handle_suspend(app);
        break;
    case LWH_CMD_RESUME:
        handle_resume(app);
        break;
    case LWH_CMD_VOLUME: {
        gdouble volume = 0.0;

        number_member(obj, "Volume", &volume);
        webkit_web_view_set_is_muted(app->view, volume == 0.0);
        break;
    }
    case LWH_LSP_PERFCNTR:
        node = member_or_null(obj, "Info");
        js_call_node(app, "livelySystemInformation", node);
        json_node_unref(node);
        break;
    case LWH_LSP_NOWPLAYING:
        node = member_or_null(obj, "Info");
        g_free(app->last_nowplaying);
        app->last_nowplaying = json_to_string(node, FALSE);
        js_call_node(app, "livelyCurrentTrack", node);
        json_node_unref(node);
        break;
    case LWH_LP_SLIDER:
        number_member(obj, "Value", &number);
        node = json_node_new(JSON_NODE_VALUE);
        json_node_set_double(node, number);
        property_message(app, obj, node);
        break;
    case LWH_LP_TEXTBOX:
    case LWH_LP_CPICKER:
        property_message(app, obj, string_member_node(obj, "Value"));
        break;
    case LWH_LP_DROPDOWN:
        number_member(obj, "Value", &number);
        node = json_node_new(JSON_NODE_VALUE);
        json_node_set_int(node, (gint64)number);
        property_message(app, obj, node);
        break;
    case LWH_LP_FDROPDOWN: {
        const gchar *value = string_member(obj, "Value");
        gboolean exists = FALSE;

        if (value != NULL && app->page_dir != NULL) {
            gchar *full = g_build_filename(app->page_dir, value, NULL);

            exists = g_file_test(full, G_FILE_TEST_IS_REGULAR);
            g_free(full);
        }
        node = json_node_new(exists ? JSON_NODE_VALUE : JSON_NODE_NULL);
        if (exists)
            json_node_set_string(node, value);
        property_message(app, obj, node);
        break;
    }
    case LWH_LP_BUTTON:
        if (boolean_member(obj, "IsDefault")) {
            restore_properties(app);
        } else {
            node = json_node_new(JSON_NODE_VALUE);
            json_node_set_boolean(node, TRUE);
            property_message(app, obj, node);
        }
        break;
    case LWH_LP_CHECKBOX:
        node = json_node_new(JSON_NODE_VALUE);
        json_node_set_boolean(node, boolean_member(obj, "Value"));
        property_message(app, obj, node);
        break;
    case LWH_LP_DROPDOWN_SCALER:
        logv(app, "lp_dropdown_scaler has no effect on a web wallpaper");
        break;
    case LWH_LSP_AUDIO:
        if (!app->args.audio) {
            logv(app, "lsp_audio ignored (--audio not given)");
        } else if (app->paused) {
            logv(app, "lsp_audio ignored while paused");
        } else {
            node = member_or_null(obj, "Data");
            js_call_node(app, "livelyAudioListener", node);
            json_node_unref(node);
        }
        break;
    case LWH_HOST_MPV_COMMAND:
        logv(app, "host_mpv_command is only understood by lively-mpv-host");
        break;
    case LWH_MSG_HWND:
    case LWH_MSG_CONSOLE:
    case LWH_MSG_WPLOADED:
    case LWH_MSG_SCREENSHOT:
        logv(app, "host-to-core message type %d received on stdin, ignored", type);
        break;
    default:
        logv(app, "unknown message type %d ignored", type);
        break;
    }
}

static void handle_line(App *app, const gchar *line)
{
    JsonParser *parser;
    GError *error = NULL;
    JsonNode *root;

    logv(app, "stdin: %s", line);
    if (*line == '\0')
        return;
    parser = json_parser_new();
    if (!json_parser_load_from_data(parser, line, -1, &error)) {
        lwh_ipc_send_console(LWH_CONSOLE_ERROR, "Invalid message: %s", error->message);
        g_error_free(error);
        g_object_unref(parser);
        return;
    }
    root = json_parser_get_root(parser);
    if (root == NULL || !JSON_NODE_HOLDS_OBJECT(root))
        lwh_ipc_send_console(LWH_CONSOLE_ERROR, "Invalid message: not a JSON object");
    else
        handle_message(app, json_node_get_object(root));
    g_object_unref(parser);
}

static gboolean on_stdin(GIOChannel *channel, G_GNUC_UNUSED GIOCondition condition, gpointer user_data)
{
    App *app = user_data;

    for (;;) {
        gchar *line = NULL;
        gsize length = 0;
        gsize terminator = 0;
        GError *error = NULL;
        GIOStatus status = g_io_channel_read_line(channel, &line, &length, &terminator, &error);

        switch (status) {
        case G_IO_STATUS_NORMAL:
            line[terminator] = '\0';
            handle_line(app, line);
            g_free(line);
            if (app->quitting) {
                app->stdin_watch = 0;
                return FALSE;
            }
            break;
        case G_IO_STATUS_AGAIN:
            return TRUE;
        case G_IO_STATUS_EOF:
            app->stdin_watch = 0;
            request_exit(app, EXIT_OK, "stdin closed");
            return FALSE;
        case G_IO_STATUS_ERROR:
        default:
            lwh_ipc_send_console(LWH_CONSOLE_ERROR, "stdin read error: %s",
                                 error != NULL ? error->message : "unknown");
            g_clear_error(&error);
            app->stdin_watch = 0;
            request_exit(app, EXIT_OK, "stdin error");
            return FALSE;
        }
    }
}

static gboolean on_signal(gpointer user_data)
{
    App *app = user_data;

    request_exit(app, EXIT_OK, "terminated by signal");
    return G_SOURCE_REMOVE;
}

/* ------------------------------------------------------------------------- */
/* setup                                                                      */

/* WebKitGTK derives prefers-color-scheme from GtkSettings (prefer-dark-theme, a
 * "-dark" theme name or GTK_THEME=...:dark). */
static void apply_color_scheme(App *app)
{
    GtkSettings *settings = gtk_settings_get_default();
    gchar *theme = NULL;

    switch (app->args.scheme) {
    case LWH_SCHEME_DARK:
        g_object_set(settings, "gtk-application-prefer-dark-theme", TRUE, NULL);
        break;
    case LWH_SCHEME_LIGHT:
        g_object_set(settings, "gtk-application-prefer-dark-theme", FALSE, NULL);
        g_object_get(settings, "gtk-theme-name", &theme, NULL);
        if (theme != NULL && (g_str_has_suffix(theme, "-dark") || g_str_has_suffix(theme, "-Dark") ||
                              g_str_has_suffix(theme, ":dark")))
            g_object_set(settings, "gtk-theme-name", "Adwaita", NULL);
        g_free(theme);
        break;
    case LWH_SCHEME_AUTO:
        break;
    }
}

/* GDK decides per paint whether a toplevel is painted through GL or through an
 * SHM buffer, and WebKit's first accelerated frame creates the GL paint context
 * in the middle of an SHM paint. Mesa registers explicit sync on the surface as
 * soon as the EGL surface exists, so that SHM commit is a protocol error on
 * compositors that enforce it (KWin). GDK_GL=always creates the GL paint context
 * together with the window, so every paint goes through EGL from the start. */
static void force_gdk_gl_paint(App *app)
{
    const gchar *current = g_getenv("GDK_GL");
    gchar *value;

    if (current == NULL || *current == '\0')
        value = g_strdup("always");
    else if (strstr(current, "always") != NULL)
        value = g_strdup(current);
    else
        value = g_strconcat(current, ",always", NULL);
    logv(app, "GDK_GL=%s", value);
    g_setenv("GDK_GL", value, TRUE);
    g_free(value);
}

static WebKitWebView *create_web_view(App *app)
{
    WebKitWebsiteDataManager *data_manager = NULL;
    WebKitSettings *settings;
    WebKitUserContentManager *ucm;
    WebKitUserScript *script;
    WebKitWebView *view;
    GdkRGBA transparent = { 0.0, 0.0, 0.0, 0.0 };

    if (app->args.user_data != NULL) {
        logv(app, "website data directory: %s", app->args.user_data);
        data_manager = webkit_website_data_manager_new("base-data-directory", app->args.user_data,
                                                       "base-cache-directory", app->args.user_data, NULL);
        app->context = webkit_web_context_new_with_website_data_manager(data_manager);
        g_object_unref(data_manager);
    } else {
        app->context = webkit_web_context_new();
    }
    g_signal_connect(app->context, "download-started", G_CALLBACK(on_download_started), app);

    settings = webkit_settings_new();
    webkit_settings_set_media_playback_requires_user_gesture(settings, FALSE);
    webkit_settings_set_enable_webgl(settings, TRUE);
    webkit_settings_set_hardware_acceleration_policy(settings, WEBKIT_HARDWARE_ACCELERATION_POLICY_ALWAYS);
    webkit_settings_set_enable_developer_extras(settings, app->args.debug_port != 0);
    webkit_settings_set_allow_file_access_from_file_urls(settings, TRUE);
    webkit_settings_set_allow_universal_access_from_file_urls(settings, TRUE);
    webkit_settings_set_enable_write_console_messages_to_stdout(settings, FALSE);
    webkit_settings_set_enable_media_stream(settings, FALSE);
    /* window.open always reaches the create handler; it decides (Form1 semantics). */
    webkit_settings_set_javascript_can_open_windows_automatically(settings, TRUE);

    ucm = webkit_user_content_manager_new();
    script = webkit_user_script_new(console_bridge_script, WEBKIT_USER_CONTENT_INJECT_ALL_FRAMES,
                                    WEBKIT_USER_SCRIPT_INJECT_AT_DOCUMENT_START, NULL, NULL);
    webkit_user_content_manager_add_script(ucm, script);
    webkit_user_script_unref(script);
    g_signal_connect(ucm, "script-message-received::lively", G_CALLBACK(on_script_message), app);
    webkit_user_content_manager_register_script_message_handler(ucm, "lively");

    view = WEBKIT_WEB_VIEW(g_object_new(WEBKIT_TYPE_WEB_VIEW,
                                        "web-context", app->context,
                                        "settings", settings,
                                        "user-content-manager", ucm,
                                        NULL));
    g_object_unref(settings);
    g_object_unref(ucm);
    if (view == NULL)
        return NULL;

    webkit_web_view_set_background_color(view, &transparent);
    webkit_web_view_set_is_muted(view, app->args.volume == 0);
    if (app->args.scale_given)
        webkit_web_view_set_zoom_level(view, app->args.scale);

    g_signal_connect(view, "load-changed", G_CALLBACK(on_load_changed), app);
    g_signal_connect(view, "load-failed", G_CALLBACK(on_load_failed), app);
    g_signal_connect(view, "web-process-terminated", G_CALLBACK(on_web_process_terminated), app);
    g_signal_connect(view, "create", G_CALLBACK(on_create), app);
    g_signal_connect(view, "context-menu", G_CALLBACK(on_context_menu), app);
    if (app->args.debug_port != 0) {
        WebKitWebInspector *inspector = webkit_web_view_get_inspector(view);

        g_signal_connect(inspector, "attach", G_CALLBACK(on_inspector_attach), app);
        g_signal_connect(inspector, "detach", G_CALLBACK(on_inspector_detach), app);
        g_signal_connect(inspector, "closed", G_CALLBACK(on_inspector_closed), app);
    }
    return view;
}

static void create_window(App *app)
{
    GtkWindow *window;
    GdkVisual *visual;
    GtkWidget *content;

    app->window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    window = GTK_WINDOW(app->window);

    if (app->args.windowed) {
        gtk_window_set_title(window, app->args.title);
        gtk_window_set_decorated(window, TRUE);
        gtk_window_set_resizable(window, TRUE);
        gtk_window_set_default_size(window, app->args.window_w, app->args.window_h);
        gtk_window_set_position(window, GTK_WIN_POS_CENTER);
    } else {
        gtk_window_set_title(window, "lively-web-host");
        gtk_window_set_decorated(window, FALSE);

        gtk_layer_init_for_window(window);
        gtk_layer_set_layer(window, app->args.layer_bottom ? GTK_LAYER_SHELL_LAYER_BOTTOM : GTK_LAYER_SHELL_LAYER_BACKGROUND);
        gtk_layer_set_namespace(window, app->args.ns);
        gtk_layer_set_anchor(window, GTK_LAYER_SHELL_EDGE_LEFT, TRUE);
        gtk_layer_set_anchor(window, GTK_LAYER_SHELL_EDGE_RIGHT, TRUE);
        gtk_layer_set_anchor(window, GTK_LAYER_SHELL_EDGE_TOP, TRUE);
        gtk_layer_set_anchor(window, GTK_LAYER_SHELL_EDGE_BOTTOM, TRUE);
        gtk_layer_set_exclusive_zone(window, -1);
        gtk_layer_set_keyboard_mode(window, GTK_LAYER_SHELL_KEYBOARD_MODE_NONE);
        gtk_layer_set_monitor(window, app->monitor);
    }

    /* Transparent until the first load finishes (WebView2 does the same). */
    visual = gdk_screen_get_rgba_visual(gtk_widget_get_screen(app->window));
    if (visual != NULL)
        gtk_widget_set_visual(app->window, visual);
    gtk_widget_set_app_paintable(app->window, TRUE);

    if (!app->args.interactive) {
        cairo_region_t *empty = cairo_region_create();

        gtk_widget_input_shape_combine_region(app->window, empty);
        cairo_region_destroy(empty);
    }

    app->overlay = gtk_overlay_new();
    gtk_container_add(GTK_CONTAINER(app->window), app->overlay);

    if (app->args.span) {
        /* GtkLayout is the scrollable GtkFixed: children sit at fixed offsets
         * without growing the container's size request past the output. */
        content = gtk_layout_new(NULL, NULL);
        gtk_widget_set_size_request(GTK_WIDGET(app->view), app->args.span_vw, app->args.span_vh);
        gtk_layout_put(GTK_LAYOUT(content), GTK_WIDGET(app->view), -app->args.span_x, -app->args.span_y);
        gtk_container_add(GTK_CONTAINER(app->overlay), content);
    } else {
        gtk_container_add(GTK_CONTAINER(app->overlay), GTK_WIDGET(app->view));
    }

    app->image = gtk_image_new();
    gtk_widget_set_halign(app->image, GTK_ALIGN_FILL);
    gtk_widget_set_valign(app->image, GTK_ALIGN_FILL);
    gtk_widget_set_no_show_all(app->image, TRUE);
    gtk_overlay_add_overlay(GTK_OVERLAY(app->overlay), app->image);
    gtk_overlay_set_overlay_pass_through(GTK_OVERLAY(app->overlay), app->image, TRUE);

    g_signal_connect_after(app->window, "realize", G_CALLBACK(on_window_realize), app);
    g_signal_connect(app->window, "destroy", G_CALLBACK(on_window_destroy), app);
    if (!app->args.windowed)
        g_signal_connect(app->display, "monitor-removed", G_CALLBACK(on_monitor_removed), app);
}

/* Form1.InitializeWebView2Async: choose what to load. */
static void start_load(App *app)
{
    if (app->args.type == LWH_PAGE_LOCAL) {
        gchar *uri = g_filename_to_uri(app->page_path, NULL, NULL);

        lwh_ipc_send_console(LWH_CONSOLE_LOG, "Opening local project: %s", app->page_path);
        logv(app, "loading %s", uri);
        webkit_web_view_load_uri(app->view, uri);
        g_free(uri);
        return;
    }

    {
        gchar *html = NULL;
        gchar *id = NULL;

        if (lwh_stream_try_parse_shadertoy(app->args.target, &html)) {
            gchar *origin = lwh_stream_url_origin(app->args.target);

            lwh_ipc_send_console(LWH_CONSOLE_LOG, "Opening shadertoy shader: %s", html);
            logv(app, "shadertoy wrapper base uri: %s", origin != NULL ? origin : "(none)");
            webkit_web_view_load_html(app->view, html, origin);
            g_free(origin);
            g_free(html);
        } else if (lwh_stream_try_parse_youtube_id(app->args.target, &id)) {
            gchar *embed = lwh_stream_youtube_embed_url(id);

            app->video_stream = TRUE;
            lwh_ipc_send_console(LWH_CONSOLE_LOG, "Opening yt stream: %s", id);
            logv(app, "loading %s", embed);
            webkit_web_view_load_uri(app->view, embed);
            g_free(embed);
            g_free(id);
        } else {
            lwh_ipc_send_console(LWH_CONSOLE_LOG, "Opening address: %s", app->args.target);
            webkit_web_view_load_uri(app->view, app->args.target);
        }
    }
}

static gboolean resolve_local_page(App *app)
{
    gchar *path = NULL;

    if (g_str_has_prefix(app->args.target, "file://"))
        path = g_filename_from_uri(app->args.target, NULL, NULL);
    if (path == NULL)
        path = g_canonicalize_filename(app->args.target, NULL);
    if (!g_file_test(path, G_FILE_TEST_IS_REGULAR)) {
        lwh_ipc_send_console(LWH_CONSOLE_ERROR, "WebView navigation failed: file not found: %s", path);
        fprintf(stderr, "lively-web-host: file not found: %s\n", path);
        g_free(path);
        return FALSE;
    }
    app->page_path = path;
    app->page_dir = g_path_get_dirname(path);
    return TRUE;
}

static void app_cleanup(App *app)
{
    if (app->stdin_watch != 0)
        g_source_remove(app->stdin_watch);
    if (app->stdin_channel != NULL)
        g_io_channel_unref(app->stdin_channel);
    if (app->display != NULL)
        g_signal_handlers_disconnect_by_data(app->display, app);
    if (app->inspector_window != NULL)
        gtk_widget_destroy(app->inspector_window);
    if (app->window != NULL) {
        g_signal_handlers_disconnect_by_func(app->window, on_window_destroy, app);
        gtk_widget_destroy(app->window);
    }
    if (app->context != NULL)
        g_object_unref(app->context);
    if (app->pause_surface != NULL)
        cairo_surface_destroy(app->pause_surface);
    g_free(app->last_nowplaying);
    g_free(app->page_path);
    g_free(app->page_dir);
    lwh_args_clear(&app->args);
}

int main(int argc, char **argv)
{
    App app;
    GError *error = NULL;
    gchar *known_outputs = NULL;
    LwhOutputStatus status;

    memset(&app, 0, sizeof(app));
    lwh_ipc_init();

    if (!lwh_args_parse(&app.args, argc, argv, &error)) {
        fprintf(stderr, "lively-web-host: %s\n", error->message);
        g_error_free(error);
        return EXIT_BAD_ARGS;
    }
    if (app.args.type == LWH_PAGE_LOCAL && !resolve_local_page(&app)) {
        lwh_ipc_send_wploaded(FALSE);
        app_cleanup(&app);
        return EXIT_LOAD;
    }
    if (app.args.debug_port != 0) {
        gchar *address = g_strdup_printf("127.0.0.1:%d", app.args.debug_port);

        g_setenv("WEBKIT_INSPECTOR_HTTP_SERVER", address, TRUE);
        logv(&app, "remote inspector on http://%s/", address);
        g_free(address);
    }
    if (app.args.scheme == LWH_SCHEME_LIGHT) {
        const gchar *env_theme = g_getenv("GTK_THEME");

        if (env_theme != NULL && (g_str_has_suffix(env_theme, ":dark") || g_str_has_suffix(env_theme, "-dark")))
            g_unsetenv("GTK_THEME");
    }

    force_gdk_gl_paint(&app);
    gdk_set_allowed_backends("wayland");
    if (!gtk_init_check(&argc, &argv)) {
        fprintf(stderr, "lively-web-host: cannot connect to a Wayland display\n");
        app_cleanup(&app);
        return EXIT_NO_PROTOCOL;
    }
    app.display = gdk_display_get_default();
    if (!GDK_IS_WAYLAND_DISPLAY(app.display)) {
        fprintf(stderr, "lively-web-host: the default display is not a Wayland display\n");
        app_cleanup(&app);
        return EXIT_NO_PROTOCOL;
    }
    if (!app.args.windowed && !gtk_layer_is_supported()) {
        fprintf(stderr, "lively-web-host: compositor does not provide zwlr_layer_shell_v1\n");
        app_cleanup(&app);
        return EXIT_NO_PROTOCOL;
    }

    status = app.args.windowed ? LWH_OUTPUT_FOUND
                               : lwh_output_find_monitor(app.display, app.args.output, &app.monitor, &known_outputs);
    switch (status) {
    case LWH_OUTPUT_FOUND:
        if (app.args.windowed)
            logv(&app, "windowed preview %dx%d \"%s\"", app.args.window_w, app.args.window_h, app.args.title);
        else
            logv(&app, "output %s found (outputs: %s)", app.args.output, known_outputs);
        break;
    case LWH_OUTPUT_NO_XDG_OUTPUT:
        fprintf(stderr, "lively-web-host: compositor does not provide zxdg_output_manager_v1\n");
        g_free(known_outputs);
        app_cleanup(&app);
        return EXIT_NO_PROTOCOL;
    case LWH_OUTPUT_ERROR:
        fprintf(stderr, "lively-web-host: Wayland error while enumerating outputs\n");
        g_free(known_outputs);
        app_cleanup(&app);
        return EXIT_NO_PROTOCOL;
    case LWH_OUTPUT_NOT_FOUND:
    default:
        fprintf(stderr, "lively-web-host: output \"%s\" not found (available: %s)\n", app.args.output,
                known_outputs != NULL && *known_outputs != '\0' ? known_outputs : "none");
        g_free(known_outputs);
        app_cleanup(&app);
        return EXIT_NO_OUTPUT;
    }
    g_free(known_outputs);

    apply_color_scheme(&app);
    app.view = create_web_view(&app);
    if (app.view == NULL) {
        fprintf(stderr, "lively-web-host: WebKitWebView could not be created\n");
        app_cleanup(&app);
        return EXIT_RENDERER;
    }
    create_window(&app);

    app.stdin_channel = g_io_channel_unix_new(STDIN_FILENO);
    g_io_channel_set_encoding(app.stdin_channel, NULL, NULL);
    g_io_channel_set_line_term(app.stdin_channel, "\n", 1);
    g_io_channel_set_flags(app.stdin_channel, G_IO_FLAG_NONBLOCK, NULL);
    app.stdin_watch = g_io_add_watch(app.stdin_channel, G_IO_IN | G_IO_HUP | G_IO_ERR, on_stdin, &app);
    g_unix_signal_add(SIGTERM, on_signal, &app);
    g_unix_signal_add(SIGINT, on_signal, &app);

    gtk_widget_show_all(app.window);
    start_load(&app);

    gtk_main();

    {
        int code = app.exit_code;

        app_cleanup(&app);
        return code;
    }
}
