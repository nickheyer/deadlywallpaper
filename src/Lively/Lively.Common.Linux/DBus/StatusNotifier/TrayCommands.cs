using Lively.Models.Services;
using System;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// The actions and state queries the tray icon needs from the application. Every delegate is invoked on a
    /// D-Bus worker thread; delegates that touch UI must marshal to the UI thread themselves.
    /// </summary>
    public sealed class TrayCommands
    {
        public Action OpenApp { get; set; }
        public Action CloseWallpapers { get; set; }
        public Action TogglePause { get; set; }
        public Action ChangeWallpaper { get; set; }
        public Action CustomiseWallpaper { get; set; }
        public Action ShowUpdatePage { get; set; }
        public Action ReportBug { get; set; }
        public Action Exit { get; set; }

        /// <summary>Whether wallpaper playback is currently paused (drives the pause item's checkmark).</summary>
        public Func<bool> IsPaused { get; set; }

        /// <summary>Whether a customisable wallpaper is running (enables the customise item).</summary>
        public Func<bool> CanCustomise { get; set; }

        /// <summary>Current update status (label and enabled state of the update item).</summary>
        public Func<AppUpdateStatus> UpdateStatus { get; set; }

        /// <summary>Whether the update item (and its separator) is part of the menu at all.</summary>
        public bool ShowUpdateItem { get; set; }

        /// <summary>Localized string lookup by resource key (TextOpenLively, TextCloseWallpapers, ...).</summary>
        public Func<string, string> GetString { get; set; }
    }
}
