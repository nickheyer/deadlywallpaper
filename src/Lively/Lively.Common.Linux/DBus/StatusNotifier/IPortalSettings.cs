using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// <c>org.freedesktop.portal.Settings</c> on <c>org.freedesktop.portal.Desktop</c> at <c>/org/freedesktop/portal/desktop</c>;
    /// <c>Read("org.freedesktop.appearance", "color-scheme")</c> yields 0 (no preference), 1 (prefer dark) or 2 (prefer light),
    /// <c>Read("org.freedesktop.appearance", "accent-color")</c> a (ddd) struct with components in 0..1.
    /// <c>SettingChanged(namespace, key, value)</c> is emitted whenever a setting changes.
    /// </summary>
    [DBusInterface("org.freedesktop.portal.Settings")]
    public interface IPortalSettings : IDBusObject
    {
        Task<object> ReadAsync(string ns, string key);
        Task<IDisposable> WatchSettingChangedAsync(Action<(string ns, string key, object value)> handler, Action<Exception> onError = null);
    }
}
