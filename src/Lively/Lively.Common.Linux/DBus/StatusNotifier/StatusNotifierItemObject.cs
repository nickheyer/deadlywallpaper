using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// The <c>org.kde.StatusNotifierItem</c> object exported at <c>/StatusNotifierItem</c>.
    /// </summary>
    public sealed class StatusNotifierItemObject : IStatusNotifierItem
    {
        public static readonly ObjectPath Path = new("/StatusNotifierItem");
        public const string ApplicationStatusCategory = "ApplicationStatus";
        public const string ActiveStatus = "Active";
        public const string PassiveStatus = "Passive";
        private const string XdgActivationTokenVariable = "XDG_ACTIVATION_TOKEN";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Action activate;
        private readonly object sync = new();
        private readonly string id;
        private readonly string iconName;
        private readonly ObjectPath menuPath;
        private string title;
        private string status = ActiveStatus;
        private (int, int, byte[])[] iconPixmap = [];

        /// <param name="activate">Runs on <c>Activate</c> (left click / double click on the icon).</param>
        public StatusNotifierItemObject(string id, string title, string iconName, ObjectPath menuPath, Action activate)
        {
            if (string.IsNullOrWhiteSpace(id))
                throw new ArgumentException("The item id must not be empty.", nameof(id));
            if (string.IsNullOrWhiteSpace(title))
                throw new ArgumentException("The item title must not be empty.", nameof(title));
            if (string.IsNullOrWhiteSpace(iconName))
                throw new ArgumentException("The icon name must not be empty.", nameof(iconName));
            this.id = id;
            this.title = title;
            this.iconName = iconName;
            this.menuPath = menuPath;
            this.activate = activate ?? throw new ArgumentNullException(nameof(activate));
        }

        public event Action OnNewTitle;
        public event Action OnNewIcon;
        public event Action<string> OnNewStatus;

        public ObjectPath ObjectPath => Path;

        public string Status
        {
            get { lock (sync) return status; }
        }

        public (int, int, byte[])[] IconPixmap
        {
            get { lock (sync) return iconPixmap; }
        }

        /// <summary>Replaces the icon pixmaps and emits <c>NewIcon</c>.</summary>
        public void SetIconPixmap((int, int, byte[])[] pixmap)
        {
            ArgumentNullException.ThrowIfNull(pixmap);
            lock (sync)
                iconPixmap = pixmap;
            OnNewIcon?.Invoke();
        }

        /// <summary>Sets <c>Status</c> ("Active", "Passive" or "NeedsAttention") and emits <c>NewStatus</c>.</summary>
        public void SetStatus(string newStatus)
        {
            if (string.IsNullOrWhiteSpace(newStatus))
                throw new ArgumentException("The status must not be empty.", nameof(newStatus));
            lock (sync)
                status = newStatus;
            OnNewStatus?.Invoke(newStatus);
        }

        /// <summary>Sets <c>Title</c> and emits <c>NewTitle</c>.</summary>
        public void SetTitle(string newTitle)
        {
            if (string.IsNullOrWhiteSpace(newTitle))
                throw new ArgumentException("The title must not be empty.", nameof(newTitle));
            lock (sync)
                title = newTitle;
            OnNewTitle?.Invoke();
        }

        public Task ActivateAsync(int x, int y)
        {
            Logger.Debug("StatusNotifierItem activated at ({0}, {1})", x, y);
            activate();
            return Task.CompletedTask;
        }

        /// <summary>Middle click has no bound action.</summary>
        public Task SecondaryActivateAsync(int x, int y) => Task.CompletedTask;

        /// <summary>The host renders the exported dbusmenu itself; nothing else opens on right click.</summary>
        public Task ContextMenuAsync(int x, int y) => Task.CompletedTask;

        /// <summary>Scrolling over the icon has no bound action.</summary>
        public Task ScrollAsync(int delta, string orientation) => Task.CompletedTask;

        /// <summary>
        /// Publishes the token the host granted so the window the application raises next can take focus on Wayland;
        /// toolkits read <c>XDG_ACTIVATION_TOKEN</c> from the environment when activating a window.
        /// </summary>
        public Task ProvideXdgActivationTokenAsync(string token)
        {
            Environment.SetEnvironmentVariable(XdgActivationTokenVariable, token);
            return Task.CompletedTask;
        }

        public Task<IDisposable> WatchNewTitleAsync(Action handler, Action<Exception> onError = null)
        {
            return SignalWatcher.AddAsync(this, nameof(OnNewTitle), handler);
        }

        public Task<IDisposable> WatchNewIconAsync(Action handler, Action<Exception> onError = null)
        {
            return SignalWatcher.AddAsync(this, nameof(OnNewIcon), handler);
        }

        public Task<IDisposable> WatchNewStatusAsync(Action<string> handler, Action<Exception> onError = null)
        {
            return SignalWatcher.AddAsync(this, nameof(OnNewStatus), handler);
        }

        public Task<object> GetAsync(string prop)
        {
            var all = Snapshot();
            object value = prop switch
            {
                nameof(StatusNotifierItemProperties.Category) => all.Category,
                nameof(StatusNotifierItemProperties.Id) => all.Id,
                nameof(StatusNotifierItemProperties.Title) => all.Title,
                nameof(StatusNotifierItemProperties.Status) => all.Status,
                nameof(StatusNotifierItemProperties.WindowId) => all.WindowId,
                nameof(StatusNotifierItemProperties.IconThemePath) => all.IconThemePath,
                nameof(StatusNotifierItemProperties.IconName) => all.IconName,
                nameof(StatusNotifierItemProperties.IconPixmap) => all.IconPixmap,
                nameof(StatusNotifierItemProperties.OverlayIconName) => all.OverlayIconName,
                nameof(StatusNotifierItemProperties.OverlayIconPixmap) => all.OverlayIconPixmap,
                nameof(StatusNotifierItemProperties.AttentionIconName) => all.AttentionIconName,
                nameof(StatusNotifierItemProperties.AttentionIconPixmap) => all.AttentionIconPixmap,
                nameof(StatusNotifierItemProperties.AttentionMovieName) => all.AttentionMovieName,
                nameof(StatusNotifierItemProperties.ToolTip) => all.ToolTip,
                nameof(StatusNotifierItemProperties.ItemIsMenu) => all.ItemIsMenu,
                nameof(StatusNotifierItemProperties.Menu) => all.Menu,
                _ => throw new ArgumentException($"org.kde.StatusNotifierItem has no property '{prop}'.", nameof(prop)),
            };
            return Task.FromResult(value);
        }

        public Task<StatusNotifierItemProperties> GetAllAsync() => Task.FromResult(Snapshot());

        private StatusNotifierItemProperties Snapshot()
        {
            lock (sync)
            {
                return new StatusNotifierItemProperties
                {
                    Category = ApplicationStatusCategory,
                    Id = id,
                    Title = title,
                    Status = status,
                    WindowId = 0,
                    IconThemePath = string.Empty,
                    IconName = iconName,
                    IconPixmap = iconPixmap,
                    OverlayIconName = string.Empty,
                    OverlayIconPixmap = [],
                    AttentionIconName = string.Empty,
                    AttentionIconPixmap = [],
                    AttentionMovieName = string.Empty,
                    ToolTip = (iconName, iconPixmap, title, string.Empty),
                    ItemIsMenu = false,
                    Menu = menuPath,
                };
            }
        }
    }
}
