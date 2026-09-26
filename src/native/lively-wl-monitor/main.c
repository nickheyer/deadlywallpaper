/*
 * lively-wl-monitor: reports Wayland outputs and foreign toplevel window state
 * to the Lively Linux core as JSON lines (see ../PROTOCOL.md, section 6).
 *
 * Outputs come from wl_output + zxdg_output_v1. Toplevels come from
 * zwlr_foreign_toplevel_manager_v1 (wlroots compositors) or
 * org_kde_plasma_window_management (KWin); wlr is preferred when both exist.
 * On KWin the virtual desktop membership of every window and the current
 * virtual desktop (org_kde_plasma_virtual_desktop_management), the activity
 * membership and the "show desktop" state are reported as well.
 */
#define _POSIX_C_SOURCE 200809L

#include <errno.h>
#include <inttypes.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/signalfd.h>
#include <unistd.h>

#include <wayland-client.h>

#include "plasma-virtual-desktop-client-protocol.h"
#include "plasma-window-management-client-protocol.h"
#include "wlr-foreign-toplevel-management-unstable-v1-client-protocol.h"
#include "xdg-output-unstable-v1-client-protocol.h"

#define EXIT_RUNTIME_FAILURE 1
#define EXIT_BAD_ARGUMENTS 2
#define EXIT_MISSING_PROTOCOL 3

#define WL_OUTPUT_BIND_VERSION 4
#define XDG_OUTPUT_BIND_VERSION 3
#define WLR_TOPLEVEL_BIND_VERSION 3
#define PLASMA_WM_BIND_VERSION 21
#define PLASMA_VD_BIND_VERSION 2

enum toplevel_protocol {
	TOPLEVEL_NONE,
	TOPLEVEL_WLR,
	TOPLEVEL_PLASMA,
};

struct global {
	struct global *next;
	uint32_t name;
	char *interface;
	uint32_t version;
};

struct output {
	struct output *next;
	struct state *state;
	uint32_t global_name;
	struct wl_output *wl_output;
	struct zxdg_output_v1 *xdg_output;
	char *name;            /* wl_output.name (v4) */
	char *description;     /* wl_output.description (v4) */
	char *xdg_name;        /* zxdg_output_v1.name (v2) */
	char *xdg_description; /* zxdg_output_v1.description (v2) */
	char *make;
	char *model;
	int32_t physical_width_mm;
	int32_t physical_height_mm;
	int32_t transform;
	int32_t mode_width;
	int32_t mode_height;
	int32_t refresh_mhz;
	bool have_mode;
	int32_t logical_x;
	int32_t logical_y;
	int32_t logical_width;
	int32_t logical_height;
	bool have_logical_size;
	bool done; /* wl_output.done or zxdg_output_v1.done received */
};

struct toplevel {
	struct toplevel *next;
	struct state *state;
	char *id;
	char *app_id;
	char *title;
	bool activated;
	bool fullscreen;
	bool maximized;
	bool minimized;
	bool skip_taskbar;
	bool ready; /* wlr: first done received; plasma: initial_state received */
	bool have_geometry;
	int32_t geometry_x;
	int32_t geometry_y;
	int32_t geometry_width;
	int32_t geometry_height;
	struct zwlr_foreign_toplevel_handle_v1 *wlr_handle;
	struct output **wlr_outputs;
	size_t wlr_output_count;
	struct org_kde_plasma_window *plasma_window;
	/* Plasma: ids of the virtual desktops the window is on (empty = on all). */
	char **virtual_desktops;
	size_t virtual_desktop_count;
	/* Plasma: ids of the activities the window is on (empty = on all). */
	char **activities;
	size_t activity_count;
};

/* One org_kde_plasma_virtual_desktop; "active" is whether it is a current desktop. */
struct virtual_desktop {
	struct virtual_desktop *next;
	struct state *state;
	char *id;
	bool active;
	struct org_kde_plasma_virtual_desktop *proxy;
};

struct state {
	struct wl_display *display;
	struct wl_registry *registry;
	bool bound;
	struct global *pending_globals;
	uint32_t wl_output_version;
	uint32_t xdg_output_version;
	struct zxdg_output_manager_v1 *xdg_output_manager;
	struct zwlr_foreign_toplevel_manager_v1 *wlr_manager;
	struct org_kde_plasma_window_management *plasma_manager;
	uint32_t plasma_version;
	struct org_kde_plasma_virtual_desktop_management *vd_manager;
	struct virtual_desktop *virtual_desktops;
	bool show_desktop; /* Plasma "show desktop" mode is active */
	bool layer_shell;
	bool plasma_shell;
	enum toplevel_protocol protocol;
	struct output *outputs;
	struct toplevel *toplevels;
	bool outputs_dirty;
	bool toplevels_dirty;
	unsigned proxies_created;
	bool once;
	int signal_fd;
};

/* ------------------------------------------------------------------------- */
/* Utilities                                                                 */
/* ------------------------------------------------------------------------- */

static void die(int code, const char *fmt, ...)
{
	va_list ap;

	fputs("lively-wl-monitor: ", stderr);
	va_start(ap, fmt);
	vfprintf(stderr, fmt, ap);
	va_end(ap);
	fputc('\n', stderr);
	exit(code);
}

static void die_display(struct state *st)
{
	int err = wl_display_get_error(st->display);

	if (err == EPROTO) {
		const struct wl_interface *iface = NULL;
		uint32_t id = 0;
		uint32_t code = wl_display_get_protocol_error(st->display, &iface, &id);
		die(EXIT_RUNTIME_FAILURE, "Wayland protocol error %" PRIu32 " on %s@%" PRIu32,
		    code, iface ? iface->name : "unknown", id);
	}
	die(EXIT_RUNTIME_FAILURE, "Wayland display error: %s", strerror(err ? err : errno));
}

static void *xcalloc(size_t count, size_t size)
{
	void *p = calloc(count, size);

	if (!p)
		die(EXIT_RUNTIME_FAILURE, "out of memory");
	return p;
}

static char *xstrdup(const char *s)
{
	char *copy = strdup(s);

	if (!copy)
		die(EXIT_RUNTIME_FAILURE, "out of memory");
	return copy;
}

static void replace_string(char **dst, const char *src)
{
	char *copy = xstrdup(src);

	free(*dst);
	*dst = copy;
}

static uint32_t min_u32(uint32_t a, uint32_t b)
{
	return a < b ? a : b;
}

static int32_t clamp_u32_to_i32(uint32_t v)
{
	return v > (uint32_t)INT32_MAX ? INT32_MAX : (int32_t)v;
}

/* ------------------------------------------------------------------------- */
/* String lists (virtual desktop and activity ids of a window)               */
/* ------------------------------------------------------------------------- */

static bool string_list_contains(char *const *list, size_t count, const char *s)
{
	for (size_t i = 0; i < count; i++) {
		if (strcmp(list[i], s) == 0)
			return true;
	}
	return false;
}

/* Adds s unless present; returns true when the list changed. */
static bool string_list_add(char ***list, size_t *count, const char *s)
{
	char **grown;

	if (string_list_contains(*list, *count, s))
		return false;
	grown = realloc(*list, (*count + 1) * sizeof *grown);
	if (!grown)
		die(EXIT_RUNTIME_FAILURE, "out of memory");
	*list = grown;
	(*list)[(*count)++] = xstrdup(s);
	return true;
}

/* Removes s if present; returns true when the list changed. */
static bool string_list_remove(char **list, size_t *count, const char *s)
{
	for (size_t i = 0; i < *count; i++) {
		if (strcmp(list[i], s) != 0)
			continue;
		free(list[i]);
		memmove(&list[i], &list[i + 1], (*count - i - 1) * sizeof *list);
		(*count)--;
		return true;
	}
	return false;
}

static void string_list_free(char **list, size_t count)
{
	for (size_t i = 0; i < count; i++)
		free(list[i]);
	free(list);
}

/* ------------------------------------------------------------------------- */
/* JSON writing                                                              */
/* ------------------------------------------------------------------------- */

static void flush_line(void)
{
	if (fflush(stdout) != 0 || ferror(stdout))
		die(EXIT_RUNTIME_FAILURE, "writing to stdout failed: %s", strerror(errno));
}

/* Length of the well-formed UTF-8 sequence starting at p, or 0 if malformed. */
static size_t utf8_sequence_length(const unsigned char *p)
{
	unsigned char c = p[0];
	size_t len;
	unsigned char lo = 0x80;
	unsigned char hi = 0xBF;

	if (c >= 0xC2 && c <= 0xDF) {
		len = 2;
	} else if (c >= 0xE0 && c <= 0xEF) {
		len = 3;
		if (c == 0xE0)
			lo = 0xA0;
		else if (c == 0xED)
			hi = 0x9F;
	} else if (c >= 0xF0 && c <= 0xF4) {
		len = 4;
		if (c == 0xF0)
			lo = 0x90;
		else if (c == 0xF4)
			hi = 0x8F;
	} else {
		return 0;
	}
	if (p[1] < lo || p[1] > hi)
		return 0;
	for (size_t i = 2; i < len; i++) {
		if (p[i] < 0x80 || p[i] > 0xBF)
			return 0;
	}
	return len;
}

/* Writes s as a JSON string literal. NULL is written as "". Malformed UTF-8
 * bytes are replaced by U+FFFD so every emitted line is valid UTF-8 JSON. */
static void json_write_string(FILE *out, const char *s)
{
	static const char hex[] = "0123456789abcdef";
	const unsigned char *p = (const unsigned char *)(s ? s : "");

	fputc('"', out);
	while (*p) {
		unsigned char c = *p;

		if (c == '"') {
			fputs("\\\"", out);
			p++;
		} else if (c == '\\') {
			fputs("\\\\", out);
			p++;
		} else if (c < 0x20) {
			switch (c) {
			case '\b':
				fputs("\\b", out);
				break;
			case '\f':
				fputs("\\f", out);
				break;
			case '\n':
				fputs("\\n", out);
				break;
			case '\r':
				fputs("\\r", out);
				break;
			case '\t':
				fputs("\\t", out);
				break;
			default:
				fputs("\\u00", out);
				fputc(hex[c >> 4], out);
				fputc(hex[c & 0xF], out);
				break;
			}
			p++;
		} else if (c < 0x80) {
			fputc(c, out);
			p++;
		} else {
			size_t len = utf8_sequence_length(p);

			if (len == 0) {
				fputs("\\ufffd", out);
				p++;
			} else {
				fwrite(p, 1, len, out);
				p += len;
			}
		}
	}
	fputc('"', out);
}

static void json_write_bool(FILE *out, bool value)
{
	fputs(value ? "true" : "false", out);
}

static void json_write_string_list(FILE *out, char *const *list, size_t count)
{
	fputc('[', out);
	for (size_t i = 0; i < count; i++) {
		if (i > 0)
			fputc(',', out);
		json_write_string(out, list[i]);
	}
	fputc(']', out);
}

/* Writes a scale factor with three decimals, trailing zeros removed. */
static void json_write_scale(FILE *out, double scale)
{
	char buf[64];
	char *dot;

	snprintf(buf, sizeof buf, "%.3f", scale);
	dot = strchr(buf, '.');
	if (dot) {
		char *end = buf + strlen(buf);

		while (end > dot + 1 && end[-1] == '0')
			end--;
		if (end == dot + 1)
			end = dot;
		*end = '\0';
	}
	fputs(buf, out);
}

/* ------------------------------------------------------------------------- */
/* Outputs                                                                   */
/* ------------------------------------------------------------------------- */

static const char *output_name(const struct output *o)
{
	return o->name ? o->name : o->xdg_name;
}

static const char *output_description(const struct output *o)
{
	return o->description ? o->description : o->xdg_description;
}

static bool output_transform_swaps_axes(const struct output *o)
{
	switch (o->transform) {
	case WL_OUTPUT_TRANSFORM_90:
	case WL_OUTPUT_TRANSFORM_270:
	case WL_OUTPUT_TRANSFORM_FLIPPED_90:
	case WL_OUTPUT_TRANSFORM_FLIPPED_270:
		return true;
	default:
		return false;
	}
}

/* Width of the output in physical pixels along the logical x axis. */
static int32_t output_pixel_width(const struct output *o)
{
	return output_transform_swaps_axes(o) ? o->mode_height : o->mode_width;
}

/* An output is reported once its atomic batch finished and it carries a
 * name, a current mode and a non-empty logical rectangle. */
static bool output_complete(const struct output *o)
{
	return o->done && o->have_mode && o->have_logical_size &&
	       o->logical_width > 0 && o->logical_height > 0 &&
	       output_pixel_width(o) > 0 && output_name(o) != NULL;
}

static bool output_intersects(const struct output *o, int32_t x, int32_t y,
			      int32_t width, int32_t height)
{
	int64_t ox0 = o->logical_x;
	int64_t oy0 = o->logical_y;
	int64_t ox1 = ox0 + o->logical_width;
	int64_t oy1 = oy0 + o->logical_height;
	int64_t wx0 = x;
	int64_t wy0 = y;
	int64_t wx1 = wx0 + width;
	int64_t wy1 = wy0 + height;

	return width > 0 && height > 0 && wx0 < ox1 && wx1 > ox0 && wy0 < oy1 && wy1 > oy0;
}

static void output_handle_geometry(void *data, struct wl_output *wl_output, int32_t x,
				   int32_t y, int32_t physical_width, int32_t physical_height,
				   int32_t subpixel, const char *make, const char *model,
				   int32_t transform)
{
	struct output *o = data;

	(void)wl_output;
	(void)x;
	(void)y;
	(void)subpixel;
	o->physical_width_mm = physical_width;
	o->physical_height_mm = physical_height;
	o->transform = transform;
	replace_string(&o->make, make);
	replace_string(&o->model, model);
}

static void output_handle_mode(void *data, struct wl_output *wl_output, uint32_t flags,
			       int32_t width, int32_t height, int32_t refresh)
{
	struct output *o = data;

	(void)wl_output;
	if (!(flags & WL_OUTPUT_MODE_CURRENT))
		return;
	o->mode_width = width;
	o->mode_height = height;
	o->refresh_mhz = refresh;
	o->have_mode = true;
}

static void output_handle_done(void *data, struct wl_output *wl_output)
{
	struct output *o = data;

	(void)wl_output;
	o->done = true;
	o->state->outputs_dirty = true;
}

static void output_handle_scale(void *data, struct wl_output *wl_output, int32_t factor)
{
	/* The integer buffer scale is not reported; the real scale is computed
	 * from the current mode and the logical size. */
	(void)data;
	(void)wl_output;
	(void)factor;
}

static void output_handle_name(void *data, struct wl_output *wl_output, const char *name)
{
	struct output *o = data;

	(void)wl_output;
	replace_string(&o->name, name);
}

static void output_handle_description(void *data, struct wl_output *wl_output,
				      const char *description)
{
	struct output *o = data;

	(void)wl_output;
	replace_string(&o->description, description);
}

static const struct wl_output_listener output_listener = {
	.geometry = output_handle_geometry,
	.mode = output_handle_mode,
	.done = output_handle_done,
	.scale = output_handle_scale,
	.name = output_handle_name,
	.description = output_handle_description,
};

static void xdg_output_handle_logical_position(void *data, struct zxdg_output_v1 *xdg_output,
					       int32_t x, int32_t y)
{
	struct output *o = data;

	(void)xdg_output;
	o->logical_x = x;
	o->logical_y = y;
	o->state->outputs_dirty = true;
}

static void xdg_output_handle_logical_size(void *data, struct zxdg_output_v1 *xdg_output,
					   int32_t width, int32_t height)
{
	struct output *o = data;

	(void)xdg_output;
	o->logical_width = width;
	o->logical_height = height;
	o->have_logical_size = true;
	o->state->outputs_dirty = true;
}

static void xdg_output_handle_done(void *data, struct zxdg_output_v1 *xdg_output)
{
	struct output *o = data;

	(void)xdg_output;
	o->done = true;
	o->state->outputs_dirty = true;
}

static void xdg_output_handle_name(void *data, struct zxdg_output_v1 *xdg_output,
				   const char *name)
{
	struct output *o = data;

	(void)xdg_output;
	replace_string(&o->xdg_name, name);
}

static void xdg_output_handle_description(void *data, struct zxdg_output_v1 *xdg_output,
					  const char *description)
{
	struct output *o = data;

	(void)xdg_output;
	replace_string(&o->xdg_description, description);
}

static const struct zxdg_output_v1_listener xdg_output_listener = {
	.logical_position = xdg_output_handle_logical_position,
	.logical_size = xdg_output_handle_logical_size,
	.done = xdg_output_handle_done,
	.name = xdg_output_handle_name,
	.description = xdg_output_handle_description,
};

static void output_ensure_xdg(struct state *st, struct output *o)
{
	if (o->xdg_output || !st->xdg_output_manager)
		return;
	o->xdg_output = zxdg_output_manager_v1_get_xdg_output(st->xdg_output_manager, o->wl_output);
	if (!o->xdg_output)
		die(EXIT_RUNTIME_FAILURE, "out of memory creating zxdg_output_v1");
	zxdg_output_v1_add_listener(o->xdg_output, &xdg_output_listener, o);
	st->proxies_created++;
}

static void output_create(struct state *st, uint32_t global_name, uint32_t version)
{
	struct output *o = xcalloc(1, sizeof *o);
	struct output **tail;

	o->state = st;
	o->global_name = global_name;
	o->wl_output = wl_registry_bind(st->registry, global_name, &wl_output_interface,
					min_u32(version, st->wl_output_version));
	if (!o->wl_output)
		die(EXIT_RUNTIME_FAILURE, "out of memory binding wl_output");
	wl_output_add_listener(o->wl_output, &output_listener, o);
	st->proxies_created++;

	for (tail = &st->outputs; *tail; tail = &(*tail)->next)
		;
	*tail = o;

	output_ensure_xdg(st, o);
}

static bool toplevel_remove_output(struct toplevel *t, const struct output *o);
static bool toplevel_visible(const struct toplevel *t);

static void output_destroy(struct state *st, struct output *o)
{
	struct output **link;

	for (struct toplevel *t = st->toplevels; t; t = t->next) {
		if (toplevel_remove_output(t, o) && toplevel_visible(t))
			st->toplevels_dirty = true;
	}

	for (link = &st->outputs; *link && *link != o; link = &(*link)->next)
		;
	if (*link == o)
		*link = o->next;

	if (o->xdg_output)
		zxdg_output_v1_destroy(o->xdg_output);
	if (wl_output_get_version(o->wl_output) >= WL_OUTPUT_RELEASE_SINCE_VERSION)
		wl_output_release(o->wl_output);
	else
		wl_output_destroy(o->wl_output);

	free(o->name);
	free(o->description);
	free(o->xdg_name);
	free(o->xdg_description);
	free(o->make);
	free(o->model);
	free(o);
}

static void emit_outputs(struct state *st)
{
	bool first = true;

	fputs("{\"event\":\"outputs\",\"outputs\":[", stdout);
	for (const struct output *o = st->outputs; o; o = o->next) {
		if (!output_complete(o))
			continue;
		if (!first)
			fputc(',', stdout);
		first = false;
		fputs("{\"name\":", stdout);
		json_write_string(stdout, output_name(o));
		fputs(",\"description\":", stdout);
		json_write_string(stdout, output_description(o));
		fputs(",\"make\":", stdout);
		json_write_string(stdout, o->make);
		fputs(",\"model\":", stdout);
		json_write_string(stdout, o->model);
		printf(",\"x\":%" PRId32 ",\"y\":%" PRId32 ",\"width\":%" PRId32
		       ",\"height\":%" PRId32 ",\"scale\":",
		       o->logical_x, o->logical_y, o->logical_width, o->logical_height);
		json_write_scale(stdout, (double)output_pixel_width(o) / (double)o->logical_width);
		printf(",\"transform\":%" PRId32 ",\"physical_width_mm\":%" PRId32
		       ",\"physical_height_mm\":%" PRId32 ",\"refresh_mhz\":%" PRId32 "}",
		       o->transform, o->physical_width_mm, o->physical_height_mm, o->refresh_mhz);
	}
	fputs("]}\n", stdout);
	flush_line();
	st->outputs_dirty = false;
}

/* ------------------------------------------------------------------------- */
/* Toplevels (shared)                                                        */
/* ------------------------------------------------------------------------- */

static bool toplevel_visible(const struct toplevel *t)
{
	return t->ready;
}

static struct toplevel *toplevel_create(struct state *st)
{
	struct toplevel *t = xcalloc(1, sizeof *t);
	struct toplevel **tail;

	t->state = st;
	for (tail = &st->toplevels; *tail; tail = &(*tail)->next)
		;
	*tail = t;
	return t;
}

static void toplevel_destroy(struct state *st, struct toplevel *t)
{
	struct toplevel **link;

	if (toplevel_visible(t))
		st->toplevels_dirty = true;

	for (link = &st->toplevels; *link && *link != t; link = &(*link)->next)
		;
	if (*link == t)
		*link = t->next;

	if (t->wlr_handle)
		zwlr_foreign_toplevel_handle_v1_destroy(t->wlr_handle);
	if (t->plasma_window) {
		if (st->plasma_version >= ORG_KDE_PLASMA_WINDOW_DESTROY_SINCE_VERSION)
			org_kde_plasma_window_destroy(t->plasma_window);
		else
			wl_proxy_destroy((struct wl_proxy *)t->plasma_window);
	}
	free(t->id);
	free(t->app_id);
	free(t->title);
	free(t->wlr_outputs);
	string_list_free(t->virtual_desktops, t->virtual_desktop_count);
	string_list_free(t->activities, t->activity_count);
	free(t);
}

static void toplevel_add_output(struct toplevel *t, struct output *o)
{
	struct output **grown;

	for (size_t i = 0; i < t->wlr_output_count; i++) {
		if (t->wlr_outputs[i] == o)
			return;
	}
	grown = realloc(t->wlr_outputs, (t->wlr_output_count + 1) * sizeof *grown);
	if (!grown)
		die(EXIT_RUNTIME_FAILURE, "out of memory");
	t->wlr_outputs = grown;
	t->wlr_outputs[t->wlr_output_count++] = o;
}

static bool toplevel_remove_output(struct toplevel *t, const struct output *o)
{
	for (size_t i = 0; i < t->wlr_output_count; i++) {
		if (t->wlr_outputs[i] != o)
			continue;
		memmove(&t->wlr_outputs[i], &t->wlr_outputs[i + 1],
			(t->wlr_output_count - i - 1) * sizeof *t->wlr_outputs);
		t->wlr_output_count--;
		return true;
	}
	return false;
}

static void toplevel_mark_dirty(struct toplevel *t)
{
	if (toplevel_visible(t))
		t->state->toplevels_dirty = true;
}

static void write_toplevel_outputs(FILE *out, const struct state *st, const struct toplevel *t)
{
	bool first = true;

	fputc('[', out);
	if (st->protocol == TOPLEVEL_WLR) {
		for (size_t i = 0; i < t->wlr_output_count; i++) {
			const struct output *o = t->wlr_outputs[i];

			if (!output_complete(o))
				continue;
			if (!first)
				fputc(',', out);
			first = false;
			json_write_string(out, output_name(o));
		}
	} else if (t->have_geometry) {
		for (const struct output *o = st->outputs; o; o = o->next) {
			if (!output_complete(o) ||
			    !output_intersects(o, t->geometry_x, t->geometry_y,
					       t->geometry_width, t->geometry_height))
				continue;
			if (!first)
				fputc(',', out);
			first = false;
			json_write_string(out, output_name(o));
		}
	}
	fputc(']', out);
}

/* A window is on the current virtual desktop when it is on every desktop (no
 * membership reported) or when one of its desktops is currently activated.
 * Without org_kde_plasma_virtual_desktop_management there is no notion of a
 * current desktop and every window counts as being on it. */
static bool toplevel_on_current_desktop(const struct state *st, const struct toplevel *t)
{
	if (!st->vd_manager || t->virtual_desktop_count == 0)
		return true;
	for (const struct virtual_desktop *vd = st->virtual_desktops; vd; vd = vd->next) {
		if (vd->active && vd->id &&
		    string_list_contains(t->virtual_desktops, t->virtual_desktop_count, vd->id))
			return true;
	}
	return false;
}

static void emit_toplevels(struct state *st)
{
	bool first = true;

	fputs("{\"event\":\"toplevels\",\"show_desktop\":", stdout);
	json_write_bool(stdout, st->show_desktop);
	fputs(",\"toplevels\":[", stdout);
	for (const struct toplevel *t = st->toplevels; t; t = t->next) {
		if (!toplevel_visible(t))
			continue;
		if (!first)
			fputc(',', stdout);
		first = false;
		fputs("{\"id\":", stdout);
		json_write_string(stdout, t->id);
		fputs(",\"app_id\":", stdout);
		json_write_string(stdout, t->app_id);
		fputs(",\"title\":", stdout);
		json_write_string(stdout, t->title);
		fputs(",\"activated\":", stdout);
		json_write_bool(stdout, t->activated);
		fputs(",\"fullscreen\":", stdout);
		json_write_bool(stdout, t->fullscreen);
		fputs(",\"maximized\":", stdout);
		json_write_bool(stdout, t->maximized);
		fputs(",\"minimized\":", stdout);
		json_write_bool(stdout, t->minimized);
		fputs(",\"skip_taskbar\":", stdout);
		json_write_bool(stdout, t->skip_taskbar);
		fputs(",\"outputs\":", stdout);
		write_toplevel_outputs(stdout, st, t);
		fputs(",\"geometry\":", stdout);
		if (t->have_geometry) {
			printf("[%" PRId32 ",%" PRId32 ",%" PRId32 ",%" PRId32 "]",
			       t->geometry_x, t->geometry_y, t->geometry_width, t->geometry_height);
		} else {
			fputs("null", stdout);
		}
		fputs(",\"virtual_desktops\":", stdout);
		json_write_string_list(stdout, t->virtual_desktops, t->virtual_desktop_count);
		fputs(",\"activities\":", stdout);
		json_write_string_list(stdout, t->activities, t->activity_count);
		fputs(",\"on_current_desktop\":", stdout);
		json_write_bool(stdout, toplevel_on_current_desktop(st, t));
		fputc('}', stdout);
	}
	fputs("]}\n", stdout);
	flush_line();
	st->toplevels_dirty = false;
}

/* ------------------------------------------------------------------------- */
/* wlr-foreign-toplevel-management                                           */
/* ------------------------------------------------------------------------- */

static void wlr_handle_title(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle,
			     const char *title)
{
	struct toplevel *t = data;

	(void)handle;
	replace_string(&t->title, title);
}

static void wlr_handle_app_id(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle,
			      const char *app_id)
{
	struct toplevel *t = data;

	(void)handle;
	replace_string(&t->app_id, app_id);
}

static void wlr_handle_output_enter(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle,
				    struct wl_output *wl_output)
{
	struct toplevel *t = data;
	struct output *o;

	(void)handle;
	if (!wl_output)
		return;
	o = wl_output_get_user_data(wl_output);
	if (!o)
		return;
	toplevel_add_output(t, o);
}

static void wlr_handle_output_leave(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle,
				    struct wl_output *wl_output)
{
	struct toplevel *t = data;

	(void)handle;
	if (!wl_output)
		return;
	toplevel_remove_output(t, wl_output_get_user_data(wl_output));
}

static void wlr_handle_state(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle,
			     struct wl_array *state)
{
	struct toplevel *t = data;
	uint32_t *entry;

	(void)handle;
	t->activated = false;
	t->fullscreen = false;
	t->maximized = false;
	t->minimized = false;
	wl_array_for_each(entry, state) {
		switch (*entry) {
		case ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_MAXIMIZED:
			t->maximized = true;
			break;
		case ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_MINIMIZED:
			t->minimized = true;
			break;
		case ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_ACTIVATED:
			t->activated = true;
			break;
		case ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_FULLSCREEN:
			t->fullscreen = true;
			break;
		default:
			break;
		}
	}
}

static void wlr_handle_done(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle)
{
	struct toplevel *t = data;

	(void)handle;
	t->ready = true;
	t->state->toplevels_dirty = true;
}

static void wlr_handle_closed(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle)
{
	struct toplevel *t = data;

	(void)handle;
	toplevel_destroy(t->state, t);
}

static void wlr_handle_parent(void *data, struct zwlr_foreign_toplevel_handle_v1 *handle,
			      struct zwlr_foreign_toplevel_handle_v1 *parent)
{
	(void)data;
	(void)handle;
	(void)parent;
}

static const struct zwlr_foreign_toplevel_handle_v1_listener wlr_handle_listener = {
	.title = wlr_handle_title,
	.app_id = wlr_handle_app_id,
	.output_enter = wlr_handle_output_enter,
	.output_leave = wlr_handle_output_leave,
	.state = wlr_handle_state,
	.done = wlr_handle_done,
	.closed = wlr_handle_closed,
	.parent = wlr_handle_parent,
};

static void wlr_manager_handle_toplevel(void *data,
					struct zwlr_foreign_toplevel_manager_v1 *manager,
					struct zwlr_foreign_toplevel_handle_v1 *handle)
{
	struct state *st = data;
	struct toplevel *t = toplevel_create(st);
	char id[16];

	(void)manager;
	snprintf(id, sizeof id, "%" PRIu32, wl_proxy_get_id((struct wl_proxy *)handle));
	t->id = xstrdup(id);
	t->wlr_handle = handle;
	/* wlr only lists windows meant for taskbars and docks. */
	t->skip_taskbar = false;
	zwlr_foreign_toplevel_handle_v1_add_listener(handle, &wlr_handle_listener, t);
	st->proxies_created++;
}

static void wlr_manager_handle_finished(void *data,
					struct zwlr_foreign_toplevel_manager_v1 *manager)
{
	(void)data;
	(void)manager;
	/* Only sent after a stop request, which this program never issues; a
	 * compositor doing it on its own has ended the toplevel feed. */
	die(EXIT_RUNTIME_FAILURE, "compositor finished zwlr_foreign_toplevel_manager_v1");
}

static const struct zwlr_foreign_toplevel_manager_v1_listener wlr_manager_listener = {
	.toplevel = wlr_manager_handle_toplevel,
	.finished = wlr_manager_handle_finished,
};

/* ------------------------------------------------------------------------- */
/* org_kde_plasma_window_management                                          */
/* ------------------------------------------------------------------------- */

static void plasma_window_handle_title_changed(void *data, struct org_kde_plasma_window *window,
					       const char *title)
{
	struct toplevel *t = data;

	(void)window;
	replace_string(&t->title, title);
	toplevel_mark_dirty(t);
}

static void plasma_window_handle_app_id_changed(void *data, struct org_kde_plasma_window *window,
						const char *app_id)
{
	struct toplevel *t = data;

	(void)window;
	replace_string(&t->app_id, app_id);
	toplevel_mark_dirty(t);
}

static void plasma_window_handle_state_changed(void *data, struct org_kde_plasma_window *window,
					       uint32_t flags)
{
	struct toplevel *t = data;

	(void)window;
	t->activated = (flags & ORG_KDE_PLASMA_WINDOW_MANAGEMENT_STATE_ACTIVE) != 0;
	t->minimized = (flags & ORG_KDE_PLASMA_WINDOW_MANAGEMENT_STATE_MINIMIZED) != 0;
	t->maximized = (flags & ORG_KDE_PLASMA_WINDOW_MANAGEMENT_STATE_MAXIMIZED) != 0;
	t->fullscreen = (flags & ORG_KDE_PLASMA_WINDOW_MANAGEMENT_STATE_FULLSCREEN) != 0;
	t->skip_taskbar = (flags & ORG_KDE_PLASMA_WINDOW_MANAGEMENT_STATE_SKIPTASKBAR) != 0;
	toplevel_mark_dirty(t);
}

static void plasma_window_handle_virtual_desktop_changed(void *data,
							 struct org_kde_plasma_window *window,
							 int32_t number)
{
	(void)data;
	(void)window;
	(void)number;
}

static void plasma_window_handle_themed_icon_name_changed(void *data,
							  struct org_kde_plasma_window *window,
							  const char *name)
{
	(void)data;
	(void)window;
	(void)name;
}

static void plasma_window_handle_unmapped(void *data, struct org_kde_plasma_window *window)
{
	struct toplevel *t = data;

	(void)window;
	toplevel_destroy(t->state, t);
}

static void plasma_window_handle_initial_state(void *data, struct org_kde_plasma_window *window)
{
	struct toplevel *t = data;

	(void)window;
	t->ready = true;
	toplevel_mark_dirty(t);
}

static void plasma_window_handle_parent_window(void *data, struct org_kde_plasma_window *window,
					       struct org_kde_plasma_window *parent)
{
	(void)data;
	(void)window;
	(void)parent;
}

static void plasma_window_handle_geometry(void *data, struct org_kde_plasma_window *window,
					  int32_t x, int32_t y, uint32_t width, uint32_t height)
{
	struct toplevel *t = data;

	(void)window;
	t->geometry_x = x;
	t->geometry_y = y;
	t->geometry_width = clamp_u32_to_i32(width);
	t->geometry_height = clamp_u32_to_i32(height);
	t->have_geometry = true;
	toplevel_mark_dirty(t);
}

static void plasma_window_handle_icon_changed(void *data, struct org_kde_plasma_window *window)
{
	(void)data;
	(void)window;
}

static void plasma_window_handle_pid_changed(void *data, struct org_kde_plasma_window *window,
					     uint32_t pid)
{
	(void)data;
	(void)window;
	(void)pid;
}

static void plasma_window_handle_virtual_desktop_entered(void *data,
							 struct org_kde_plasma_window *window,
							 const char *id)
{
	struct toplevel *t = data;

	(void)window;
	if (string_list_add(&t->virtual_desktops, &t->virtual_desktop_count, id))
		toplevel_mark_dirty(t);
}

static void plasma_window_handle_virtual_desktop_left(void *data,
						      struct org_kde_plasma_window *window,
						      const char *id)
{
	struct toplevel *t = data;

	(void)window;
	if (string_list_remove(t->virtual_desktops, &t->virtual_desktop_count, id))
		toplevel_mark_dirty(t);
}

static void plasma_window_handle_application_menu(void *data,
						  struct org_kde_plasma_window *window,
						  const char *service_name,
						  const char *object_path)
{
	(void)data;
	(void)window;
	(void)service_name;
	(void)object_path;
}

static void plasma_window_handle_activity_entered(void *data,
						  struct org_kde_plasma_window *window,
						  const char *id)
{
	struct toplevel *t = data;

	(void)window;
	if (string_list_add(&t->activities, &t->activity_count, id))
		toplevel_mark_dirty(t);
}

static void plasma_window_handle_activity_left(void *data, struct org_kde_plasma_window *window,
					       const char *id)
{
	struct toplevel *t = data;

	(void)window;
	if (string_list_remove(t->activities, &t->activity_count, id))
		toplevel_mark_dirty(t);
}

static void plasma_window_handle_resource_name_changed(void *data,
						       struct org_kde_plasma_window *window,
						       const char *resource_name)
{
	(void)data;
	(void)window;
	(void)resource_name;
}

static void plasma_window_handle_client_geometry(void *data, struct org_kde_plasma_window *window,
						 int32_t x, int32_t y, uint32_t width,
						 uint32_t height)
{
	/* The frame geometry from the geometry event is what is reported. */
	(void)data;
	(void)window;
	(void)x;
	(void)y;
	(void)width;
	(void)height;
}

static void plasma_window_handle_mapped(void *data, struct org_kde_plasma_window *window)
{
	/* Windows are listed from initial_state on, whether or not the compositor
	 * has painted them yet. */
	(void)data;
	(void)window;
}

static const struct org_kde_plasma_window_listener plasma_window_listener = {
	.title_changed = plasma_window_handle_title_changed,
	.app_id_changed = plasma_window_handle_app_id_changed,
	.state_changed = plasma_window_handle_state_changed,
	.virtual_desktop_changed = plasma_window_handle_virtual_desktop_changed,
	.themed_icon_name_changed = plasma_window_handle_themed_icon_name_changed,
	.unmapped = plasma_window_handle_unmapped,
	.initial_state = plasma_window_handle_initial_state,
	.parent_window = plasma_window_handle_parent_window,
	.geometry = plasma_window_handle_geometry,
	.icon_changed = plasma_window_handle_icon_changed,
	.pid_changed = plasma_window_handle_pid_changed,
	.virtual_desktop_entered = plasma_window_handle_virtual_desktop_entered,
	.virtual_desktop_left = plasma_window_handle_virtual_desktop_left,
	.application_menu = plasma_window_handle_application_menu,
	.activity_entered = plasma_window_handle_activity_entered,
	.activity_left = plasma_window_handle_activity_left,
	.resource_name_changed = plasma_window_handle_resource_name_changed,
	.client_geometry = plasma_window_handle_client_geometry,
	.mapped = plasma_window_handle_mapped,
};

static void plasma_window_track(struct state *st, struct org_kde_plasma_window *window,
				uint32_t internal_id)
{
	struct toplevel *t;
	char id[16];

	if (!window)
		die(EXIT_RUNTIME_FAILURE, "out of memory creating org_kde_plasma_window");
	t = toplevel_create(st);
	snprintf(id, sizeof id, "%" PRIu32, internal_id);
	t->id = xstrdup(id);
	t->plasma_window = window;
	t->ready = st->plasma_version < ORG_KDE_PLASMA_WINDOW_INITIAL_STATE_SINCE_VERSION;
	org_kde_plasma_window_add_listener(window, &plasma_window_listener, t);
	st->proxies_created++;
}

static void plasma_manager_handle_show_desktop_changed(
	void *data, struct org_kde_plasma_window_management *manager, uint32_t state)
{
	struct state *st = data;
	bool enabled = state == ORG_KDE_PLASMA_WINDOW_MANAGEMENT_SHOW_DESKTOP_ENABLED;

	(void)manager;
	if (st->show_desktop == enabled)
		return;
	st->show_desktop = enabled;
	st->toplevels_dirty = true;
}

static void plasma_manager_handle_window(void *data,
					 struct org_kde_plasma_window_management *manager,
					 uint32_t id)
{
	struct state *st = data;

	/* Compositors at version 13 or newer announce windows with
	 * window_with_uuid instead; this event is the pre-13 announcement. */
	if (st->plasma_version >= ORG_KDE_PLASMA_WINDOW_MANAGEMENT_WINDOW_WITH_UUID_SINCE_VERSION)
		return;
	plasma_window_track(st, org_kde_plasma_window_management_get_window(manager, id), id);
}

static void plasma_manager_handle_stacking_order_changed(
	void *data, struct org_kde_plasma_window_management *manager, struct wl_array *ids)
{
	(void)data;
	(void)manager;
	(void)ids;
}

static void plasma_manager_handle_stacking_order_uuid_changed(
	void *data, struct org_kde_plasma_window_management *manager, const char *uuids)
{
	(void)data;
	(void)manager;
	(void)uuids;
}

static void plasma_manager_handle_window_with_uuid(
	void *data, struct org_kde_plasma_window_management *manager, uint32_t id,
	const char *uuid)
{
	struct state *st = data;

	plasma_window_track(st, org_kde_plasma_window_management_get_window_by_uuid(manager, uuid),
			    id);
}

static void plasma_manager_handle_stacking_order_changed_2(
	void *data, struct org_kde_plasma_window_management *manager)
{
	(void)data;
	(void)manager;
}

static const struct org_kde_plasma_window_management_listener plasma_manager_listener = {
	.show_desktop_changed = plasma_manager_handle_show_desktop_changed,
	.window = plasma_manager_handle_window,
	.stacking_order_changed = plasma_manager_handle_stacking_order_changed,
	.stacking_order_uuid_changed = plasma_manager_handle_stacking_order_uuid_changed,
	.window_with_uuid = plasma_manager_handle_window_with_uuid,
	.stacking_order_changed_2 = plasma_manager_handle_stacking_order_changed_2,
};

/* ------------------------------------------------------------------------- */
/* org_kde_plasma_virtual_desktop_management                                 */
/* ------------------------------------------------------------------------- */

static void virtual_desktop_destroy(struct state *st, struct virtual_desktop *vd)
{
	struct virtual_desktop **link;

	for (link = &st->virtual_desktops; *link && *link != vd; link = &(*link)->next)
		;
	if (*link == vd)
		*link = vd->next;
	if (vd->active)
		st->toplevels_dirty = true;
	if (vd->proxy)
		org_kde_plasma_virtual_desktop_destroy(vd->proxy);
	free(vd->id);
	free(vd);
}

static struct virtual_desktop *virtual_desktop_find(struct state *st, const char *id)
{
	for (struct virtual_desktop *vd = st->virtual_desktops; vd; vd = vd->next) {
		if (vd->id && strcmp(vd->id, id) == 0)
			return vd;
	}
	return NULL;
}

static void virtual_desktop_handle_desktop_id(void *data,
					      struct org_kde_plasma_virtual_desktop *proxy,
					      const char *desktop_id)
{
	struct virtual_desktop *vd = data;

	(void)proxy;
	replace_string(&vd->id, desktop_id);
}

static void virtual_desktop_handle_name(void *data, struct org_kde_plasma_virtual_desktop *proxy,
					const char *name)
{
	(void)data;
	(void)proxy;
	(void)name;
}

static void virtual_desktop_handle_activated(void *data,
					     struct org_kde_plasma_virtual_desktop *proxy)
{
	struct virtual_desktop *vd = data;

	(void)proxy;
	if (vd->active)
		return;
	vd->active = true;
	vd->state->toplevels_dirty = true;
}

static void virtual_desktop_handle_deactivated(void *data,
					       struct org_kde_plasma_virtual_desktop *proxy)
{
	struct virtual_desktop *vd = data;

	(void)proxy;
	if (!vd->active)
		return;
	vd->active = false;
	vd->state->toplevels_dirty = true;
}

static void virtual_desktop_handle_done(void *data, struct org_kde_plasma_virtual_desktop *proxy)
{
	(void)data;
	(void)proxy;
}

static void virtual_desktop_handle_removed(void *data,
					   struct org_kde_plasma_virtual_desktop *proxy)
{
	struct virtual_desktop *vd = data;

	(void)proxy;
	virtual_desktop_destroy(vd->state, vd);
}

static const struct org_kde_plasma_virtual_desktop_listener virtual_desktop_listener = {
	.desktop_id = virtual_desktop_handle_desktop_id,
	.name = virtual_desktop_handle_name,
	.activated = virtual_desktop_handle_activated,
	.deactivated = virtual_desktop_handle_deactivated,
	.done = virtual_desktop_handle_done,
	.removed = virtual_desktop_handle_removed,
};

static void vd_manager_handle_desktop_created(
	void *data, struct org_kde_plasma_virtual_desktop_management *manager,
	const char *desktop_id, uint32_t position)
{
	struct state *st = data;
	struct virtual_desktop *vd;
	struct virtual_desktop **tail;

	(void)position;
	if (virtual_desktop_find(st, desktop_id))
		return;
	vd = xcalloc(1, sizeof *vd);
	vd->state = st;
	vd->id = xstrdup(desktop_id);
	vd->proxy = org_kde_plasma_virtual_desktop_management_get_virtual_desktop(manager, desktop_id);
	if (!vd->proxy)
		die(EXIT_RUNTIME_FAILURE, "out of memory creating org_kde_plasma_virtual_desktop");
	org_kde_plasma_virtual_desktop_add_listener(vd->proxy, &virtual_desktop_listener, vd);
	st->proxies_created++;
	for (tail = &st->virtual_desktops; *tail; tail = &(*tail)->next)
		;
	*tail = vd;
}

static void vd_manager_handle_desktop_removed(
	void *data, struct org_kde_plasma_virtual_desktop_management *manager,
	const char *desktop_id)
{
	struct state *st = data;
	struct virtual_desktop *vd = virtual_desktop_find(st, desktop_id);

	(void)manager;
	if (vd)
		virtual_desktop_destroy(st, vd);
}

static void vd_manager_handle_done(void *data,
				   struct org_kde_plasma_virtual_desktop_management *manager)
{
	(void)data;
	(void)manager;
}

static void vd_manager_handle_rows(void *data,
				   struct org_kde_plasma_virtual_desktop_management *manager,
				   uint32_t rows)
{
	(void)data;
	(void)manager;
	(void)rows;
}

static const struct org_kde_plasma_virtual_desktop_management_listener vd_manager_listener = {
	.desktop_created = vd_manager_handle_desktop_created,
	.desktop_removed = vd_manager_handle_desktop_removed,
	.done = vd_manager_handle_done,
	.rows = vd_manager_handle_rows,
};

/* ------------------------------------------------------------------------- */
/* Registry                                                                  */
/* ------------------------------------------------------------------------- */

static void pending_global_remove(struct state *st, uint32_t name)
{
	for (struct global **link = &st->pending_globals; *link; link = &(*link)->next) {
		struct global *g = *link;

		if (g->name != name)
			continue;
		*link = g->next;
		free(g->interface);
		free(g);
		return;
	}
}

static void registry_handle_global(void *data, struct wl_registry *registry, uint32_t name,
				   const char *interface, uint32_t version)
{
	struct state *st = data;

	(void)registry;
	if (!st->bound) {
		struct global *g = xcalloc(1, sizeof *g);
		struct global **tail;

		g->name = name;
		g->interface = xstrdup(interface);
		g->version = version;
		for (tail = &st->pending_globals; *tail; tail = &(*tail)->next)
			;
		*tail = g;
		return;
	}
	/* After start-up only outputs can come and go; the capabilities line
	 * was printed once and describes the protocols bound at start-up. */
	if (strcmp(interface, wl_output_interface.name) == 0)
		output_create(st, name, version);
}

static void registry_handle_global_remove(void *data, struct wl_registry *registry,
					  uint32_t name)
{
	struct state *st = data;

	(void)registry;
	if (!st->bound) {
		pending_global_remove(st, name);
		return;
	}
	for (struct output *o = st->outputs; o; o = o->next) {
		if (o->global_name != name)
			continue;
		output_destroy(st, o);
		st->outputs_dirty = true;
		return;
	}
}

static const struct wl_registry_listener registry_listener = {
	.global = registry_handle_global,
	.global_remove = registry_handle_global_remove,
};

static void bind_globals(struct state *st)
{
	const struct global *xdg_manager = NULL;
	const struct global *wlr_manager = NULL;
	const struct global *plasma_manager = NULL;
	const struct global *vd_manager = NULL;
	uint32_t lowest_output_version = 0;

	for (const struct global *g = st->pending_globals; g; g = g->next) {
		if (strcmp(g->interface, wl_output_interface.name) == 0) {
			if (lowest_output_version == 0 || g->version < lowest_output_version)
				lowest_output_version = g->version;
		} else if (strcmp(g->interface, zxdg_output_manager_v1_interface.name) == 0) {
			xdg_manager = g;
		} else if (strcmp(g->interface, zwlr_foreign_toplevel_manager_v1_interface.name) == 0) {
			wlr_manager = g;
		} else if (strcmp(g->interface, org_kde_plasma_window_management_interface.name) == 0) {
			plasma_manager = g;
		} else if (strcmp(g->interface, org_kde_plasma_virtual_desktop_management_interface.name) == 0) {
			vd_manager = g;
		} else if (strcmp(g->interface, "zwlr_layer_shell_v1") == 0) {
			st->layer_shell = true;
		} else if (strcmp(g->interface, "org_kde_plasma_shell") == 0) {
			st->plasma_shell = true;
		}
	}

	if (!xdg_manager)
		die(EXIT_MISSING_PROTOCOL, "the compositor does not provide zxdg_output_manager_v1");

	st->wl_output_version = min_u32(WL_OUTPUT_BIND_VERSION,
					lowest_output_version ? lowest_output_version
							      : WL_OUTPUT_BIND_VERSION);
	st->xdg_output_version = min_u32(XDG_OUTPUT_BIND_VERSION, xdg_manager->version);
	/* zxdg_output_v1.done is deprecated from v3 in favour of wl_output.done,
	 * which only exists from wl_output v2. */
	if (st->wl_output_version < WL_OUTPUT_DONE_SINCE_VERSION && st->xdg_output_version > 2)
		st->xdg_output_version = 2;
	if (st->wl_output_version < WL_OUTPUT_NAME_SINCE_VERSION &&
	    st->xdg_output_version < ZXDG_OUTPUT_V1_NAME_SINCE_VERSION)
		die(EXIT_MISSING_PROTOCOL,
		    "the compositor provides neither wl_output v4 nor zxdg_output_v1 v2, output names are unavailable");

	st->xdg_output_manager = wl_registry_bind(st->registry, xdg_manager->name,
						  &zxdg_output_manager_v1_interface,
						  st->xdg_output_version);
	if (!st->xdg_output_manager)
		die(EXIT_RUNTIME_FAILURE, "out of memory binding zxdg_output_manager_v1");

	for (const struct global *g = st->pending_globals; g; g = g->next) {
		if (strcmp(g->interface, wl_output_interface.name) == 0)
			output_create(st, g->name, g->version);
	}

	if (wlr_manager) {
		st->wlr_manager = wl_registry_bind(st->registry, wlr_manager->name,
						   &zwlr_foreign_toplevel_manager_v1_interface,
						   min_u32(WLR_TOPLEVEL_BIND_VERSION,
							   wlr_manager->version));
		if (!st->wlr_manager)
			die(EXIT_RUNTIME_FAILURE, "out of memory binding zwlr_foreign_toplevel_manager_v1");
		zwlr_foreign_toplevel_manager_v1_add_listener(st->wlr_manager,
							      &wlr_manager_listener, st);
		st->protocol = TOPLEVEL_WLR;
	} else if (plasma_manager) {
		st->plasma_version = min_u32(PLASMA_WM_BIND_VERSION, plasma_manager->version);
		st->plasma_manager = wl_registry_bind(st->registry, plasma_manager->name,
						      &org_kde_plasma_window_management_interface,
						      st->plasma_version);
		if (!st->plasma_manager)
			die(EXIT_RUNTIME_FAILURE, "out of memory binding org_kde_plasma_window_management");
		org_kde_plasma_window_management_add_listener(st->plasma_manager,
							      &plasma_manager_listener, st);
		st->protocol = TOPLEVEL_PLASMA;
	} else {
		st->protocol = TOPLEVEL_NONE;
	}

	/* Virtual desktop membership only means something for Plasma windows. */
	if (st->protocol == TOPLEVEL_PLASMA && vd_manager) {
		st->vd_manager = wl_registry_bind(st->registry, vd_manager->name,
						  &org_kde_plasma_virtual_desktop_management_interface,
						  min_u32(PLASMA_VD_BIND_VERSION, vd_manager->version));
		if (!st->vd_manager)
			die(EXIT_RUNTIME_FAILURE, "out of memory binding org_kde_plasma_virtual_desktop_management");
		org_kde_plasma_virtual_desktop_management_add_listener(st->vd_manager,
								       &vd_manager_listener, st);
	}

	st->bound = true;
	while (st->pending_globals)
		pending_global_remove(st, st->pending_globals->name);
}

static void emit_capabilities(const struct state *st)
{
	static const char *const protocol_names[] = { "none", "wlr", "plasma" };

	printf("{\"event\":\"capabilities\",\"layer_shell\":%s,\"plasma_shell\":%s,"
	       "\"toplevel_protocol\":\"%s\"}\n",
	       st->layer_shell ? "true" : "false", st->plasma_shell ? "true" : "false",
	       protocol_names[st->protocol]);
	flush_line();
}

/* Roundtrips until a roundtrip creates no new objects, so that every bound
 * output, xdg_output and window has delivered its initial events. */
static void settle(struct state *st)
{
	do {
		st->proxies_created = 0;
		if (wl_display_roundtrip(st->display) < 0)
			die_display(st);
	} while (st->proxies_created > 0);
}

static void emit_pending(struct state *st)
{
	if (st->outputs_dirty) {
		emit_outputs(st);
		/* Plasma window output membership is derived from output rectangles. */
		if (st->protocol == TOPLEVEL_PLASMA)
			st->toplevels_dirty = true;
	}
	if (st->toplevels_dirty && st->protocol != TOPLEVEL_NONE)
		emit_toplevels(st);
	st->toplevels_dirty = false;
}

/* ------------------------------------------------------------------------- */
/* Main loop                                                                 */
/* ------------------------------------------------------------------------- */

static void setup_signal_fd(struct state *st)
{
	sigset_t mask;

	sigemptyset(&mask);
	sigaddset(&mask, SIGTERM);
	sigaddset(&mask, SIGINT);
	if (sigprocmask(SIG_BLOCK, &mask, NULL) < 0)
		die(EXIT_RUNTIME_FAILURE, "sigprocmask failed: %s", strerror(errno));
	st->signal_fd = signalfd(-1, &mask, SFD_CLOEXEC | SFD_NONBLOCK);
	if (st->signal_fd < 0)
		die(EXIT_RUNTIME_FAILURE, "signalfd failed: %s", strerror(errno));
}

/* Returns true when stdin reached EOF (or was closed) so the process must stop. */
static bool stdin_finished(void)
{
	char buf[256];
	ssize_t n = read(STDIN_FILENO, buf, sizeof buf);

	if (n > 0)
		return false; /* the monitor accepts no commands; input is discarded */
	if (n < 0 && (errno == EINTR || errno == EAGAIN))
		return false;
	return true;
}

static void run_loop(struct state *st)
{
	int display_fd = wl_display_get_fd(st->display);

	for (;;) {
		struct pollfd fds[3];
		bool need_flush;
		int ret;

		while (wl_display_prepare_read(st->display) != 0) {
			if (wl_display_dispatch_pending(st->display) < 0)
				die_display(st);
		}
		emit_pending(st);

		ret = wl_display_flush(st->display);
		need_flush = ret < 0 && errno == EAGAIN;
		if (ret < 0 && !need_flush) {
			wl_display_cancel_read(st->display);
			die_display(st);
		}

		fds[0].fd = display_fd;
		fds[0].events = POLLIN | (need_flush ? POLLOUT : 0);
		fds[0].revents = 0;
		fds[1].fd = STDIN_FILENO;
		fds[1].events = POLLIN;
		fds[1].revents = 0;
		fds[2].fd = st->signal_fd;
		fds[2].events = POLLIN;
		fds[2].revents = 0;

		ret = poll(fds, 3, -1);
		if (ret < 0) {
			wl_display_cancel_read(st->display);
			if (errno == EINTR)
				continue;
			die(EXIT_RUNTIME_FAILURE, "poll failed: %s", strerror(errno));
		}

		if (fds[2].revents != 0) {
			wl_display_cancel_read(st->display);
			return; /* SIGTERM or SIGINT */
		}
		if (fds[1].revents != 0 && stdin_finished()) {
			wl_display_cancel_read(st->display);
			return;
		}

		if (fds[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
			wl_display_cancel_read(st->display);
			die(EXIT_RUNTIME_FAILURE, "the Wayland display connection was closed");
		}
		if (fds[0].revents & POLLIN) {
			if (wl_display_read_events(st->display) < 0)
				die_display(st);
		} else {
			wl_display_cancel_read(st->display);
		}
		if (wl_display_dispatch_pending(st->display) < 0)
			die_display(st);
		emit_pending(st);
	}
}

static void state_finish(struct state *st)
{
	while (st->toplevels)
		toplevel_destroy(st, st->toplevels);
	while (st->virtual_desktops)
		virtual_desktop_destroy(st, st->virtual_desktops);
	while (st->outputs)
		output_destroy(st, st->outputs);
	if (st->vd_manager)
		org_kde_plasma_virtual_desktop_management_destroy(st->vd_manager);
	if (st->wlr_manager)
		zwlr_foreign_toplevel_manager_v1_destroy(st->wlr_manager);
	if (st->plasma_manager)
		org_kde_plasma_window_management_destroy(st->plasma_manager);
	if (st->xdg_output_manager)
		zxdg_output_manager_v1_destroy(st->xdg_output_manager);
	wl_registry_destroy(st->registry);
	wl_display_flush(st->display);
	wl_display_disconnect(st->display);
	if (st->signal_fd >= 0)
		close(st->signal_fd);
}

static void usage(FILE *out)
{
	fputs("usage: lively-wl-monitor [--once]\n", out);
}

int main(int argc, char **argv)
{
	struct state st = { 0 };

	st.signal_fd = -1;
	for (int i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--once") == 0) {
			st.once = true;
		} else if (strcmp(argv[i], "--help") == 0) {
			usage(stdout);
			return 0;
		} else {
			fprintf(stderr, "lively-wl-monitor: unknown argument '%s'\n", argv[i]);
			usage(stderr);
			return EXIT_BAD_ARGUMENTS;
		}
	}

	signal(SIGPIPE, SIG_IGN);

	st.display = wl_display_connect(NULL);
	if (!st.display)
		die(EXIT_RUNTIME_FAILURE, "cannot connect to the Wayland display: %s", strerror(errno));
	st.registry = wl_display_get_registry(st.display);
	if (!st.registry)
		die(EXIT_RUNTIME_FAILURE, "out of memory creating wl_registry");
	wl_registry_add_listener(st.registry, &registry_listener, &st);
	if (wl_display_roundtrip(st.display) < 0)
		die_display(&st);

	bind_globals(&st);
	emit_capabilities(&st);
	settle(&st);
	emit_outputs(&st);
	if (st.protocol != TOPLEVEL_NONE)
		emit_toplevels(&st);

	if (!st.once) {
		setup_signal_fd(&st);
		run_loop(&st);
	}

	state_finish(&st);
	return 0;
}
