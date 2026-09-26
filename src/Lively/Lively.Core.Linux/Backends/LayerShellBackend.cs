using Lively.Common.Exceptions;
using Lively.Core.Display;
using Lively.Core.Linux.Display;
using Lively.Core.Linux.Wallpapers;
using Lively.Factories;
using Lively.Models;
using Lively.Models.Enums;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Backends
{
    /// <summary>
    /// Wallpapers as wlr-layer-shell background surfaces, one native host per output.
    /// Works on wlroots compositors (Sway, Hyprland, river, ...) and any other compositor that
    /// implements zwlr_layer_shell_v1 and puts the background layer below its desktop.
    /// </summary>
    public sealed class LayerShellBackend : IWallpaperBackend
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly WaylandMonitorService monitor;
        private readonly IDisplayManager displayManager;
        private readonly HostWallpaperFactory hosts;
        private readonly ILivelyPropertyFactory propertyFactory;
        private readonly Common.Services.IUserSettingsService userSettings;

        public string Name => "layer-shell";

        public LayerShellBackend(WaylandMonitorService monitor, IDisplayManager displayManager, HostWallpaperFactory hosts,
            ILivelyPropertyFactory propertyFactory, Common.Services.IUserSettingsService userSettings)
        {
            this.monitor = monitor;
            this.displayManager = displayManager;
            this.hosts = hosts;
            this.propertyFactory = propertyFactory;
            this.userSettings = userSettings;
        }

        public Task InitializeAsync()
        {
            if (!monitor.Capabilities.LayerShell)
                throw new WorkerWException("This compositor does not provide zwlr_layer_shell_v1, so wallpapers cannot be placed behind windows. Supported: KDE Plasma (through the Plasma plugin), Sway, Hyprland, river, Wayfire and other wlroots based compositors.");
            Logger.Info("Layer-shell backend ready.");
            return Task.CompletedTask;
        }

        public IWallpaper CreateWallpaper(LibraryModel model, DisplayMonitor display, WallpaperArrangement arrangement, bool isWindowed)
        {
            if (isWindowed)
                return hosts.CreateWindowed(model, display);

            if (arrangement == WallpaperArrangement.span && displayManager.IsMultiScreen())
            {
                HostWallpaperFactory.ThrowIfUnsupported(model);
                var virtualBounds = displayManager.VirtualScreenBounds;
                var propertyCopy = propertyFactory.CreateLivelyPropertyFolder(model, display, arrangement, userSettings);
                var parts = new List<IWallpaper>();
                // Primary first so it owns the audio track.
                foreach (var screen in displayManager.DisplayMonitors.OrderByDescending(s => s.IsPrimary).ToList())
                    parts.Add(hosts.CreateForOutput(model, screen, arrangement, new SpanGeometry(screen.Bounds, virtualBounds)));
                return new CompositeWallpaper(model, displayManager.PrimaryDisplayMonitor, propertyCopy, parts);
            }

            return hosts.CreateForOutput(model, display, arrangement, null);
        }

        public Task OnDisplaysChangedAsync() => Task.CompletedTask;

        public Task RestoreDesktopAsync() => Task.CompletedTask;

        public void Dispose() { }
    }
}
