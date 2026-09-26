using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// StatusNotifierItem specification, <c>org.kde.StatusNotifierItem</c>, exported at <c>/StatusNotifierItem</c>.
    /// </summary>
    [DBusInterface("org.kde.StatusNotifierItem")]
    public interface IStatusNotifierItem : IDBusObject
    {
        Task ContextMenuAsync(int x, int y);
        Task ActivateAsync(int x, int y);
        Task SecondaryActivateAsync(int x, int y);
        Task ScrollAsync(int delta, string orientation);
        /// <summary>KDE extension: the host hands over an xdg-activation token right before <c>Activate</c>.</summary>
        Task ProvideXdgActivationTokenAsync(string token);

        Task<IDisposable> WatchNewTitleAsync(Action handler, Action<Exception> onError = null);
        Task<IDisposable> WatchNewIconAsync(Action handler, Action<Exception> onError = null);
        Task<IDisposable> WatchNewStatusAsync(Action<string> handler, Action<Exception> onError = null);

        Task<object> GetAsync(string prop);
        Task<StatusNotifierItemProperties> GetAllAsync();
    }

    /// <summary>
    /// Property set of <c>org.kde.StatusNotifierItem</c>; pixmaps are <c>(width, height, ARGB32 big-endian bytes)</c>.
    /// </summary>
    [Dictionary]
    public sealed class StatusNotifierItemProperties
    {
        private string _Category = string.Empty;
        private string _Id = string.Empty;
        private string _Title = string.Empty;
        private string _Status = string.Empty;
        private int _WindowId;
        private string _IconThemePath = string.Empty;
        private string _IconName = string.Empty;
        private (int, int, byte[])[] _IconPixmap = [];
        private string _OverlayIconName = string.Empty;
        private (int, int, byte[])[] _OverlayIconPixmap = [];
        private string _AttentionIconName = string.Empty;
        private (int, int, byte[])[] _AttentionIconPixmap = [];
        private string _AttentionMovieName = string.Empty;
        private (string, (int, int, byte[])[], string, string) _ToolTip = (string.Empty, [], string.Empty, string.Empty);
        private bool _ItemIsMenu;
        private ObjectPath _Menu = new("/");

        public string Category { get => _Category; set => _Category = value; }
        public string Id { get => _Id; set => _Id = value; }
        public string Title { get => _Title; set => _Title = value; }
        public string Status { get => _Status; set => _Status = value; }
        public int WindowId { get => _WindowId; set => _WindowId = value; }
        public string IconThemePath { get => _IconThemePath; set => _IconThemePath = value; }
        public string IconName { get => _IconName; set => _IconName = value; }
        public (int, int, byte[])[] IconPixmap { get => _IconPixmap; set => _IconPixmap = value; }
        public string OverlayIconName { get => _OverlayIconName; set => _OverlayIconName = value; }
        public (int, int, byte[])[] OverlayIconPixmap { get => _OverlayIconPixmap; set => _OverlayIconPixmap = value; }
        public string AttentionIconName { get => _AttentionIconName; set => _AttentionIconName = value; }
        public (int, int, byte[])[] AttentionIconPixmap { get => _AttentionIconPixmap; set => _AttentionIconPixmap = value; }
        public string AttentionMovieName { get => _AttentionMovieName; set => _AttentionMovieName = value; }
        public (string, (int, int, byte[])[], string, string) ToolTip { get => _ToolTip; set => _ToolTip = value; }
        public bool ItemIsMenu { get => _ItemIsMenu; set => _ItemIsMenu = value; }
        public ObjectPath Menu { get => _Menu; set => _Menu = value; }
    }
}
