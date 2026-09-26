using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.Power
{
    /// <summary>
    /// power-profiles-daemon on the system bus, <c>net.hadess.PowerProfiles</c> at <c>/net/hadess/PowerProfiles</c>.
    /// </summary>
    [DBusInterface("net.hadess.PowerProfiles")]
    public interface IHadessPowerProfiles : IDBusObject
    {
        /// <summary>org.freedesktop.DBus.Properties.Get (ActiveProfile is "power-saver", "balanced" or "performance").</summary>
        Task<object> GetAsync(string prop);
        Task<IDisposable> WatchPropertiesAsync(Action<PropertyChanges> handler);
    }

    /// <summary>
    /// power-profiles-daemon 0.20+ on the system bus, <c>org.freedesktop.UPower.PowerProfiles</c> at <c>/org/freedesktop/UPower/PowerProfiles</c>.
    /// </summary>
    [DBusInterface("org.freedesktop.UPower.PowerProfiles")]
    public interface IUPowerPowerProfiles : IDBusObject
    {
        /// <summary>org.freedesktop.DBus.Properties.Get (ActiveProfile is "power-saver", "balanced" or "performance").</summary>
        Task<object> GetAsync(string prop);
        Task<IDisposable> WatchPropertiesAsync(Action<PropertyChanges> handler);
    }
}
