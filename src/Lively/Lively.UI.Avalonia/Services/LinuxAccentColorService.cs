using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Threading;
using Lively.Common.Linux.DBus;
using Lively.Common.Linux.DBus.StatusNotifier;
using System;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Keeps the Fluent theme's accent (SystemAccentColor and its Dark1..3 / Light1..3 shades) equal to the
    /// desktop's accent colour, read live from the settings portal (org.freedesktop.appearance accent-color,
    /// set by KDE and GNOME), the way the WinUI client follows the Windows accent through UISettings.
    /// </summary>
    public sealed class LinuxAccentColorService : IDisposable
    {
        private const string PortalBusName = "org.freedesktop.portal.Desktop";
        private static readonly ObjectPath PortalPath = new("/org/freedesktop/portal/desktop");
        private const string AppearanceNamespace = "org.freedesktop.appearance";
        private const string AccentColorKey = "accent-color";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private IDisposable subscription;
        private bool disposed;

        /// <summary>
        /// Subscribes to accent changes and applies the current accent. Runs on the D-Bus thread pool;
        /// resource updates are posted to the UI thread.
        /// </summary>
        public async Task StartAsync()
        {
            IPortalSettings settings;
            try
            {
                settings = DBusConnections.Session.CreateProxy<IPortalSettings>(PortalBusName, PortalPath);
                subscription = await settings.WatchSettingChangedAsync(change =>
                {
                    if (change.ns == AppearanceNamespace && change.key == AccentColorKey)
                        Apply(change.value);
                }, error => Logger.Error(error, "Lost the settings portal subscription"));
            }
            catch (Exception ex) when (ex is DBusException || ex is InvalidOperationException)
            {
                Logger.Error(ex, "The settings portal is unreachable; the theme keeps its default accent");
                return;
            }

            object value;
            try
            {
                value = await settings.ReadAsync(AppearanceNamespace, AccentColorKey);
            }
            catch (DBusException ex)
            {
                Logger.Info("The settings portal has no accent colour ({0}); the theme keeps its default accent", ex.Message);
                return;
            }
            Apply(value);
        }

        private void Apply(object value)
        {
            if (!TryParse(value, out var accent))
            {
                Logger.Info("The desktop reports no accent colour; the theme keeps its default accent");
                return;
            }
            Dispatcher.UIThread.Post(() =>
            {
                if (!disposed)
                    SetAccent(Application.Current.Resources, accent);
            });
        }

        /// <summary>
        /// The portal encodes the colour as a (ddd) struct with components in 0..1; components outside that
        /// range are the portal's way of saying that no accent is set.
        /// </summary>
        public static bool TryParse(object value, out Color color)
        {
            color = default;
            if (value is not ValueTuple<double, double, double> rgb)
                return false;
            var (r, g, b) = rgb;
            if (r < 0 || r > 1 || g < 0 || g > 1 || b < 0 || b > 1)
                return false;
            color = Color.FromRgb((byte)Math.Round(r * 255), (byte)Math.Round(g * 255), (byte)Math.Round(b * 255));
            return true;
        }

        /// <summary>
        /// Writes the accent and its six shades into the application resources; every theme brush that
        /// references them through DynamicResource follows.
        /// </summary>
        public static void SetAccent(IResourceDictionary resources, Color accent)
        {
            var hsl = accent.ToHsl();
            resources["SystemAccentColor"] = accent;
            resources["SystemAccentColorDark1"] = Shade(hsl, -0.10);
            resources["SystemAccentColorDark2"] = Shade(hsl, -0.20);
            resources["SystemAccentColorDark3"] = Shade(hsl, -0.30);
            resources["SystemAccentColorLight1"] = Shade(hsl, 0.10);
            resources["SystemAccentColorLight2"] = Shade(hsl, 0.20);
            resources["SystemAccentColorLight3"] = Shade(hsl, 0.30);
        }

        private static Color Shade(HslColor hsl, double lightnessDelta)
        {
            var lightness = Math.Clamp(hsl.L + lightnessDelta, 0, 1);
            return new HslColor(hsl.A, hsl.H, hsl.S, lightness).ToRgb();
        }

        public void Dispose()
        {
            if (disposed)
                return;
            disposed = true;
            subscription?.Dispose();
            subscription = null;
        }
    }
}
