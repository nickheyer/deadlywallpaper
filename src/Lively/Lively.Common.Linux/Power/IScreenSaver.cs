using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.Power
{
    /// <summary>
    /// <c>org.freedesktop.ScreenSaver</c> at <c>/ScreenSaver</c>: whether the session is locked.
    /// </summary>
    [DBusInterface("org.freedesktop.ScreenSaver")]
    public interface IScreenSaver : IDBusObject
    {
        Task<bool> GetActiveAsync();
        Task<IDisposable> WatchActiveChangedAsync(Action<bool> handler, Action<Exception> onError = null);
    }
}
