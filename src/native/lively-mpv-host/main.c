/* lively-mpv-host entry point: argument parsing, signal handling and the poll() loop. */
#include "app.h"

#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/signalfd.h>
#include <unistd.h>

void app_log(const struct app *a, const char *fmt, ...)
{
    if (!a->verbose)
        return;
    va_list ap;
    va_start(ap, fmt);
    fputs("lively-mpv-host: ", stderr);
    vfprintf(stderr, fmt, ap);
    fputc('\n', stderr);
    va_end(ap);
}

void app_error(const struct app *a, const char *fmt, ...)
{
    (void)a;
    va_list ap;
    va_start(ap, fmt);
    fputs("lively-mpv-host: error: ", stderr);
    vfprintf(stderr, fmt, ap);
    fputc('\n', stderr);
    va_end(ap);
}

void app_quit(struct app *a, int exit_code)
{
    if (!a->running)
        return;
    a->running = false;
    a->exit_code = exit_code;
}

static void usage(FILE *out)
{
    fputs("usage: lively-mpv-host --output NAME [options] <file-or-url>\n"
          "  --output NAME          Wayland output name (required)\n"
          "  --namespace NAME       layer-shell namespace (default lively-wallpaper)\n"
          "  --layer LAYER          background|bottom (default background)\n"
          "  --span X,Y,W,H,VW,VH   this output's logical rect inside a VWxVH virtual screen\n"
          "  --interactive          full input region (pointer events reach the wallpaper)\n"
          "  --property PATH        LivelyProperties.json copy to apply after load\n"
          "  --volume N             initial volume 0-100 (default 0)\n"
          "  --verbose              log stdin lines and internal events to stderr\n"
          "  --hwdec MODE           mpv hwdec value, auto-safe|no (default auto-safe)\n"
          "  --image                still image: image-display-duration=inf, loop-file=no\n"
          "  --scaler MODE          none|fill|uniform|uniformFill (default uniform)\n"
          "  --ytdl-format FMT      enable ytdl with this format string\n"
          "  --config-dir DIR       mpv config directory (default: no config files)\n"
          "  --speed F              playback speed\n"
          "  --windowed WxH         render into a normal window of this size instead of a layer surface\n"
          "                         (--output and --span are ignored)\n"
          "  --title TEXT           window title in windowed mode (default \"Lively Wallpaper\")\n"
          "  --help                 this text\n",
          out);
}

static bool parse_span(const char *text, struct span *span)
{
    double v[6];
    char trailing = 0;
    int n = sscanf(text, "%lf,%lf,%lf,%lf,%lf,%lf%c", &v[0], &v[1], &v[2], &v[3], &v[4], &v[5], &trailing);
    if (n != 6)
        return false;
    if (v[2] <= 0 || v[3] <= 0 || v[4] <= 0 || v[5] <= 0)
        return false;
    span->enabled = true;
    span->x = v[0];
    span->y = v[1];
    span->w = v[2];
    span->h = v[3];
    span->vw = v[4];
    span->vh = v[5];
    return true;
}

static bool parse_scaler(const char *text, enum scaler *scaler)
{
    if (strcasecmp(text, "none") == 0)
        *scaler = SCALER_NONE;
    else if (strcasecmp(text, "fill") == 0)
        *scaler = SCALER_FILL;
    else if (strcasecmp(text, "uniform") == 0)
        *scaler = SCALER_UNIFORM;
    else if (strcasecmp(text, "uniformFill") == 0)
        *scaler = SCALER_UNIFORM_FILL;
    else
        return false;
    return true;
}

static bool parse_window_size(const char *text, uint32_t *w, uint32_t *h)
{
    unsigned width = 0, height = 0;
    char trailing = 0;
    if (sscanf(text, "%ux%u%c", &width, &height, &trailing) != 2 || width == 0 || height == 0)
        return false;
    *w = width;
    *h = height;
    return true;
}

static bool parse_int(const char *text, int lo, int hi, int *out)
{
    char *end = NULL;
    errno = 0;
    long v = strtol(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' || v < lo || v > hi)
        return false;
    *out = (int)v;
    return true;
}

static bool parse_double(const char *text, double *out)
{
    char *end = NULL;
    errno = 0;
    double v = strtod(text, &end);
    if (errno != 0 || end == text || *end != '\0')
        return false;
    *out = v;
    return true;
}

/* Returns 0 on success, 1 for --help, -1 for bad arguments (message already printed). */
static int parse_args(struct app *a, int argc, char **argv)
{
    for (int i = 1; i < argc; i++) {
        const char *arg = argv[i];
        if (strncmp(arg, "--", 2) != 0 || strcmp(arg, "--") == 0) {
            if (strcmp(arg, "--") == 0) {
                if (i + 1 >= argc || a->media) {
                    app_error(a, "expected exactly one file or URL");
                    return -1;
                }
                a->media = argv[++i];
                continue;
            }
            if (a->media) {
                app_error(a, "unexpected argument: %s", arg);
                return -1;
            }
            a->media = arg;
            continue;
        }
        char name[64];
        const char *inline_value = strchr(arg, '=');
        size_t name_len = inline_value ? (size_t)(inline_value - arg) : strlen(arg);
        if (name_len >= sizeof name) {
            app_error(a, "unknown option: %s", arg);
            return -1;
        }
        memcpy(name, arg, name_len);
        name[name_len] = '\0';
        if (inline_value)
            inline_value++;

        bool is_flag = strcmp(name, "--interactive") == 0 || strcmp(name, "--verbose") == 0 ||
                       strcmp(name, "--image") == 0 || strcmp(name, "--help") == 0;
        if (is_flag) {
            if (inline_value) {
                app_error(a, "%s does not take a value", name);
                return -1;
            }
            if (strcmp(name, "--interactive") == 0)
                a->interactive = true;
            else if (strcmp(name, "--verbose") == 0)
                a->verbose = true;
            else if (strcmp(name, "--image") == 0)
                a->image = true;
            else
                return 1;
            continue;
        }

        const char *value = inline_value;
        if (!value) {
            if (i + 1 >= argc) {
                app_error(a, "%s needs a value", name);
                return -1;
            }
            value = argv[++i];
        }
        if (strcmp(name, "--output") == 0) {
            a->output_name = value;
        } else if (strcmp(name, "--namespace") == 0) {
            a->layer_namespace = value;
        } else if (strcmp(name, "--layer") == 0) {
            if (strcmp(value, "background") == 0)
                a->layer = ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND;
            else if (strcmp(value, "bottom") == 0)
                a->layer = ZWLR_LAYER_SHELL_V1_LAYER_BOTTOM;
            else {
                app_error(a, "--layer must be background or bottom, got %s", value);
                return -1;
            }
        } else if (strcmp(name, "--span") == 0) {
            if (!parse_span(value, &a->span)) {
                app_error(a, "--span must be X,Y,W,H,VW,VH with positive sizes, got %s", value);
                return -1;
            }
        } else if (strcmp(name, "--property") == 0) {
            a->property_path = value;
        } else if (strcmp(name, "--volume") == 0) {
            if (!parse_int(value, 0, 100, &a->volume)) {
                app_error(a, "--volume must be 0-100, got %s", value);
                return -1;
            }
        } else if (strcmp(name, "--hwdec") == 0) {
            if (*value == '\0') {
                app_error(a, "--hwdec needs a mode such as auto-safe or no");
                return -1;
            }
            a->hwdec = value;
        } else if (strcmp(name, "--scaler") == 0) {
            if (!parse_scaler(value, &a->scaler)) {
                app_error(a, "--scaler must be none, fill, uniform or uniformFill, got %s", value);
                return -1;
            }
        } else if (strcmp(name, "--ytdl-format") == 0) {
            a->ytdl_format = value;
        } else if (strcmp(name, "--config-dir") == 0) {
            a->config_dir = value;
        } else if (strcmp(name, "--speed") == 0) {
            if (!parse_double(value, &a->speed) || a->speed <= 0) {
                app_error(a, "--speed must be a positive number, got %s", value);
                return -1;
            }
            a->has_speed = true;
        } else if (strcmp(name, "--windowed") == 0) {
            if (!parse_window_size(value, &a->window_w, &a->window_h)) {
                app_error(a, "--windowed must be WIDTHxHEIGHT with positive sizes, got %s", value);
                return -1;
            }
            a->windowed = true;
        } else if (strcmp(name, "--title") == 0) {
            a->title = value;
        } else {
            app_error(a, "unknown option: %s", name);
            return -1;
        }
    }
    if (!a->windowed && (!a->output_name || *a->output_name == '\0')) {
        app_error(a, "--output is required");
        return -1;
    }
    if (!a->media || *a->media == '\0') {
        app_error(a, "a file or URL is required");
        return -1;
    }
    return 0;
}

static int setup_signals(struct app *a)
{
    struct sigaction ignore_pipe;
    memset(&ignore_pipe, 0, sizeof ignore_pipe);
    ignore_pipe.sa_handler = SIG_IGN;
    if (sigaction(SIGPIPE, &ignore_pipe, NULL) < 0) {
        app_error(a, "sigaction(SIGPIPE): %s", strerror(errno));
        return -1;
    }
    sigset_t mask;
    sigemptyset(&mask);
    sigaddset(&mask, SIGINT);
    sigaddset(&mask, SIGTERM);
    sigaddset(&mask, SIGHUP);
    if (sigprocmask(SIG_BLOCK, &mask, NULL) < 0) {
        app_error(a, "sigprocmask: %s", strerror(errno));
        return -1;
    }
    a->signal_fd = signalfd(-1, &mask, SFD_NONBLOCK | SFD_CLOEXEC);
    if (a->signal_fd < 0) {
        app_error(a, "signalfd: %s", strerror(errno));
        return -1;
    }
    return 0;
}

static void drain_wakeup_pipe(struct app *a)
{
    char buf[64];
    for (;;) {
        ssize_t n = read(a->wakeup_pipe[0], buf, sizeof buf);
        if (n > 0)
            continue;
        if (n < 0 && errno == EINTR)
            continue;
        if (n < 0 && errno != EAGAIN)
            app_error(a, "wakeup pipe read failed: %s", strerror(errno));
        return;
    }
}

static void wayland_failure(struct app *a, const char *what)
{
    int err = wl_display_get_error(a->display);
    if (err == EPROTO) {
        uint32_t id = 0, code;
        const struct wl_interface *iface = NULL;
        code = wl_display_get_protocol_error(a->display, &iface, &id);
        app_error(a, "%s: protocol error %u on %s@%u", what, code, iface ? iface->name : "unknown", id);
    } else {
        app_error(a, "%s: %s", what, strerror(err ? err : errno));
    }
    app_quit(a, HOST_EXIT_NO_PROTOCOL);
}

static void run_loop(struct app *a)
{
    const int wl_fd = wl_display_get_fd(a->display);
    /* Events queued before the wakeup callback was installed. */
    player_handle_events(a);
    player_handle_update(a);

    while (a->running) {
        bool prepared = false;
        for (;;) {
            if (wl_display_prepare_read(a->display) == 0) {
                prepared = true;
                break;
            }
            if (wl_display_dispatch_pending(a->display) < 0) {
                wayland_failure(a, "wl_display_dispatch_pending");
                break;
            }
            if (!a->running)
                break;
        }
        if (!a->running) {
            if (prepared)
                wl_display_cancel_read(a->display);
            break;
        }
        if (wl_display_flush(a->display) < 0 && errno != EAGAIN) {
            wl_display_cancel_read(a->display);
            wayland_failure(a, "wl_display_flush");
            break;
        }

        struct pollfd fds[4];
        int nfds = 0;
        fds[nfds++] = (struct pollfd){ .fd = wl_fd, .events = POLLIN };
        fds[nfds++] = (struct pollfd){ .fd = a->wakeup_pipe[0], .events = POLLIN };
        fds[nfds++] = (struct pollfd){ .fd = a->signal_fd, .events = POLLIN };
        int stdin_index = -1;
        if (!a->stdin_eof) {
            stdin_index = nfds;
            fds[nfds++] = (struct pollfd){ .fd = STDIN_FILENO, .events = POLLIN };
        }
        int timeout = player_screenshot_waiting(a) ? 20 : -1;
        int ready = poll(fds, (nfds_t)nfds, timeout);
        if (ready < 0) {
            wl_display_cancel_read(a->display);
            if (errno == EINTR)
                continue;
            app_error(a, "poll failed: %s", strerror(errno));
            app_quit(a, HOST_EXIT_NO_PROTOCOL);
            break;
        }

        if (fds[0].revents & POLLIN) {
            if (wl_display_read_events(a->display) < 0) {
                wayland_failure(a, "wl_display_read_events");
                break;
            }
        } else {
            wl_display_cancel_read(a->display);
        }
        if (wl_display_dispatch_pending(a->display) < 0) {
            wayland_failure(a, "wl_display_dispatch_pending");
            break;
        }
        if (fds[0].revents & (POLLERR | POLLHUP)) {
            app_error(a, "Wayland connection lost");
            app_quit(a, HOST_EXIT_NO_PROTOCOL);
            break;
        }

        if (fds[1].revents & POLLIN) {
            drain_wakeup_pipe(a);
            player_handle_events(a);
            player_handle_update(a);
        }
        if (fds[2].revents & POLLIN) {
            struct signalfd_siginfo info;
            ssize_t n = read(a->signal_fd, &info, sizeof info);
            if (n == (ssize_t)sizeof info) {
                app_log(a, "signal %u received, exiting", info.ssi_signo);
                app_quit(a, HOST_EXIT_OK);
            } else if (n < 0 && errno != EAGAIN && errno != EINTR) {
                app_error(a, "signalfd read failed: %s", strerror(errno));
            }
        }
        if (stdin_index >= 0 && (fds[stdin_index].revents & (POLLIN | POLLHUP | POLLERR)))
            ipc_read_stdin(a);
        player_poll_screenshots(a);
    }
}

int main(int argc, char **argv)
{
    struct app app;
    memset(&app, 0, sizeof app);
    app.layer_namespace = "lively-wallpaper";
    app.layer = ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND;
    app.hwdec = "auto-safe";
    app.title = "Lively Wallpaper";
    app.scaler = SCALER_UNIFORM;
    app.wakeup_pipe[0] = -1;
    app.wakeup_pipe[1] = -1;
    app.signal_fd = -1;
    app.egl_display = EGL_NO_DISPLAY;
    app.egl_context = EGL_NO_CONTEXT;
    app.egl_surface = EGL_NO_SURFACE;
    app.running = true;
    app.exit_code = HOST_EXIT_OK;

    int rc = parse_args(&app, argc, argv);
    if (rc < 0) {
        usage(stderr);
        return HOST_EXIT_BAD_ARGS;
    }
    if (rc > 0) {
        usage(stdout);
        return HOST_EXIT_OK;
    }
    if (app.windowed) {
        if (app.output_name)
            app_log(&app, "--output %s is ignored in windowed mode", app.output_name);
        if (app.span.enabled) {
            app_log(&app, "--span is ignored in windowed mode");
            app.span.enabled = false;
        }
    }
    if (setup_signals(&app) < 0)
        return HOST_EXIT_RENDERER;

    rc = wl_init(&app);
    if (rc == 0)
        rc = egl_init(&app);
    if (rc == 0)
        rc = player_init(&app);
    if (rc == 0)
        rc = wl_create_surface(&app);
    if (rc == 0) {
        run_loop(&app);
        rc = app.exit_code;
    }

    player_shutdown(&app);
    wl_shutdown(&app);
    free(app.inbuf);
    if (app.signal_fd >= 0 && close(app.signal_fd) < 0)
        app_error(&app, "close signalfd: %s", strerror(errno));
    app_log(&app, "exit code %d", rc);
    return rc;
}
