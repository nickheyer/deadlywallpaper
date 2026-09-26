using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// <c>org.kde.StatusNotifierWatcher</c> at <c>/StatusNotifierWatcher</c>: items register their bus name here and
    /// hosts (panels) pick them up from it.
    /// </summary>
    [DBusInterface("org.kde.StatusNotifierWatcher")]
    public interface IStatusNotifierWatcher : IDBusObject
    {
        Task RegisterStatusNotifierItemAsync(string service);

        /// <summary>org.freedesktop.DBus.Properties.Get (IsStatusNotifierHostRegistered, ProtocolVersion, RegisteredStatusNotifierItems).</summary>
        Task<object> GetAsync(string prop);
    }
}
