using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.DBus.Activities
{
    /// <summary>
    /// Follows the current KDE activity from kactivitymanagerd on the session bus. The Wayland window
    /// list reports which activities a window belongs to but not which one is current, so the playback
    /// monitor combines both to ignore windows that live on another activity, the way the Windows core
    /// ignores windows on another virtual desktop. <see cref="CurrentActivity"/> is null while no
    /// activity manager owns the bus name; nothing is filtered by activity then because there are none.
    /// </summary>
    public sealed class KdeActivityTracker : IDisposable
    {
        public const string BusName = "org.kde.ActivityManager";
        public static readonly ObjectPath ObjectPath = new("/ActivityManager/Activities");

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Connection sessionBus;
        private readonly object sync = new();
        private IDisposable ownerSubscription;
        private IDisposable signalSubscription;
        private string current;
        private bool started;
        private bool disposed;

        public KdeActivityTracker(Connection sessionBus)
        {
            this.sessionBus = sessionBus ?? throw new ArgumentNullException(nameof(sessionBus));
        }

        /// <summary>Id of the current activity, or null when no activity manager is running.</summary>
        public string CurrentActivity
        {
            get { lock (sync) return current; }
        }

        /// <summary>Raised with the new <see cref="CurrentActivity"/> value.</summary>
        public event EventHandler<string> CurrentActivityChanged;

        /// <summary>
        /// Resolves the activity manager now and whenever it appears or leaves the bus.
        /// </summary>
        public async Task StartAsync()
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            lock (sync)
            {
                if (started)
                    throw new InvalidOperationException("The activity tracker has already been started.");
                started = true;
            }
            ownerSubscription = await sessionBus.ResolveServiceOwnerAsync(BusName, OnOwnerChanged, OnSubscriptionError);
        }

        private void OnOwnerChanged(ServiceOwnerChangedEventArgs change)
        {
            if (!string.IsNullOrEmpty(change.NewOwner))
            {
                _ = AttachAsync();
                return;
            }

            IDisposable stale;
            lock (sync)
            {
                stale = signalSubscription;
                signalSubscription = null;
            }
            stale?.Dispose();
            Logger.Info("No activity manager on the session bus; windows are not filtered by activity.");
            Set(null);
        }

        private async Task AttachAsync()
        {
            try
            {
                var proxy = sessionBus.CreateProxy<IKActivities>(BusName, ObjectPath);
                var subscription = await proxy.WatchCurrentActivityChangedAsync(Set, OnSubscriptionError);
                IDisposable replaced;
                lock (sync)
                {
                    replaced = signalSubscription;
                    signalSubscription = subscription;
                }
                replaced?.Dispose();
                Set(await proxy.CurrentActivityAsync());
                Logger.Info("Following the current activity from {0}", BusName);
            }
            catch (DBusException ex)
            {
                Logger.Error(ex, "Reading the current activity from {0} failed", BusName);
            }
        }

        private void OnSubscriptionError(Exception error)
        {
            Logger.Error(error, "Lost the activity manager subscription");
        }

        private void Set(string id)
        {
            var value = string.IsNullOrEmpty(id) ? null : id;
            bool changed;
            lock (sync)
            {
                changed = value != current;
                current = value;
            }
            if (changed)
            {
                Logger.Info("Current activity: {0}", value ?? "(none)");
                CurrentActivityChanged?.Invoke(this, value);
            }
        }

        public void Dispose()
        {
            IDisposable owner;
            IDisposable signal;
            lock (sync)
            {
                if (disposed)
                    return;
                disposed = true;
                owner = ownerSubscription;
                signal = signalSubscription;
                ownerSubscription = null;
                signalSubscription = null;
            }
            owner?.Dispose();
            signal?.Dispose();
        }
    }
}
