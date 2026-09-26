using Lively.Common.Linux.DBus.Notifications;
using Lively.Common.Services;
using Lively.Models.Enums;
using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// The Lively tray icon as a StatusNotifierItem with a dbusmenu context menu.
    /// Construct it, then call <see cref="StartAsync"/> to export the objects, own the bus name and register with the
    /// StatusNotifierWatcher; registration is repeated whenever a watcher (re)appears on the bus.
    /// </summary>
    public sealed class StatusNotifierSystray : ISystray
    {
        public const string ItemId = "lively-wallpaper";
        public const string ItemTitle = "Lively Wallpaper";
        public const string IconName = "lively-wallpaper";
        public const string WatcherBusName = "org.kde.StatusNotifierWatcher";
        public static readonly ObjectPath WatcherPath = new("/StatusNotifierWatcher");
        private const string PortalBusName = "org.freedesktop.portal.Desktop";
        private static readonly ObjectPath PortalPath = new("/org/freedesktop/portal/desktop");
        private const string AppearanceNamespace = "org.freedesktop.appearance";
        private const string ColorSchemeKey = "color-scheme";
        private const uint PreferDark = 1;

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Connection connection;
        private readonly NotificationService notifications;
        private readonly string lightIconPath;
        private readonly string darkIconPath;
        private readonly (int, int, byte[])[] lightPixmap;
        private readonly (int, int, byte[])[] darkPixmap;
        private readonly MenuModel menuModel;
        private readonly DBusMenuObject menu;
        private readonly StatusNotifierItemObject item;
        private readonly object sync = new();
        private IDisposable watcherOwnerSubscription;
        private AppTheme? currentTheme;
        private string currentIconPath;
        private bool starting;
        private bool started;
        private bool disposed;

        /// <param name="commands">Application actions and state behind the menu.</param>
        /// <param name="notifications">Sends <see cref="ShowBalloonNotification"/> as desktop notifications.</param>
        /// <param name="lightIconPath">PNG shown while the light theme is active.</param>
        /// <param name="darkIconPath">PNG shown while the dark theme is active.</param>
        public StatusNotifierSystray(TrayCommands commands, NotificationService notifications, string lightIconPath, string darkIconPath)
        {
            ArgumentNullException.ThrowIfNull(commands);
            this.notifications = notifications ?? throw new ArgumentNullException(nameof(notifications));
            this.lightIconPath = lightIconPath;
            this.darkIconPath = darkIconPath;
            lightPixmap = [TrayIconPixmap.Load(lightIconPath)];
            darkPixmap = [TrayIconPixmap.Load(darkIconPath)];

            menuModel = new MenuModel(commands);
            menu = new DBusMenuObject(menuModel);
            item = new StatusNotifierItemObject(ItemId, ItemTitle, IconName, DBusMenuObject.Path, commands.OpenApp);
            item.SetIconPixmap(lightPixmap);
            currentIconPath = lightIconPath;
            connection = DBusConnections.CreateSession();
            BusName = $"org.kde.StatusNotifierItem-{Environment.ProcessId}-1";
        }

        /// <summary>The well-known name this item owns on the session bus.</summary>
        public string BusName { get; }

        public MenuModel Menu => menuModel;

        /// <summary>
        /// Connects to the session bus, exports <c>/StatusNotifierItem</c> and <c>/MenuBar</c>, owns <see cref="BusName"/>
        /// and registers with the StatusNotifierWatcher now and whenever one appears.
        /// </summary>
        public async Task StartAsync()
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            lock (sync)
            {
                if (starting || started)
                    throw new InvalidOperationException("The tray icon has already been started.");
                starting = true;
            }

            await connection.ConnectAsync();
            await connection.RegisterObjectAsync(item);
            await connection.RegisterObjectAsync(menu);
            await connection.RegisterServiceAsync(BusName, ServiceRegistrationOptions.None);
            lock (sync)
                started = true;
            watcherOwnerSubscription = await connection.ResolveServiceOwnerAsync(WatcherBusName, OnWatcherOwnerChanged, OnWatcherSubscriptionError);
            Logger.Info("Tray icon exported as {0}", BusName);
        }

        /// <summary>
        /// Re-reads pause, customise and update state through <see cref="TrayCommands"/> and pushes the changes to the host.
        /// </summary>
        public void RefreshState()
        {
            menu.RefreshState();
        }

        public void SetTheme(AppTheme theme)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            var resolved = theme == AppTheme.Auto ? DBusSync.Run(DetectSystemThemeAsync) : theme;
            lock (sync)
            {
                if (currentTheme == resolved)
                    return;
                currentTheme = resolved;
                currentIconPath = resolved == AppTheme.Dark ? darkIconPath : lightIconPath;
            }
            item.SetIconPixmap(resolved == AppTheme.Dark ? darkPixmap : lightPixmap);
            Logger.Debug("Tray icon theme set to {0}", resolved);
        }

        public void ShowBalloonNotification(int timeout, string title, string msg)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            string iconPath;
            lock (sync)
                iconPath = currentIconPath;
            DBusSync.Run(() => notifications.ShowAsync(title, msg, timeout, iconPath));
        }

        public void Visibility(bool visible)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            item.SetStatus(visible ? StatusNotifierItemObject.ActiveStatus : StatusNotifierItemObject.PassiveStatus);
        }

        public void Dispose()
        {
            bool wasStarted;
            lock (sync)
            {
                if (disposed)
                    return;
                disposed = true;
                wasStarted = started;
            }

            watcherOwnerSubscription?.Dispose();
            if (wasStarted)
            {
                try
                {
                    DBusSync.Run(() => connection.UnregisterServiceAsync(BusName));
                    connection.UnregisterObject(item);
                    connection.UnregisterObject(menu);
                }
                catch (DisconnectedException ex)
                {
                    Logger.Warn(ex, "The session bus connection was already closed; {0} is released with it", BusName);
                }
            }
            connection.Dispose();
        }

        private void OnWatcherOwnerChanged(ServiceOwnerChangedEventArgs change)
        {
            if (string.IsNullOrEmpty(change.NewOwner))
            {
                Logger.Warn("StatusNotifierWatcher left the bus; the tray icon reappears when a watcher registers again");
                return;
            }
            _ = RegisterWithWatcherAsync(change.NewOwner);
        }

        private void OnWatcherSubscriptionError(Exception error)
        {
            Logger.Error(error, "Lost the StatusNotifierWatcher owner subscription");
        }

        private async Task RegisterWithWatcherAsync(string owner)
        {
            try
            {
                var watcher = connection.CreateProxy<IStatusNotifierWatcher>(WatcherBusName, WatcherPath);
                await watcher.RegisterStatusNotifierItemAsync(BusName);
                var hostRegistered = (bool)await watcher.GetAsync("IsStatusNotifierHostRegistered");
                Logger.Info("Registered {0} with StatusNotifierWatcher {1} (host registered: {2})", BusName, owner, hostRegistered);
            }
            catch (DBusException ex)
            {
                Logger.Error(ex, "StatusNotifierWatcher {0} rejected the registration of {1}", owner, BusName);
            }
        }

        private async Task<AppTheme> DetectSystemThemeAsync()
        {
            var settings = DBusConnections.Session.CreateProxy<IPortalSettings>(PortalBusName, PortalPath);
            var value = await settings.ReadAsync(AppearanceNamespace, ColorSchemeKey);
            var scheme = Convert.ToUInt32(value);
            return scheme == PreferDark ? AppTheme.Dark : AppTheme.Light;
        }
    }
}
