/* lively-mpv-host: libmpv + wlr-layer-shell wallpaper renderer (PROTOCOL.md sections 1-4). */
#ifndef LIVELY_MPV_HOST_APP_H
#define LIVELY_MPV_HOST_APP_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <time.h>

#include <EGL/egl.h>
#include <wayland-client.h>
#include <wayland-egl.h>
#include <mpv/client.h>
#include <mpv/render_gl.h>

#include "cJSON.h"
#include "wlr-layer-shell-unstable-v1-client-protocol.h"
#include "xdg-output-unstable-v1-client-protocol.h"
#include "xdg-shell-client-protocol.h"

enum host_exit_code {
    HOST_EXIT_OK = 0,
    HOST_EXIT_BAD_ARGS = 2,
    HOST_EXIT_NO_PROTOCOL = 3,
    HOST_EXIT_NO_OUTPUT = 4,
    HOST_EXIT_RENDERER = 5,
    HOST_EXIT_MEDIA = 6,
};

/* Values match Lively's WallpaperScaler enum and lp_dropdown_scaler. */
enum scaler {
    SCALER_NONE = 0,
    SCALER_FILL = 1,
    SCALER_UNIFORM = 2,
    SCALER_UNIFORM_FILL = 3,
};

/* Values match cmd_screenshot.Format. */
enum shot_format {
    SHOT_JPEG = 0,
    SHOT_PNG = 1,
    SHOT_WEBP = 2,
    SHOT_BMP = 3,
};

/* MessageType enum order from Lively.Models/Message/MessageType.cs plus the Linux additions. */
enum msg_type {
    MSG_HWND = 0,
    MSG_CONSOLE = 1,
    MSG_WPLOADED = 2,
    MSG_SCREENSHOT = 3,
    CMD_RELOAD = 4,
    CMD_CLOSE = 5,
    CMD_SCREENSHOT = 6,
    CMD_SUSPEND = 7,
    CMD_RESUME = 8,
    CMD_VOLUME = 9,
    LSP_PERFCNTR = 10,
    LSP_NOWPLAYING = 11,
    LP_SLIDER = 12,
    LP_TEXTBOX = 13,
    LP_DROPDOWN = 14,
    LP_FDROPDOWN = 15,
    LP_BUTTON = 16,
    LP_CPICKER = 17,
    LP_CHECKBOX = 18,
    LP_DROPDOWN_SCALER = 19,
    LSP_AUDIO = 20,
    HOST_MPV_COMMAND = 100,
};

struct output {
    struct output *next;
    struct wl_output *wl_output;
    struct zxdg_output_v1 *xdg_output;
    uint32_t global_name;
    char *name;      /* wl_output.name (version >= 4) */
    char *xdg_name;  /* zxdg_output_v1.name */
    int32_t scale;   /* wl_output.scale, integer buffer scale */
    int32_t logical_x, logical_y, logical_w, logical_h;
    int32_t pixel_w, pixel_h;
    bool entered;    /* windowed mode: the toplevel currently overlaps this output */
};

struct span {
    bool enabled;
    double x, y, w, h;   /* this output's rectangle in the virtual screen */
    double vw, vh;       /* virtual screen size */
};

struct shot_request {
    struct shot_request *next;
    char *path;
    enum shot_format format;
    uint64_t token;          /* reply_userdata of the mpv command */
    bool started;            /* command handed to mpv */
    bool command_done;       /* mpv reported completion; now waiting for the file */
    struct timespec deadline;
};

/* An mpv_set_property_async request whose reply has not arrived yet (for error reporting). */
struct pending_set {
    struct pending_set *next;
    uint64_t token;
    char *desc;
};

struct app {
    /* command line */
    const char *output_name;
    const char *layer_namespace;
    const char *media;
    const char *property_path;
    const char *ytdl_format;
    const char *config_dir;
    const char *hwdec;
    enum zwlr_layer_shell_v1_layer layer;
    struct span span;
    bool interactive;
    bool verbose;
    bool image;
    bool has_speed;
    bool windowed;                     /* --windowed: xdg_toplevel instead of a layer surface */
    uint32_t window_w, window_h;       /* requested initial toplevel size */
    const char *title;
    double speed;
    int volume;
    enum scaler scaler;

    /* wayland */
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    uint32_t compositor_version;
    struct zwlr_layer_shell_v1 *layer_shell;
    uint32_t layer_shell_version;
    struct zxdg_output_manager_v1 *xdg_output_manager;
    uint32_t xdg_output_manager_version;
    uint32_t wl_output_version;
    struct xdg_wm_base *wm_base;
    uint32_t wm_base_version;
    struct output *outputs;
    struct output *output;             /* the selected output (layer mode) */
    struct wl_surface *surface;
    struct zwlr_layer_surface_v1 *layer_surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *xdg_toplevel;
    uint32_t pending_w, pending_h;     /* size from the last xdg_toplevel.configure, 0 = our choice */
    struct wl_callback *frame_callback;
    struct wl_egl_window *egl_window;
    uint32_t surface_w, surface_h;     /* logical size from the last configure */
    int32_t buffer_scale;
    bool configured;
    bool redraw_needed;
    bool hwnd_sent;

    /* EGL */
    EGLDisplay egl_display;
    EGLConfig egl_config;
    EGLContext egl_context;
    EGLSurface egl_surface;

    /* mpv */
    mpv_handle *mpv;
    mpv_render_context *render;
    int wakeup_pipe[2];
    bool file_loaded;
    bool is_gif;
    int64_t video_w, video_h;          /* video-out-params/w,h (pixel size of the rendered video) */
    int64_t disp_w, disp_h;            /* display size after aspect correction and rotation */
    struct shot_request *shots;        /* FIFO of screenshot requests */
    struct pending_set *pending_sets;  /* property sets awaiting their reply */
    uint64_t next_token;

    /* stdin */
    char *inbuf;
    size_t inlen, incap;
    bool stdin_eof;

    /* lifecycle */
    int signal_fd;
    bool running;
    int exit_code;
};

/* main.c */
void app_log(const struct app *a, const char *fmt, ...);
void app_error(const struct app *a, const char *fmt, ...);
void app_quit(struct app *a, int exit_code);

/* wl.c */
int wl_init(struct app *a);
int wl_create_surface(struct app *a);
int egl_init(struct app *a);
void wl_render(struct app *a);
void wl_request_redraw(struct app *a);
void wl_shutdown(struct app *a);
uint32_t wl_buffer_width(const struct app *a);
uint32_t wl_buffer_height(const struct app *a);

/* player.c */
int player_init(struct app *a);
void player_handle_events(struct app *a);
void player_handle_update(struct app *a);
void player_surface_resized(struct app *a);
void player_set_scaler(struct app *a, enum scaler scaler);
int player_set_slider(struct app *a, const char *name, double value, double step);
int player_set_checkbox(struct app *a, const char *name, bool value);
int player_set_pause(struct app *a, bool paused);
int player_set_volume(struct app *a, int volume);
int player_reload(struct app *a);
int player_apply_properties(struct app *a);
int player_screenshot(struct app *a, const char *path, enum shot_format format);
int player_run_command(struct app *a, const cJSON *command);
bool player_screenshot_waiting(const struct app *a);
void player_poll_screenshots(struct app *a);
void player_shutdown(struct app *a);

/* ipc.c */
void ipc_send_hwnd(struct app *a);
void ipc_send_console(struct app *a, const char *message, int category);
void ipc_send_wploaded(struct app *a, bool success);
void ipc_send_screenshot(struct app *a, const char *path, bool success);
int ipc_read_stdin(struct app *a);
void ipc_handle_line(struct app *a, const char *line);

#endif
