using Lively.Common;
using Lively.Common.Exceptions;
using Lively.Common.Services;
using Lively.Core.Linux.Display;
using Lively.Core.Linux.Plasma;
using Lively.Core.Linux.Wallpapers;
using Lively.Factories;
using Lively.Models;
using Lively.Models.Enums;
using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Core.Linux.Backends
{
    /// <summary>
    /// Wallpapers rendered inside plasmashell's desktop window through the com.lively.wallpaper
    /// Plasma wallpaper plugin. This is the only way to sit behind desktop icons and widgets on KDE.
    /// </summary>
    public sealed class PlasmaBackend : IWallpaperBackend
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly Connection sessionBus;
        private readonly LinuxDisplayManager displayManager;
        private readonly IUserSettingsService userSettings;
        private readonly ILivelyPropertyFactory propertyFactory;
        private readonly HostWallpaperFactory hosts;
        private readonly PlasmaShellScripting shell;
        private readonly WallpaperSocketServer server = new WallpaperSocketServer();
        private readonly PlasmaDesktopRegistry registry;
        private readonly PlasmaPluginInstaller installer;
        private readonly PlasmaDesktopCoordinator coordinator = new PlasmaDesktopCoordinator();

        public string Name => "plasma";

        public PlasmaBackend(Connection sessionBus, LinuxDisplayManager displayManager, IUserSettingsService userSettings,
            ILivelyPropertyFactory propertyFactory, HostWallpaperFactory hosts)
        {
            this.sessionBus = sessionBus;
            this.displayManager = displayManager;
            this.userSettings = userSettings;
            this.propertyFactory = propertyFactory;
            this.hosts = hosts;
            shell = new PlasmaShellScripting(sessionBus);
            registry = new PlasmaDesktopRegistry(Path.Combine(Constants.CommonPaths.AppDataDir, "plasma-desktops.json"));
            installer = new PlasmaPluginInstaller();
        }

        public async Task InitializeAsync()
        {
            if (!await shell.IsAvailableAsync())
                throw new WorkerWException("plasmashell is not reachable on the session bus (org.kde.PlasmaShell), so the Plasma wallpaper plugin cannot be driven.");

            installer.EnsureInstalled();
            server.Start();

            // A previous core that crashed may have left desktops on our plugin.
            var leftovers = registry.Snapshot();
            foreach (var kv in leftovers)
            {
                Logger.Info($"Restoring desktop {kv.Key} to '{kv.Value}' left over from a previous run.");
                await shell.RestorePluginAsync(kv.Key, kv.Value);
                registry.Forget(kv.Key);
            }

            await RefreshWorkAreasAsync();
            Logger.Info("Plasma backend ready.");
        }

        public IWallpaper CreateWallpaper(LibraryModel model, DisplayMonitor display, WallpaperArrangement arrangement, bool isWindowed)
        {
            if (isWindowed)
                return hosts.CreateWindowed(model, display);

            HostWallpaperFactory.ThrowIfUnsupported(model);

            if (arrangement == WallpaperArrangement.span && displayManager.IsMultiScreen())
            {
                var virtualBounds = displayManager.VirtualScreenBounds;
                var propertyCopy = propertyFactory.CreateLivelyPropertyFolder(model, display, arrangement, userSettings);
                var parts = new List<IWallpaper>();
                foreach (var screen in displayManager.DisplayMonitors.OrderByDescending(s => s.IsPrimary).ToList())
                    parts.Add(CreateForDisplay(model, screen, propertyCopy, new SpanGeometry(screen.Bounds, virtualBounds)));
                return new CompositeWallpaper(model, displayManager.PrimaryDisplayMonitor, propertyCopy, parts);
            }

            var copy = propertyFactory.CreateLivelyPropertyFolder(model, display, arrangement, userSettings);
            return CreateForDisplay(model, display, copy, null);
        }

        private PlasmaWallpaper CreateForDisplay(LibraryModel model, DisplayMonitor display, string propertyCopy, SpanGeometry? span)
        {
            var interactive = userSettings.Settings.InputForward != InputForwardMode.off;
            return new PlasmaWallpaper(model, display, propertyCopy, shell, server, registry, coordinator, span,
                userSettings.Settings.WallpaperScaling, userSettings.Settings.AudioVolumeGlobal, interactive);
        }

        /// <summary>Plasma containments are matched to Wayland outputs by their screen geometry.</summary>
        public static PlasmaDesktop FindDesktop(IReadOnlyList<PlasmaDesktop> desktops, DisplayMonitor display)
        {
            var exact = desktops.FirstOrDefault(d => d.Geometry == display.Bounds);
            if (exact != null)
                return exact;
            return desktops
                .Select(d => (desktop: d, overlap: Area(Rectangle.Intersect(d.Geometry, display.Bounds))))
                .Where(x => x.overlap > 0)
                .OrderByDescending(x => x.overlap)
                .Select(x => x.desktop)
                .FirstOrDefault();
        }

        private static long Area(Rectangle r) => r.IsEmpty ? 0 : (long)r.Width * r.Height;

        public Task OnDisplaysChangedAsync() => RefreshWorkAreasAsync();

        /// <summary>
        /// Work area of every display: its bounds minus the always-visible panels on that screen, the same
        /// area a maximized window fills. Windows reports it as the monitor's working area (no taskbar).
        /// </summary>
        private async Task RefreshWorkAreasAsync()
        {
            var desktops = await shell.ListDesktopsAsync();
            var panels = await shell.ListPanelsAsync();
            displayManager.SetWorkingAreas(ComputeWorkAreas(displayManager.DisplayMonitors.ToList(), desktops, panels));
        }

        public static Dictionary<string, Rectangle> ComputeWorkAreas(IReadOnlyList<DisplayMonitor> displays,
            IReadOnlyList<PlasmaDesktop> desktops, IReadOnlyList<PlasmaPanel> panels)
        {
            var result = new Dictionary<string, Rectangle>();
            foreach (var display in displays)
            {
                var desktop = FindDesktop(desktops, display);
                var area = desktop == null
                    ? display.Bounds
                    : SubtractPanels(display.Bounds, panels.Where(p => p.Screen == desktop.Screen));
                result[display.DeviceId] = area;
            }
            return result;
        }

        public static Rectangle SubtractPanels(Rectangle bounds, IEnumerable<PlasmaPanel> panelsOnScreen)
        {
            var area = bounds;
            foreach (var panel in panelsOnScreen)
            {
                if (!panel.ReservesSpace || panel.Thickness <= 0)
                    continue;
                var thickness = Math.Min(panel.Thickness, panel.Location is "left" or "right" ? area.Width : area.Height);
                switch (panel.Location)
                {
                    case "top":
                        area = new Rectangle(area.X, area.Y + thickness, area.Width, area.Height - thickness);
                        break;
                    case "bottom":
                        area = new Rectangle(area.X, area.Y, area.Width, area.Height - thickness);
                        break;
                    case "left":
                        area = new Rectangle(area.X + thickness, area.Y, area.Width - thickness, area.Height);
                        break;
                    case "right":
                        area = new Rectangle(area.X, area.Y, area.Width - thickness, area.Height);
                        break;
                }
            }
            return area;
        }

        public async Task RestoreDesktopAsync()
        {
            // Wallpapers closed a moment ago are still putting their desktop back; let them finish
            // before the leftovers are handled and before the socket server goes away.
            await coordinator.WaitForPendingClosesAsync();

            foreach (var kv in registry.Snapshot())
            {
                try
                {
                    await shell.RestorePluginAsync(kv.Key, kv.Value);
                    registry.Forget(kv.Key);
                }
                catch (Exception ex)
                {
                    Logger.Error($"Failed to restore desktop {kv.Key} to '{kv.Value}': {ex.Message}");
                }
            }
        }

        public void Dispose()
        {
            server.Dispose();
        }
    }

    /// <summary>
    /// Serialises the hand-over of a Plasma desktop between wallpapers: a new wallpaper waits until the
    /// previous one on the same desktop has restored the plugin, so an apply can never be overtaken by
    /// the restore it replaces, and shutdown waits for every restore still in flight.
    /// </summary>
    public sealed class PlasmaDesktopCoordinator
    {
        private readonly object sync = new object();
        private readonly Dictionary<int, Task> pendingCloses = new Dictionary<int, Task>();

        /// <summary>Completes once no restore is pending on the desktop.</summary>
        public Task WaitForDesktopAsync(int desktopId)
        {
            lock (sync)
                return pendingCloses.TryGetValue(desktopId, out var pending) ? pending : Task.CompletedTask;
        }

        public void TrackClose(int desktopId, Task close)
        {
            lock (sync)
                pendingCloses[desktopId] = close;
            close.ContinueWith(_ =>
            {
                lock (sync)
                {
                    if (pendingCloses.TryGetValue(desktopId, out var current) && ReferenceEquals(current, close))
                        pendingCloses.Remove(desktopId);
                }
            }, TaskScheduler.Default);
        }

        public Task WaitForPendingClosesAsync()
        {
            Task[] pending;
            lock (sync)
                pending = pendingCloses.Values.ToArray();
            return Task.WhenAll(pending);
        }
    }

    /// <summary>
    /// Copies the com.lively.wallpaper package into the user's Plasma wallpaper directory whenever
    /// the shipped copy differs from the installed one.
    /// </summary>
    public sealed class PlasmaPluginInstaller
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        public string SourceDirectory { get; }
        public string DestinationDirectory { get; }

        public PlasmaPluginInstaller()
        {
            SourceDirectory = Environment.GetEnvironmentVariable("LIVELY_PLASMA_PKG")
                ?? Path.Combine(AppContext.BaseDirectory, "plugins", "plasma", PlasmaShellScripting.PluginId);
            var dataHome = Environment.GetEnvironmentVariable("XDG_DATA_HOME");
            if (string.IsNullOrEmpty(dataHome))
                dataHome = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".local", "share");
            DestinationDirectory = Path.Combine(dataHome, "plasma", "wallpapers", PlasmaShellScripting.PluginId);
        }

        public void EnsureInstalled()
        {
            if (!File.Exists(Path.Combine(SourceDirectory, "metadata.json")))
                throw new WallpaperPluginNotFoundException($"The Plasma wallpaper plugin package is missing at {SourceDirectory} (expected metadata.json). Build with `make dist` or set LIVELY_PLASMA_PKG.");

            var sourceHash = HashDirectory(SourceDirectory);
            var stampPath = Path.Combine(DestinationDirectory, ".lively-package-hash");
            if (File.Exists(stampPath) && File.ReadAllText(stampPath).Trim() == sourceHash)
                return;

            Logger.Info($"Installing Plasma wallpaper plugin to {DestinationDirectory}");
            if (Directory.Exists(DestinationDirectory))
                Directory.Delete(DestinationDirectory, true);
            CopyDirectory(SourceDirectory, DestinationDirectory);
            File.WriteAllText(stampPath, sourceHash);
        }

        public static string HashDirectory(string directory)
        {
            using var sha = SHA256.Create();
            var files = Directory.GetFiles(directory, "*", SearchOption.AllDirectories)
                .Where(f => !Path.GetFileName(f).StartsWith(".lively-", StringComparison.Ordinal))
                .OrderBy(f => f, StringComparer.Ordinal);
            var builder = new StringBuilder();
            foreach (var file in files)
            {
                var relative = Path.GetRelativePath(directory, file);
                builder.Append(relative).Append(':').Append(Convert.ToHexString(sha.ComputeHash(File.ReadAllBytes(file)))).Append('\n');
            }
            return Convert.ToHexString(sha.ComputeHash(Encoding.UTF8.GetBytes(builder.ToString())));
        }

        private static void CopyDirectory(string source, string destination)
        {
            Directory.CreateDirectory(destination);
            foreach (var file in Directory.GetFiles(source))
                File.Copy(file, Path.Combine(destination, Path.GetFileName(file)), true);
            foreach (var dir in Directory.GetDirectories(source))
                CopyDirectory(dir, Path.Combine(destination, Path.GetFileName(dir)));
        }
    }
}
