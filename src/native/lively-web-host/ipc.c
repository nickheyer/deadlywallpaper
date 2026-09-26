#include "ipc.h"

#include <json-glib/json-glib.h>
#include <stdio.h>
#include <unistd.h>

static FILE *proto_out = NULL;

void lwh_ipc_init(void)
{
    int fd = dup(STDOUT_FILENO);

    if (fd < 0) {
        proto_out = stdout;
        return;
    }
    if (dup2(STDERR_FILENO, STDOUT_FILENO) < 0) {
        close(fd);
        proto_out = stdout;
        return;
    }
    proto_out = fdopen(fd, "w");
    if (proto_out == NULL) {
        close(fd);
        proto_out = stdout;
    }
}

static void send_object(JsonBuilder *builder)
{
    JsonGenerator *gen = json_generator_new();
    JsonNode *root = json_builder_get_root(builder);
    gchar *text;

    json_generator_set_pretty(gen, FALSE);
    json_generator_set_root(gen, root);
    text = json_generator_to_data(gen, NULL);
    if (proto_out == NULL)
        proto_out = stdout;
    fputs(text, proto_out);
    fputc('\n', proto_out);
    fflush(proto_out);
    g_free(text);
    json_node_unref(root);
    g_object_unref(gen);
    g_object_unref(builder);
}

static JsonBuilder *begin_message(LwhMessageType type)
{
    JsonBuilder *b = json_builder_new();

    json_builder_begin_object(b);
    json_builder_set_member_name(b, "Type");
    json_builder_add_int_value(b, type);
    return b;
}

void lwh_ipc_send_hwnd(void)
{
    JsonBuilder *b = begin_message(LWH_MSG_HWND);

    json_builder_set_member_name(b, "Hwnd");
    json_builder_add_int_value(b, 0);
    json_builder_end_object(b);
    send_object(b);
}

void lwh_ipc_send_console(LwhConsoleCategory category, const gchar *format, ...)
{
    JsonBuilder *b = begin_message(LWH_MSG_CONSOLE);
    va_list ap;
    gchar *message;

    va_start(ap, format);
    message = g_strdup_vprintf(format, ap);
    va_end(ap);

    json_builder_set_member_name(b, "Message");
    json_builder_add_string_value(b, message);
    json_builder_set_member_name(b, "Category");
    json_builder_add_int_value(b, category);
    json_builder_end_object(b);
    send_object(b);
    g_free(message);
}

void lwh_ipc_send_wploaded(gboolean success)
{
    JsonBuilder *b = begin_message(LWH_MSG_WPLOADED);

    json_builder_set_member_name(b, "Success");
    json_builder_add_boolean_value(b, success);
    json_builder_end_object(b);
    send_object(b);
}

void lwh_ipc_send_screenshot(const gchar *file_name, gboolean success)
{
    JsonBuilder *b = begin_message(LWH_MSG_SCREENSHOT);

    json_builder_set_member_name(b, "FileName");
    if (file_name != NULL)
        json_builder_add_string_value(b, file_name);
    else
        json_builder_add_null_value(b);
    json_builder_set_member_name(b, "Success");
    json_builder_add_boolean_value(b, success);
    json_builder_end_object(b);
    send_object(b);
}
