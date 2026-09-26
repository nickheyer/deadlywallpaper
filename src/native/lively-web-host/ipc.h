#ifndef LWH_IPC_H
#define LWH_IPC_H

#include <glib.h>

/* MessageType values from Lively.Models/Message/MessageType.cs plus the Linux additions. */
typedef enum {
    LWH_MSG_HWND = 0,
    LWH_MSG_CONSOLE = 1,
    LWH_MSG_WPLOADED = 2,
    LWH_MSG_SCREENSHOT = 3,
    LWH_CMD_RELOAD = 4,
    LWH_CMD_CLOSE = 5,
    LWH_CMD_SCREENSHOT = 6,
    LWH_CMD_SUSPEND = 7,
    LWH_CMD_RESUME = 8,
    LWH_CMD_VOLUME = 9,
    LWH_LSP_PERFCNTR = 10,
    LWH_LSP_NOWPLAYING = 11,
    LWH_LP_SLIDER = 12,
    LWH_LP_TEXTBOX = 13,
    LWH_LP_DROPDOWN = 14,
    LWH_LP_FDROPDOWN = 15,
    LWH_LP_BUTTON = 16,
    LWH_LP_CPICKER = 17,
    LWH_LP_CHECKBOX = 18,
    LWH_LP_DROPDOWN_SCALER = 19,
    LWH_LSP_AUDIO = 20,
    LWH_HOST_MPV_COMMAND = 100
} LwhMessageType;

typedef enum {
    LWH_CONSOLE_LOG = 0,
    LWH_CONSOLE_ERROR = 1,
    LWH_CONSOLE_CONSOLE = 2
} LwhConsoleCategory;

/* Takes ownership of the process stdout for protocol output and points fd 1 at
 * stderr so child processes and libraries cannot corrupt the message stream. */
void lwh_ipc_init(void);

void lwh_ipc_send_hwnd(void);
void lwh_ipc_send_console(LwhConsoleCategory category, const gchar *format, ...) G_GNUC_PRINTF(2, 3);
void lwh_ipc_send_wploaded(gboolean success);
void lwh_ipc_send_screenshot(const gchar *file_name, gboolean success);

#endif
