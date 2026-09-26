using System;
using System.Collections.Generic;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// <c>com.canonical.dbusmenu</c>: the menu tree a StatusNotifierItem host renders for the tray icon.
    /// A layout node is <c>(id, properties, children as variants of the same struct)</c>.
    /// </summary>
    [DBusInterface("com.canonical.dbusmenu")]
    public interface IDBusMenu : IDBusObject
    {
        Task<(uint revision, (int, IDictionary<string, object>, object[]) layout)> GetLayoutAsync(int parentId, int recursionDepth, string[] propertyNames);
        Task<(int, IDictionary<string, object>)[]> GetGroupPropertiesAsync(int[] ids, string[] propertyNames);
        Task<object> GetPropertyAsync(int id, string name);
        Task EventAsync(int id, string eventId, object data, uint timestamp);
        Task<int[]> EventGroupAsync((int, string, object, uint)[] events);
        Task<bool> AboutToShowAsync(int id);
        Task<(int[] updatesNeeded, int[] idErrors)> AboutToShowGroupAsync(int[] ids);

        Task<IDisposable> WatchItemsPropertiesUpdatedAsync(Action<((int, IDictionary<string, object>)[] updatedProps, (int, string[])[] removedProps)> handler, Action<Exception> onError = null);
        Task<IDisposable> WatchLayoutUpdatedAsync(Action<(uint revision, int parent)> handler, Action<Exception> onError = null);

        Task<object> GetAsync(string prop);
        Task<DBusMenuProperties> GetAllAsync();
    }

    [Dictionary]
    public sealed class DBusMenuProperties
    {
        private uint _Version;
        private string _TextDirection = string.Empty;
        private string _Status = string.Empty;
        private string[] _IconThemePath = [];

        public uint Version { get => _Version; set => _Version = value; }
        public string TextDirection { get => _TextDirection; set => _TextDirection = value; }
        public string Status { get => _Status; set => _Status = value; }
        public string[] IconThemePath { get => _IconThemePath; set => _IconThemePath = value; }
    }
}
