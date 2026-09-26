/* JSON line protocol: stdout emitter and stdin reader/dispatcher (PROTOCOL.md section 2). */
#include "app.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static void emit(struct app *a, cJSON *obj)
{
    if (!obj) {
        app_error(a, "out of memory building a message");
        return;
    }
    char *text = cJSON_PrintUnformatted(obj);
    cJSON_Delete(obj);
    if (!text) {
        app_error(a, "out of memory serialising a message");
        return;
    }
    if (fputs(text, stdout) == EOF || fputc('\n', stdout) == EOF || fflush(stdout) == EOF) {
        app_error(a, "stdout write failed (%s), core is gone", strerror(errno));
        free(text);
        app_quit(a, HOST_EXIT_OK);
        return;
    }
    app_log(a, "-> %s", text);
    free(text);
}

static cJSON *message(int type)
{
    cJSON *obj = cJSON_CreateObject();
    if (!obj)
        return NULL;
    if (!cJSON_AddNumberToObject(obj, "Type", type)) {
        cJSON_Delete(obj);
        return NULL;
    }
    return obj;
}

void ipc_send_hwnd(struct app *a)
{
    cJSON *obj = message(MSG_HWND);
    if (obj && !cJSON_AddNumberToObject(obj, "Hwnd", 0)) {
        cJSON_Delete(obj);
        obj = NULL;
    }
    emit(a, obj);
}

void ipc_send_console(struct app *a, const char *text, int category)
{
    cJSON *obj = message(MSG_CONSOLE);
    if (obj && (!cJSON_AddStringToObject(obj, "Message", text) || !cJSON_AddNumberToObject(obj, "Category", category))) {
        cJSON_Delete(obj);
        obj = NULL;
    }
    emit(a, obj);
}

void ipc_send_wploaded(struct app *a, bool success)
{
    cJSON *obj = message(MSG_WPLOADED);
    if (obj && !cJSON_AddBoolToObject(obj, "Success", success)) {
        cJSON_Delete(obj);
        obj = NULL;
    }
    emit(a, obj);
}

void ipc_send_screenshot(struct app *a, const char *path, bool success)
{
    const char *slash = strrchr(path, '/');
    const char *basename = slash ? slash + 1 : path;
    cJSON *obj = message(MSG_SCREENSHOT);
    if (obj && (!cJSON_AddStringToObject(obj, "FileName", basename) || !cJSON_AddBoolToObject(obj, "Success", success))) {
        cJSON_Delete(obj);
        obj = NULL;
    }
    emit(a, obj);
}

/* ---- stdin ---- */

static const char *field_string(const cJSON *root, const char *name)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(root, name);
    return cJSON_IsString(item) ? item->valuestring : NULL;
}

static bool field_number(const cJSON *root, const char *name, double *out)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(root, name);
    if (!cJSON_IsNumber(item))
        return false;
    *out = item->valuedouble;
    return true;
}

static bool field_bool(const cJSON *root, const char *name, bool *out)
{
    const cJSON *item = cJSON_GetObjectItemCaseSensitive(root, name);
    if (!cJSON_IsBool(item))
        return false;
    *out = cJSON_IsTrue(item);
    return true;
}

void ipc_handle_line(struct app *a, const char *line)
{
    while (*line == ' ' || *line == '\t' || *line == '\r')
        line++;
    if (*line == '\0')
        return;
    app_log(a, "<- %s", line);
    cJSON *root = cJSON_Parse(line);
    if (!root || !cJSON_IsObject(root)) {
        app_error(a, "ignoring malformed JSON line: %s", line);
        cJSON_Delete(root);
        return;
    }
    double type_value = -1;
    if (!field_number(root, "Type", &type_value)) {
        app_error(a, "ignoring message without a numeric Type: %s", line);
        cJSON_Delete(root);
        return;
    }
    int type = (int)type_value;
    switch (type) {
    case CMD_RELOAD:
        player_reload(a);
        break;
    case CMD_CLOSE:
        app_log(a, "cmd_close");
        app_quit(a, HOST_EXIT_OK);
        break;
    case CMD_SCREENSHOT: {
        double format = 0;
        const char *path = field_string(root, "FilePath");
        if (!path || !field_number(root, "Format", &format) || format < SHOT_JPEG || format > SHOT_BMP) {
            app_error(a, "cmd_screenshot needs FilePath and Format 0-3: %s", line);
            if (path)
                ipc_send_screenshot(a, path, false);
            break;
        }
        player_screenshot(a, path, (enum shot_format)(int)format);
        break;
    }
    case CMD_SUSPEND:
        player_set_pause(a, true);
        break;
    case CMD_RESUME:
        player_set_pause(a, false);
        break;
    case CMD_VOLUME: {
        double volume = 0;
        if (!field_number(root, "Volume", &volume)) {
            app_error(a, "cmd_volume needs Volume: %s", line);
            break;
        }
        player_set_volume(a, (int)volume);
        break;
    }
    case LP_SLIDER: {
        double value = 0, step = 0;
        const char *name = field_string(root, "Name");
        if (!name || !field_number(root, "Value", &value)) {
            app_error(a, "lp_slider needs Name and Value: %s", line);
            break;
        }
        if (!field_number(root, "Step", &step))
            step = 0;
        player_set_slider(a, name, value, step);
        break;
    }
    case LP_CHECKBOX: {
        bool value = false;
        const char *name = field_string(root, "Name");
        if (!name || !field_bool(root, "Value", &value)) {
            app_error(a, "lp_chekbox needs Name and boolean Value: %s", line);
            break;
        }
        player_set_checkbox(a, name, value);
        break;
    }
    case LP_BUTTON: {
        bool is_default = false;
        if (!field_bool(root, "IsDefault", &is_default)) {
            app_error(a, "lp_button needs IsDefault: %s", line);
            break;
        }
        if (is_default) {
            if (player_apply_properties(a) < 0)
                app_error(a, "restoring default properties failed");
        } else {
            app_log(a, "lp_button without IsDefault has no mpv action");
        }
        break;
    }
    case LP_DROPDOWN_SCALER: {
        double value = 0;
        if (!field_number(root, "Value", &value) || value < SCALER_NONE || value > SCALER_UNIFORM_FILL) {
            app_error(a, "lp_dropdown_scaler needs Value 0-3: %s", line);
            break;
        }
        player_set_scaler(a, (enum scaler)(int)value);
        break;
    }
    case HOST_MPV_COMMAND: {
        const cJSON *command = cJSON_GetObjectItemCaseSensitive(root, "Command");
        if (!command) {
            app_error(a, "host_mpv_command needs Command: %s", line);
            break;
        }
        player_run_command(a, command);
        break;
    }
    case LSP_PERFCNTR:
    case LSP_NOWPLAYING:
    case LSP_AUDIO:
    case LP_TEXTBOX:
    case LP_DROPDOWN:
    case LP_FDROPDOWN:
    case LP_CPICKER:
        /* Web-wallpaper only messages; the Windows mpv player ignores these as well. */
        app_log(a, "message type %d does not apply to the mpv host, ignored", type);
        break;
    default:
        app_log(a, "unknown message type %d ignored", type);
        break;
    }
    cJSON_Delete(root);
}

int ipc_read_stdin(struct app *a)
{
    if (a->stdin_eof)
        return 0;
    if (a->inlen + 4096 + 1 > a->incap) {
        size_t ncap = a->incap ? a->incap * 2 : 8192;
        while (ncap < a->inlen + 4096 + 1)
            ncap *= 2;
        char *nbuf = realloc(a->inbuf, ncap);
        if (!nbuf) {
            app_error(a, "out of memory growing the stdin buffer");
            app_quit(a, HOST_EXIT_OK);
            return -1;
        }
        a->inbuf = nbuf;
        a->incap = ncap;
    }
    ssize_t n = read(STDIN_FILENO, a->inbuf + a->inlen, a->incap - a->inlen - 1);
    if (n < 0) {
        if (errno == EINTR || errno == EAGAIN)
            return 0;
        app_error(a, "stdin read failed: %s", strerror(errno));
        a->stdin_eof = true;
        app_quit(a, HOST_EXIT_OK);
        return -1;
    }
    if (n == 0) {
        a->stdin_eof = true;
        if (a->inlen > 0) {
            a->inbuf[a->inlen] = '\0';
            ipc_handle_line(a, a->inbuf);
            a->inlen = 0;
        }
        app_log(a, "stdin closed, exiting");
        app_quit(a, HOST_EXIT_OK);
        return 0;
    }
    a->inlen += (size_t)n;
    for (;;) {
        char *newline = memchr(a->inbuf, '\n', a->inlen);
        if (!newline)
            break;
        *newline = '\0';
        size_t consumed = (size_t)(newline - a->inbuf) + 1;
        ipc_handle_line(a, a->inbuf);
        memmove(a->inbuf, a->inbuf + consumed, a->inlen - consumed);
        a->inlen -= consumed;
        if (!a->running)
            break;
    }
    return 0;
}
