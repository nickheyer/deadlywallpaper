#ifndef LWH_ARGS_H
#define LWH_ARGS_H

#include <glib.h>

typedef enum {
    LWH_PAGE_LOCAL,
    LWH_PAGE_ONLINE
} LwhPageType;

typedef enum {
    LWH_SCHEME_AUTO,
    LWH_SCHEME_DARK,
    LWH_SCHEME_LIGHT
} LwhColorScheme;

typedef struct {
    /* common options (PROTOCOL.md section 3) */
    gchar *output;
    gchar *ns;
    gboolean layer_bottom;
    gboolean span;
    gint span_x, span_y, span_w, span_h, span_vw, span_vh;
    gboolean interactive;
    /* preview mode: a normal window instead of a layer surface */
    gboolean windowed;
    gint window_w, window_h;
    gchar *title;
    gchar *property;
    gint volume;
    gboolean verbose;
    /* web host options (PROTOCOL.md section 5) */
    LwhPageType type;
    gint debug_port;          /* 0 when --debug is absent */
    LwhColorScheme scheme;
    gboolean pause_media;
    gboolean audio;
    gboolean sysinfo;
    gboolean nowplaying;
    gboolean pause_event;
    gboolean scale_given;
    gdouble scale;
    gchar *user_data;
    gchar *target;            /* <path-or-url> */
} LwhArgs;

/* Parses the command line. On failure *error describes the problem (exit code 2). */
gboolean lwh_args_parse(LwhArgs *args, int argc, char **argv, GError **error);
void lwh_args_clear(LwhArgs *args);

#endif
