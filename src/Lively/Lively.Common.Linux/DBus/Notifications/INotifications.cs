using System.Collections.Generic;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.Notifications
{
    /// <summary>
    /// Desktop Notifications Specification, <c>org.freedesktop.Notifications</c> at <c>/org/freedesktop/Notifications</c>.
    /// </summary>
    [DBusInterface("org.freedesktop.Notifications")]
    public interface INotifications : IDBusObject
    {
        /// <summary>Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, expire_timeout) → id</summary>
        Task<uint> NotifyAsync(string appName, uint replacesId, string appIcon, string summary, string body, string[] actions, IDictionary<string, object> hints, int expireTimeout);
    }
}
