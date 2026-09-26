using System;
using System.Threading;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus
{
    /// <summary>
    /// Bus connections whose signal handlers and callbacks run on D-Bus worker threads rather than on a captured
    /// synchronization context, so a UI thread blocked in a synchronous call can never deadlock with them.
    /// </summary>
    public static class DBusConnections
    {
        private static readonly Lazy<Connection> session = new(() => Create(Address.Session, autoConnect: true), LazyThreadSafetyMode.ExecutionAndPublication);
        private static readonly Lazy<Connection> system = new(() => Create(Address.System, autoConnect: true), LazyThreadSafetyMode.ExecutionAndPublication);

        /// <summary>Shared auto-connecting session bus connection for proxies.</summary>
        public static Connection Session => session.Value;

        /// <summary>Shared auto-connecting system bus connection for proxies.</summary>
        public static Connection System => system.Value;

        /// <summary>
        /// A dedicated session bus connection for exporting objects and owning bus names.
        /// The caller connects it with <see cref="Connection.ConnectAsync"/> and disposes it.
        /// </summary>
        public static Connection CreateSession() => Create(Address.Session, autoConnect: false);

        private static Connection Create(string address, bool autoConnect)
        {
            if (string.IsNullOrEmpty(address))
                throw new InvalidOperationException("The D-Bus address is not set; DBUS_SESSION_BUS_ADDRESS (or the system bus socket) is unavailable in this session.");
            return new Connection(new ClientConnectionOptions(address)
            {
                AutoConnect = autoConnect,
                SynchronizationContext = null,
                RunContinuationsAsynchronously = true,
            });
        }
    }
}
