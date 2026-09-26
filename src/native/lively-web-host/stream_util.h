#ifndef LWH_STREAM_UTIL_H
#define LWH_STREAM_UTIL_H

#include <glib.h>

/* C port of Lively.Common.Helpers.StreamUtil.TryParseShadertoy: on success *html
 * receives the wrapper page (free with g_free) that embeds the shader. */
gboolean lwh_stream_try_parse_shadertoy(const gchar *url, gchar **html);

/* C port of StreamUtil.TryParseYouTubeVideoIdFromUrl: on success *id receives
 * the 11 character video id (free with g_free). */
gboolean lwh_stream_try_parse_youtube_id(const gchar *url, gchar **id);

/* "scheme://host/" of url (https:// assumed when the scheme is missing), or
 * NULL when url has no host. Used as the base URI of the Shadertoy wrapper so
 * the embed iframe is same-origin with shadertoy.com. */
gchar *lwh_stream_url_origin(const gchar *url);

/* The embed player URL Form1.cs navigates to for a video id. */
gchar *lwh_stream_youtube_embed_url(const gchar *id);

#endif
