using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// The <c>com.canonical.dbusmenu</c> object exported at <c>/MenuBar</c>, backed by a <see cref="MenuModel"/>.
    /// </summary>
    public sealed class DBusMenuObject : IDBusMenu
    {
        public static readonly ObjectPath Path = new("/MenuBar");
        public const uint ProtocolVersion = 3;
        public const string NormalStatus = "normal";
        public const string LeftToRight = "ltr";
        private const string ClickedEvent = "clicked";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly MenuModel model;

        public DBusMenuObject(MenuModel model)
        {
            this.model = model ?? throw new ArgumentNullException(nameof(model));
        }

        public event Action<((int, IDictionary<string, object>)[] updatedProps, (int, string[])[] removedProps)> OnItemsPropertiesUpdated;
        public event Action<(uint revision, int parent)> OnLayoutUpdated;

        public ObjectPath ObjectPath => Path;

        public MenuModel Model => model;

        public Task<(uint revision, (int, IDictionary<string, object>, object[]) layout)> GetLayoutAsync(int parentId, int recursionDepth, string[] propertyNames)
        {
            var layout = model.BuildLayout(parentId, recursionDepth, propertyNames);
            return Task.FromResult((model.Revision, layout));
        }

        public Task<(int, IDictionary<string, object>)[]> GetGroupPropertiesAsync(int[] ids, string[] propertyNames)
        {
            return Task.FromResult(model.GetGroupProperties(ids, propertyNames));
        }

        public Task<object> GetPropertyAsync(int id, string name)
        {
            return Task.FromResult(model.GetProperty(id, name));
        }

        public Task EventAsync(int id, string eventId, object data, uint timestamp)
        {
            HandleEvent(id, eventId);
            return Task.CompletedTask;
        }

        public Task<int[]> EventGroupAsync((int, string, object, uint)[] events)
        {
            var idErrors = new List<int>();
            foreach (var (id, eventId, _, _) in events)
            {
                if (!model.TryGetEntry(id, out _) && id != MenuModel.RootId)
                {
                    idErrors.Add(id);
                    continue;
                }
                HandleEvent(id, eventId);
            }
            return Task.FromResult(idErrors.ToArray());
        }

        public Task<bool> AboutToShowAsync(int id)
        {
            return Task.FromResult(RefreshState());
        }

        public Task<(int[] updatesNeeded, int[] idErrors)> AboutToShowGroupAsync(int[] ids)
        {
            var known = ids.Where(id => id == MenuModel.RootId || model.TryGetEntry(id, out _)).ToArray();
            var unknown = ids.Except(known).ToArray();
            var changed = RefreshState();
            return Task.FromResult((changed ? known : [], unknown));
        }

        /// <summary>
        /// Re-reads the application state behind every item and, when anything changed, emits
        /// <c>ItemsPropertiesUpdated</c> for the changed properties and <c>LayoutUpdated</c> with the new revision.
        /// </summary>
        public bool RefreshState()
        {
            var changes = model.Refresh();
            if (changes.Count == 0)
                return false;
            OnItemsPropertiesUpdated?.Invoke((changes.Select(change => (change.id, change.changed)).ToArray(), []));
            OnLayoutUpdated?.Invoke((model.Revision, MenuModel.RootId));
            return true;
        }

        public Task<IDisposable> WatchItemsPropertiesUpdatedAsync(Action<((int, IDictionary<string, object>)[] updatedProps, (int, string[])[] removedProps)> handler, Action<Exception> onError = null)
        {
            return SignalWatcher.AddAsync(this, nameof(OnItemsPropertiesUpdated), handler);
        }

        public Task<IDisposable> WatchLayoutUpdatedAsync(Action<(uint revision, int parent)> handler, Action<Exception> onError = null)
        {
            return SignalWatcher.AddAsync(this, nameof(OnLayoutUpdated), handler);
        }

        public Task<object> GetAsync(string prop)
        {
            object value = prop switch
            {
                nameof(DBusMenuProperties.Version) => ProtocolVersion,
                nameof(DBusMenuProperties.Status) => NormalStatus,
                nameof(DBusMenuProperties.TextDirection) => LeftToRight,
                nameof(DBusMenuProperties.IconThemePath) => Array.Empty<string>(),
                _ => throw new ArgumentException($"com.canonical.dbusmenu has no property '{prop}'.", nameof(prop)),
            };
            return Task.FromResult(value);
        }

        public Task<DBusMenuProperties> GetAllAsync()
        {
            return Task.FromResult(new DBusMenuProperties
            {
                Version = ProtocolVersion,
                Status = NormalStatus,
                TextDirection = LeftToRight,
                IconThemePath = [],
            });
        }

        private void HandleEvent(int id, string eventId)
        {
            if (!string.Equals(eventId, ClickedEvent, StringComparison.Ordinal))
            {
                Logger.Trace("dbusmenu event '{0}' on item {1} needs no action", eventId, id);
                return;
            }

            try
            {
                if (!model.Invoke(id))
                    Logger.Warn("Click on tray menu item {0} ignored: it is a separator or disabled", id);
            }
            catch (Exception ex)
            {
                Logger.Error(ex, "Tray menu command for item {0} failed", id);
                throw;
            }
            RefreshState();
        }
    }
}
