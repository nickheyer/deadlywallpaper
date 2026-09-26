using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.Activities
{
    /// <summary>
    /// <c>org.kde.ActivityManager.Activities</c> on <c>org.kde.ActivityManager</c> at <c>/ActivityManager/Activities</c>:
    /// the KDE activity manager's current activity and its change signal.
    /// </summary>
    [DBusInterface("org.kde.ActivityManager.Activities")]
    public interface IKActivities : IDBusObject
    {
        Task<string> CurrentActivityAsync();
        Task<IDisposable> WatchCurrentActivityChangedAsync(Action<string> handler, Action<Exception> onError = null);
    }
}
