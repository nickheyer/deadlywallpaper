using Lively.Common.Linux.DBus;
using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.Power
{
    /// <summary>
    /// Battery, power-profile and session-lock state for playback decisions.
    /// <see cref="IsOnBattery"/> comes from sysfs and is refreshed on a timer; <see cref="IsPowerSaver"/> follows
    /// power-profiles-daemon on the system bus; <see cref="IsSessionLocked"/> follows the session screensaver.
    /// </summary>
    public sealed class PowerStateService : IDisposable
    {
        public const string UPowerProfilesBusName = "org.freedesktop.UPower.PowerProfiles";
        public const string HadessProfilesBusName = "net.hadess.PowerProfiles";
        public const string ScreenSaverBusName = "org.freedesktop.ScreenSaver";
        public static readonly ObjectPath UPowerProfilesPath = new("/org/freedesktop/UPower/PowerProfiles");
        public static readonly ObjectPath HadessProfilesPath = new("/net/hadess/PowerProfiles");
        public static readonly ObjectPath ScreenSaverPath = new("/ScreenSaver");
        public const string PowerSaverProfile = "power-saver";
        private const string ActiveProfileProperty = "ActiveProfile";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly TimeSpan refreshInterval;
        private readonly SysfsPowerSupply powerSupply;
        private readonly Connection sessionBus;
        private readonly Connection systemBus;
        private readonly object sync = new();
        private readonly List<IDisposable> subscriptions = [];
        private Timer refreshTimer;
        private IDisposable profilesSubscription;
        private string profilesSource;
        private bool isOnBattery;
        private bool isPowerSaver;
        private bool isSessionLocked;
        private bool started;
        private bool disposed;

        public PowerStateService(TimeSpan refreshInterval)
            : this(refreshInterval, SysfsPowerSupply.DefaultRoot)
        {
        }

        /// <param name="refreshInterval">How often the sysfs power supply state is re-read.</param>
        /// <param name="sysfsPowerSupplyRoot">Directory holding the power supply entries (<c>/sys/class/power_supply</c>).</param>
        public PowerStateService(TimeSpan refreshInterval, string sysfsPowerSupplyRoot)
            : this(refreshInterval, sysfsPowerSupplyRoot, DBusConnections.Session, DBusConnections.System)
        {
        }

        public PowerStateService(TimeSpan refreshInterval, string sysfsPowerSupplyRoot, Connection sessionBus, Connection systemBus)
        {
            if (refreshInterval <= TimeSpan.Zero)
                throw new ArgumentOutOfRangeException(nameof(refreshInterval), refreshInterval, "The refresh interval must be positive.");
            this.refreshInterval = refreshInterval;
            powerSupply = new SysfsPowerSupply(sysfsPowerSupplyRoot);
            this.sessionBus = sessionBus ?? throw new ArgumentNullException(nameof(sessionBus));
            this.systemBus = systemBus ?? throw new ArgumentNullException(nameof(systemBus));
        }

        /// <summary>A mains supply exists and none is online.</summary>
        public bool IsOnBattery
        {
            get { lock (sync) return isOnBattery; }
        }

        /// <summary>power-profiles-daemon reports the "power-saver" profile; false when no daemon is on the system bus.</summary>
        public bool IsPowerSaver
        {
            get { lock (sync) return isPowerSaver; }
        }

        /// <summary>The session screensaver / lock screen is active.</summary>
        public bool IsSessionLocked
        {
            get { lock (sync) return isSessionLocked; }
        }

        /// <summary>Raised with the new <see cref="IsSessionLocked"/> value.</summary>
        public event EventHandler<bool> LockStateChanged;

        /// <summary>Raised with the new <see cref="IsOnBattery"/> value.</summary>
        public event EventHandler<bool> PowerSourceChanged;

        /// <summary>Raised with the new <see cref="IsPowerSaver"/> value.</summary>
        public event EventHandler<bool> PowerProfileChanged;

        /// <summary>
        /// Reads the initial state, subscribes to the screensaver and power-profile signals and starts the sysfs refresh timer.
        /// </summary>
        public async Task StartAsync()
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            lock (sync)
            {
                if (started)
                    throw new InvalidOperationException("The power state service has already been started.");
                started = true;
            }

            RefreshPowerSupply();
            await StartScreenSaverAsync();
            await StartPowerProfilesAsync();
            refreshTimer = new Timer(OnRefreshTimer, null, refreshInterval, refreshInterval);
        }

        /// <summary>
        /// Re-reads the sysfs power supplies and raises <see cref="PowerSourceChanged"/> when <see cref="IsOnBattery"/> changed.
        /// </summary>
        public void RefreshPowerSupply()
        {
            var onBattery = powerSupply.IsOnBattery();
            bool changed;
            lock (sync)
            {
                changed = onBattery != isOnBattery;
                isOnBattery = onBattery;
            }
            if (changed)
                PowerSourceChanged?.Invoke(this, onBattery);
        }

        public void Dispose()
        {
            lock (sync)
            {
                if (disposed)
                    return;
                disposed = true;
            }
            refreshTimer?.Dispose();
            profilesSubscription?.Dispose();
            foreach (var subscription in subscriptions)
                subscription.Dispose();
            subscriptions.Clear();
        }

        private void OnRefreshTimer(object state)
        {
            try
            {
                RefreshPowerSupply();
            }
            catch (Exception ex)
            {
                Logger.Error(ex, "Refreshing the sysfs power supply state failed");
            }
        }

        private async Task StartScreenSaverAsync()
        {
            var screenSaver = sessionBus.CreateProxy<IScreenSaver>(ScreenSaverBusName, ScreenSaverPath);
            subscriptions.Add(await screenSaver.WatchActiveChangedAsync(SetSessionLocked, error => Logger.Error(error, "Lost the screensaver ActiveChanged subscription")));
            SetSessionLocked(await screenSaver.GetActiveAsync());
        }

        private void SetSessionLocked(bool locked)
        {
            bool changed;
            lock (sync)
            {
                changed = locked != isSessionLocked;
                isSessionLocked = locked;
            }
            if (changed)
            {
                Logger.Info("Session {0}", locked ? "locked" : "unlocked");
                LockStateChanged?.Invoke(this, locked);
            }
        }

        private async Task StartPowerProfilesAsync()
        {
            subscriptions.Add(await systemBus.ResolveServiceOwnerAsync(UPowerProfilesBusName, change => OnProfilesOwnerChanged(UPowerProfilesBusName, change), OnProfilesSubscriptionError));
            subscriptions.Add(await systemBus.ResolveServiceOwnerAsync(HadessProfilesBusName, change => OnProfilesOwnerChanged(HadessProfilesBusName, change), OnProfilesSubscriptionError));
            var anyDaemon = await systemBus.IsServiceActiveAsync(UPowerProfilesBusName) || await systemBus.IsServiceActiveAsync(HadessProfilesBusName);
            if (!anyDaemon)
                Logger.Info("No power-profiles-daemon on the system bus; IsPowerSaver stays false until one appears");
        }

        private void OnProfilesOwnerChanged(string busName, ServiceOwnerChangedEventArgs change)
        {
            if (!string.IsNullOrEmpty(change.NewOwner))
            {
                _ = AttachProfilesAsync(busName);
                return;
            }

            IDisposable stale = null;
            lock (sync)
            {
                if (profilesSource == busName)
                {
                    stale = profilesSubscription;
                    profilesSubscription = null;
                    profilesSource = null;
                }
            }
            if (stale is null)
                return;
            stale.Dispose();
            Logger.Info("{0} left the system bus", busName);
            SetPowerSaver(false);
        }

        private void OnProfilesSubscriptionError(Exception error)
        {
            Logger.Error(error, "Lost the power-profiles-daemon owner subscription");
        }

        private async Task AttachProfilesAsync(string busName)
        {
            try
            {
                lock (sync)
                {
                    // The UPower name is the current one; the hadess name is the same daemon's compatibility alias.
                    if (profilesSource == UPowerProfilesBusName || profilesSource == busName)
                        return;
                }

                var (getActiveProfile, watch) = CreateProfilesProxy(busName);
                var subscription = await watch(changes => OnProfilesPropertiesChanged(changes, getActiveProfile));
                IDisposable replaced;
                lock (sync)
                {
                    replaced = profilesSubscription;
                    profilesSubscription = subscription;
                    profilesSource = busName;
                }
                replaced?.Dispose();
                SetPowerSaver(IsPowerSaverProfile(await getActiveProfile()));
                Logger.Info("Following power profiles from {0}", busName);
            }
            catch (DBusException ex)
            {
                Logger.Error(ex, "Reading the active power profile from {0} failed", busName);
            }
        }

        private (Func<Task<object>> getActiveProfile, Func<Action<PropertyChanges>, Task<IDisposable>> watch) CreateProfilesProxy(string busName)
        {
            if (busName == UPowerProfilesBusName)
            {
                var upower = systemBus.CreateProxy<IUPowerPowerProfiles>(UPowerProfilesBusName, UPowerProfilesPath);
                return (() => upower.GetAsync(ActiveProfileProperty), upower.WatchPropertiesAsync);
            }
            if (busName == HadessProfilesBusName)
            {
                var hadess = systemBus.CreateProxy<IHadessPowerProfiles>(HadessProfilesBusName, HadessProfilesPath);
                return (() => hadess.GetAsync(ActiveProfileProperty), hadess.WatchPropertiesAsync);
            }
            throw new ArgumentOutOfRangeException(nameof(busName), busName, "Not a power-profiles-daemon bus name.");
        }

        private void OnProfilesPropertiesChanged(PropertyChanges changes, Func<Task<object>> getActiveProfile)
        {
            foreach (var change in changes.Changed)
            {
                if (change.Key == ActiveProfileProperty)
                {
                    SetPowerSaver(IsPowerSaverProfile(change.Value));
                    return;
                }
            }
            if (Array.IndexOf(changes.Invalidated, ActiveProfileProperty) >= 0)
                _ = ReReadProfileAsync(getActiveProfile);
        }

        private async Task ReReadProfileAsync(Func<Task<object>> getActiveProfile)
        {
            try
            {
                SetPowerSaver(IsPowerSaverProfile(await getActiveProfile()));
            }
            catch (DBusException ex)
            {
                Logger.Error(ex, "Re-reading the active power profile failed");
            }
        }

        private void SetPowerSaver(bool powerSaver)
        {
            bool changed;
            lock (sync)
            {
                changed = powerSaver != isPowerSaver;
                isPowerSaver = powerSaver;
            }
            if (changed)
            {
                Logger.Info("Power saver profile {0}", powerSaver ? "active" : "inactive");
                PowerProfileChanged?.Invoke(this, powerSaver);
            }
        }

        private static bool IsPowerSaverProfile(object activeProfile)
        {
            return string.Equals(activeProfile as string, PowerSaverProfile, StringComparison.Ordinal);
        }
    }
}
