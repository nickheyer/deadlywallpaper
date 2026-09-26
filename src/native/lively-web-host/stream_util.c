#include "stream_util.h"

#include <string.h>

static const gchar *const youtube_hosts[] = {
    "www.youtube.com", "youtube.com", "youtu.be", "www.youtu.be"
};

/* LinkUtil.TrySanitizeUrl: a non-blank address that parses as a URI, either as
 * given or once an https:// scheme is prepended. */
static gboolean sanitize_url(const gchar *address)
{
    gchar *stripped = g_strstrip(g_strdup(address));
    gboolean ok = *stripped != '\0';

    if (ok && !g_uri_is_valid(stripped, G_URI_FLAGS_NONE, NULL)) {
        gchar *with_scheme = g_strconcat("https://", stripped, NULL);
        ok = g_uri_is_valid(with_scheme, G_URI_FLAGS_NONE, NULL);
        g_free(with_scheme);
    }
    g_free(stripped);
    return ok;
}

static gchar *replace_all(const gchar *text, const gchar *needle, const gchar *replacement)
{
    gchar **parts = g_strsplit(text, needle, -1);
    gchar *result = g_strjoinv(replacement, parts);

    g_strfreev(parts);
    return result;
}

gboolean lwh_stream_try_parse_shadertoy(const gchar *url, gchar **html)
{
    gchar *embed_url;

    if (url == NULL || strstr(url, "shadertoy.com/view") == NULL)
        return FALSE;
    if (!sanitize_url(url))
        return FALSE;

    embed_url = replace_all(url, "view/", "embed/");
    *html = g_strconcat(
        "<!DOCTYPE html><html lang=\"en\" dir=\"ltr\"> <head> <meta charset=\"utf - 8\"> \n"
        "                    <title>Digital Brain</title> <style media=\"screen\"> iframe { position: fixed; width: 100%; height: 100%; top: 0; right: 0; bottom: 0;\n"
        "                    left: 0; z-index; -1; pointer-events: none;  } </style> </head> <body> <iframe width=\"640\" height=\"360\" frameborder=\"0\" \n"
        "                    src=", embed_url, "?gui=false&t=10&paused=false&muted=true\"></iframe> </body></html>",
        NULL);
    g_free(embed_url);
    return TRUE;
}

/* System.Uri.Segments: "/a/b/c" -> ["/", "a/", "b/", "c"]. */
static gchar **uri_segments(const gchar *path)
{
    GPtrArray *segments = g_ptr_array_new();
    gsize start = 0;
    gsize len;

    if (path == NULL || *path == '\0')
        path = "/";
    len = strlen(path);
    for (gsize i = 0; i < len; i++) {
        if (path[i] == '/') {
            g_ptr_array_add(segments, g_strndup(path + start, i + 1 - start));
            start = i + 1;
        }
    }
    if (start < len)
        g_ptr_array_add(segments, g_strdup(path + start));
    g_ptr_array_add(segments, NULL);
    return (gchar **)g_ptr_array_free(segments, FALSE);
}

static gboolean regex_capture(const gchar *pattern, const gchar *text, gint group, gchar **out)
{
    GRegex *re = g_regex_new(pattern, 0, 0, NULL);
    GMatchInfo *info = NULL;
    gboolean matched;

    if (re == NULL)
        return FALSE;
    matched = g_regex_match(re, text, 0, &info);
    if (matched)
        *out = g_match_info_fetch(info, group);
    g_match_info_free(info);
    g_regex_unref(re);
    return matched && *out != NULL && **out != '\0';
}

gboolean lwh_stream_try_parse_youtube_id(const gchar *url, gchar **id)
{
    GUri *uri;
    gchar *host;
    gboolean known_host = FALSE;
    gboolean result = FALSE;
    const gchar *query;
    GHashTable *params = NULL;

    if (url == NULL)
        return FALSE;
    uri = g_uri_parse(url, G_URI_FLAGS_NONE, NULL);
    if (uri == NULL) {
        gchar *with_scheme = g_strconcat("http://", url, NULL);
        uri = g_uri_parse(with_scheme, G_URI_FLAGS_NONE, NULL);
        g_free(with_scheme);
        if (uri == NULL)
            return FALSE;
    }

    host = g_uri_get_host(uri) != NULL ? g_ascii_strdown(g_uri_get_host(uri), -1) : g_strdup("");
    for (gsize i = 0; i < G_N_ELEMENTS(youtube_hosts); i++)
        known_host = known_host || strcmp(host, youtube_hosts[i]) == 0;
    g_free(host);
    if (!known_host) {
        g_uri_unref(uri);
        return FALSE;
    }

    query = g_uri_get_query(uri);
    if (query != NULL)
        params = g_uri_parse_params(query, -1, "&", G_URI_PARAMS_WWW_FORM, NULL);

    if (params != NULL && g_hash_table_contains(params, "v")) {
        const gchar *v = g_hash_table_lookup(params, "v");
        result = v != NULL && regex_capture("^[a-zA-Z0-9_-]{11}$", v, 0, id);
    } else if (params != NULL && g_hash_table_contains(params, "u")) {
        const gchar *u = g_hash_table_lookup(params, "u");
        result = u != NULL && regex_capture("/watch\\?v=([a-zA-Z0-9_-]{11})", u, 1, id);
    } else {
        gchar **segments = uri_segments(g_uri_get_path(uri));
        guint count = g_strv_length(segments);
        gchar *last = count > 0 ? replace_all(segments[count - 1], "/", "") : g_strdup("");

        if (regex_capture("^v=[a-zA-Z0-9_-]{11}$", last, 0, id)) {
            gchar *stripped = g_strdup(*id + 2);
            g_free(*id);
            *id = stripped;
            result = TRUE;
        } else if (count > 2 && strcmp(segments[count - 2], "v/") != 0 && strcmp(segments[count - 2], "watch/") != 0) {
            result = FALSE;
        } else {
            result = regex_capture("^[a-zA-Z0-9_-]{11}$", last, 0, id);
        }
        g_free(last);
        g_strfreev(segments);
    }

    if (params != NULL)
        g_hash_table_unref(params);
    g_uri_unref(uri);
    return result;
}

gchar *lwh_stream_url_origin(const gchar *url)
{
    GUri *uri;
    gchar *origin = NULL;

    if (url == NULL)
        return NULL;
    uri = g_uri_parse(url, G_URI_FLAGS_NONE, NULL);
    if (uri == NULL) {
        gchar *with_scheme = g_strconcat("https://", url, NULL);

        uri = g_uri_parse(with_scheme, G_URI_FLAGS_NONE, NULL);
        g_free(with_scheme);
    }
    if (uri == NULL)
        return NULL;
    if (g_uri_get_host(uri) != NULL &&
        g_regex_match_simple("^[A-Za-z0-9.:_\\[\\]-]+$", g_uri_get_host(uri), 0, 0))
        origin = g_strdup_printf("%s://%s/", g_uri_get_scheme(uri), g_uri_get_host(uri));
    g_uri_unref(uri);
    return origin;
}

gchar *lwh_stream_youtube_embed_url(const gchar *id)
{
    return g_strdup_printf("https://www.youtube.com/embed/%s?version=3&rel=0&autoplay=1&loop=1&controls=0&playlist=%s", id, id);
}
