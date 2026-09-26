#include "output_match.h"

#include <gdk/gdkwayland.h>
#include <string.h>
#include <wayland-client.h>

#include "xdg-output-unstable-v1-client-protocol.h"

typedef struct {
    struct zxdg_output_manager_v1 *manager;
} RegistryState;

typedef struct {
    GdkMonitor *monitor;
    struct zxdg_output_v1 *xdg_output;
    gchar *name;
} OutputEntry;

static void registry_global(void *data, struct wl_registry *registry, uint32_t id,
                            const char *interface, uint32_t version)
{
    RegistryState *state = data;

    if (state->manager == NULL && strcmp(interface, zxdg_output_manager_v1_interface.name) == 0)
        state->manager = wl_registry_bind(registry, id, &zxdg_output_manager_v1_interface, MIN(version, 3u));
}

static void registry_global_remove(G_GNUC_UNUSED void *data, G_GNUC_UNUSED struct wl_registry *registry,
                                   G_GNUC_UNUSED uint32_t id)
{
}

static const struct wl_registry_listener registry_listener = {
    registry_global,
    registry_global_remove
};

static void xdg_logical_position(G_GNUC_UNUSED void *data, G_GNUC_UNUSED struct zxdg_output_v1 *output,
                                 G_GNUC_UNUSED int32_t x, G_GNUC_UNUSED int32_t y)
{
}

static void xdg_logical_size(G_GNUC_UNUSED void *data, G_GNUC_UNUSED struct zxdg_output_v1 *output,
                             G_GNUC_UNUSED int32_t width, G_GNUC_UNUSED int32_t height)
{
}

static void xdg_done(G_GNUC_UNUSED void *data, G_GNUC_UNUSED struct zxdg_output_v1 *output)
{
}

static void xdg_name(void *data, G_GNUC_UNUSED struct zxdg_output_v1 *output, const char *name)
{
    OutputEntry *entry = data;

    g_free(entry->name);
    entry->name = g_strdup(name);
}

static void xdg_description(G_GNUC_UNUSED void *data, G_GNUC_UNUSED struct zxdg_output_v1 *output,
                            G_GNUC_UNUSED const char *description)
{
}

static const struct zxdg_output_v1_listener xdg_output_listener = {
    xdg_logical_position,
    xdg_logical_size,
    xdg_done,
    xdg_name,
    xdg_description
};

LwhOutputStatus lwh_output_find_monitor(GdkDisplay *display, const gchar *name,
                                        GdkMonitor **monitor, gchar **known)
{
    struct wl_display *wl_display = gdk_wayland_display_get_wl_display(display);
    struct wl_event_queue *queue;
    struct wl_display *wrapper;
    struct wl_registry *registry;
    RegistryState state = { NULL };
    LwhOutputStatus status = LWH_OUTPUT_NOT_FOUND;
    GString *names = g_string_new("");
    OutputEntry *entries;
    int count;

    *monitor = NULL;
    *known = NULL;

    queue = wl_display_create_queue(wl_display);
    wrapper = wl_proxy_create_wrapper(wl_display);
    if (queue == NULL || wrapper == NULL) {
        if (wrapper != NULL)
            wl_proxy_wrapper_destroy(wrapper);
        if (queue != NULL)
            wl_event_queue_destroy(queue);
        g_string_free(names, TRUE);
        return LWH_OUTPUT_ERROR;
    }
    wl_proxy_set_queue((struct wl_proxy *)wrapper, queue);
    registry = wl_display_get_registry(wrapper);
    wl_registry_add_listener(registry, &registry_listener, &state);
    if (wl_display_roundtrip_queue(wl_display, queue) < 0) {
        wl_registry_destroy(registry);
        wl_proxy_wrapper_destroy(wrapper);
        wl_event_queue_destroy(queue);
        g_string_free(names, TRUE);
        return LWH_OUTPUT_ERROR;
    }
    if (state.manager == NULL) {
        wl_registry_destroy(registry);
        wl_proxy_wrapper_destroy(wrapper);
        wl_event_queue_destroy(queue);
        g_string_free(names, TRUE);
        return LWH_OUTPUT_NO_XDG_OUTPUT;
    }

    count = gdk_display_get_n_monitors(display);
    entries = g_new0(OutputEntry, count > 0 ? count : 1);
    for (int i = 0; i < count; i++) {
        entries[i].monitor = gdk_display_get_monitor(display, i);
        entries[i].xdg_output = zxdg_output_manager_v1_get_xdg_output(
            state.manager, gdk_wayland_monitor_get_wl_output(entries[i].monitor));
        zxdg_output_v1_add_listener(entries[i].xdg_output, &xdg_output_listener, &entries[i]);
    }
    if (wl_display_roundtrip_queue(wl_display, queue) < 0)
        status = LWH_OUTPUT_ERROR;

    for (int i = 0; i < count; i++) {
        if (entries[i].name != NULL) {
            if (names->len > 0)
                g_string_append(names, ", ");
            g_string_append(names, entries[i].name);
            if (status == LWH_OUTPUT_NOT_FOUND && strcmp(entries[i].name, name) == 0) {
                *monitor = entries[i].monitor;
                status = LWH_OUTPUT_FOUND;
            }
        }
        zxdg_output_v1_destroy(entries[i].xdg_output);
        g_free(entries[i].name);
    }
    g_free(entries);

    zxdg_output_manager_v1_destroy(state.manager);
    wl_registry_destroy(registry);
    wl_proxy_wrapper_destroy(wrapper);
    wl_event_queue_destroy(queue);
    *known = g_string_free(names, FALSE);
    return status;
}
