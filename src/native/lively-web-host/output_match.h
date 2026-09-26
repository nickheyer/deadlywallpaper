#ifndef LWH_OUTPUT_MATCH_H
#define LWH_OUTPUT_MATCH_H

#include <gdk/gdk.h>

typedef enum {
    LWH_OUTPUT_FOUND,            /* *monitor is the GdkMonitor whose xdg-output name matches */
    LWH_OUTPUT_NO_XDG_OUTPUT,    /* compositor lacks zxdg_output_manager_v1 (exit 3) */
    LWH_OUTPUT_NOT_FOUND,        /* no output has that name (exit 4) */
    LWH_OUTPUT_ERROR             /* Wayland connection error (exit 3) */
} LwhOutputStatus;

/* Binds zxdg_output_manager_v1 on GDK's wl_display, asks for an xdg_output of
 * every GdkMonitor's wl_output and matches the reported names against name.
 * *known receives a comma separated list of the names that were seen. */
LwhOutputStatus lwh_output_find_monitor(GdkDisplay *display, const gchar *name,
                                        GdkMonitor **monitor, gchar **known);

#endif
