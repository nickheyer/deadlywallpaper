using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.NowPlaying
{
    /// <summary>Well-known MPRIS bus-name prefix and object path.</summary>
    public static class Mpris
    {
        public const string BusNamePrefix = "org.mpris.MediaPlayer2.";
        public const string ObjectPath = "/org/mpris/MediaPlayer2";
    }

    /// <summary>Tmds.DBus proxy for the <c>org.mpris.MediaPlayer2</c> root interface.</summary>
    [DBusInterface("org.mpris.MediaPlayer2")]
    public interface IMprisMediaPlayer2 : IDBusObject
    {
        Task<object> GetAsync(string prop);
    }

    /// <summary>Tmds.DBus proxy for the <c>org.mpris.MediaPlayer2.Player</c> interface.</summary>
    [DBusInterface("org.mpris.MediaPlayer2.Player")]
    public interface IMprisPlayer : IDBusObject
    {
        Task<object> GetAsync(string prop);
        Task<IDisposable> WatchPropertiesAsync(Action<PropertyChanges> handler);
    }
}
