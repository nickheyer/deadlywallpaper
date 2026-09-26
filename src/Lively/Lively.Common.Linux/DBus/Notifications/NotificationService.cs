using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.Notifications
{
    /// <summary>
    /// Sends desktop notifications through the session bus notification daemon (<c>org.freedesktop.Notifications</c>).
    /// </summary>
    public sealed class NotificationService
    {
        public const string BusName = "org.freedesktop.Notifications";
        public static readonly ObjectPath ObjectPath = new("/org/freedesktop/Notifications");
        public const string DefaultAppName = "Lively Wallpaper";
        public const string DefaultDesktopEntry = "lively-wallpaper";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Connection connection;
        private readonly string appName;
        private readonly string desktopEntry;

        public NotificationService()
            : this(DBusConnections.Session, DefaultAppName, DefaultDesktopEntry)
        {
        }

        /// <param name="connection">Session bus connection used for the <c>Notify</c> call.</param>
        /// <param name="appName">Value of <c>app_name</c>.</param>
        /// <param name="desktopEntry">Desktop file id sent in the <c>desktop-entry</c> hint so the daemon can associate the notification with the application.</param>
        public NotificationService(Connection connection, string appName, string desktopEntry)
        {
            this.connection = connection ?? throw new ArgumentNullException(nameof(connection));
            if (string.IsNullOrWhiteSpace(appName))
                throw new ArgumentException("The application name must not be empty.", nameof(appName));
            if (string.IsNullOrWhiteSpace(desktopEntry))
                throw new ArgumentException("The desktop entry id must not be empty.", nameof(desktopEntry));
            this.appName = appName;
            this.desktopEntry = desktopEntry;
        }

        /// <summary>
        /// Shows a notification and returns the id the daemon assigned to it.
        /// </summary>
        /// <param name="title">Notification summary.</param>
        /// <param name="body">Notification body.</param>
        /// <param name="timeoutMs">Expire timeout in milliseconds; <c>-1</c> lets the daemon decide and <c>0</c> never expires.</param>
        /// <param name="iconPath">Icon name or absolute image path for <c>app_icon</c>; empty when <c>null</c>.</param>
        public async Task<uint> ShowAsync(string title, string body, int timeoutMs, string iconPath = null)
        {
            ArgumentNullException.ThrowIfNull(title);
            var notifications = connection.CreateProxy<INotifications>(BusName, ObjectPath);
            var hints = new Dictionary<string, object>
            {
                ["desktop-entry"] = desktopEntry,
            };
            var id = await notifications.NotifyAsync(appName, 0, iconPath ?? string.Empty, title, body ?? string.Empty, Array.Empty<string>(), hints, timeoutMs);
            Logger.Debug("Notification {0} shown: {1}", id, title);
            return id;
        }
    }
}
