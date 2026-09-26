/* Unit test for the C port of StreamUtil (YouTube id / Shadertoy embed parsing). */
#include <stdio.h>
#include <string.h>

#include "../stream_util.h"

static int failures = 0;

static void expect_youtube(const char *url, const char *expected_id)
{
    gchar *id = NULL;
    gboolean ok = lwh_stream_try_parse_youtube_id(url, &id);

    if (expected_id == NULL) {
        if (ok) {
            printf("FAIL: %s parsed as youtube id %s, expected no match\n", url, id);
            failures++;
        } else {
            printf("ok:   %s -> no youtube id\n", url);
        }
    } else if (!ok || strcmp(id, expected_id) != 0) {
        printf("FAIL: %s -> %s, expected %s\n", url, ok ? id : "(no match)", expected_id);
        failures++;
    } else {
        printf("ok:   %s -> %s\n", url, id);
    }
    g_free(id);
}

static void expect_shadertoy(const char *url, const char *expected_src)
{
    gchar *html = NULL;
    gboolean ok = lwh_stream_try_parse_shadertoy(url, &html);

    if (expected_src == NULL) {
        if (ok) {
            printf("FAIL: %s treated as shadertoy\n", url);
            failures++;
        } else {
            printf("ok:   %s -> not shadertoy\n", url);
        }
    } else if (!ok || strstr(html, expected_src) == NULL) {
        printf("FAIL: %s -> %s\n", url, ok ? html : "(no match)");
        failures++;
    } else {
        printf("ok:   %s -> embed %s\n", url, expected_src);
    }
    g_free(html);
}

int main(void)
{
    gchar *embed;

    expect_youtube("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://youtube.com/watch?v=dQw4w9WgXcQ&t=10", "dQw4w9WgXcQ");
    expect_youtube("https://youtu.be/dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://youtu.be/dQw4w9WgXcQ/", "dQw4w9WgXcQ");
    expect_youtube("youtube.com/watch?v=dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://www.youtube.com/v/dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://www.youtube.com/watch/dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://www.youtube.com/v=dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://www.youtube.com/attribution_link?u=/watch?v=dQw4w9WgXcQ", "dQw4w9WgXcQ");
    expect_youtube("https://www.youtube.com/embed/dQw4w9WgXcQ", NULL);
    expect_youtube("https://www.youtube.com/shorts/dQw4w9WgXcQ", NULL);
    expect_youtube("https://www.youtube.com/watch?v=short", NULL);
    expect_youtube("https://www.youtube.com/", NULL);
    expect_youtube("https://vimeo.com/dQw4w9WgXcQ", NULL);
    expect_youtube("https://www.shadertoy.com/view/Ms2SD1", NULL);

    expect_shadertoy("https://www.shadertoy.com/view/Ms2SD1",
                     "src=https://www.shadertoy.com/embed/Ms2SD1?gui=false&t=10&paused=false&muted=true\"");
    expect_shadertoy("https://www.shadertoy.com/view/Xds3zN", "https://www.shadertoy.com/embed/Xds3zN?gui=false");
    expect_shadertoy("https://www.shadertoy.com/", NULL);
    expect_shadertoy("https://example.com/view/Ms2SD1", NULL);

    embed = lwh_stream_youtube_embed_url("dQw4w9WgXcQ");
    if (strcmp(embed, "https://www.youtube.com/embed/dQw4w9WgXcQ?version=3&rel=0&autoplay=1&loop=1&controls=0&playlist=dQw4w9WgXcQ") != 0) {
        printf("FAIL: embed url %s\n", embed);
        failures++;
    } else {
        printf("ok:   embed url %s\n", embed);
    }
    g_free(embed);

    {
        static const struct { const char *url; const char *origin; } origins[] = {
            { "https://www.shadertoy.com/view/Ms2SD1", "https://www.shadertoy.com/" },
            { "shadertoy.com/view/Ms2SD1", "https://shadertoy.com/" },
            { "http://example.com:8080/x", "http://example.com/" },
            { "not a url", NULL },
        };
        for (gsize i = 0; i < G_N_ELEMENTS(origins); i++) {
            gchar *origin = lwh_stream_url_origin(origins[i].url);

            if (g_strcmp0(origin, origins[i].origin) != 0) {
                printf("FAIL: origin of %s -> %s, expected %s\n", origins[i].url,
                       origin != NULL ? origin : "(null)", origins[i].origin != NULL ? origins[i].origin : "(null)");
                failures++;
            } else {
                printf("ok:   origin of %s -> %s\n", origins[i].url, origin != NULL ? origin : "(null)");
            }
            g_free(origin);
        }
    }

    printf("%d failure(s)\n", failures);
    return failures == 0 ? 0 : 1;
}
