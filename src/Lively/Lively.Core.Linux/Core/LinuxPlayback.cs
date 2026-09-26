using Lively.Common.Linux.DBus.Activities;
using Lively.Common.Linux.Power;
using Lively.Common.Services;
using Lively.Core.Display;
using Lively.Core.Linux.Display;
using Lively.Core.Suspend;
using Lively.Models;
using Lively.Models.Enums;
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Linq;
using System.Threading;

namespace Lively.Core.Linux.Core
{
    /// <summary>
    /// Pause/resume logic driven by the compositor's toplevel window list (lively-wl-monitor) and
    /// the session power/lock state. Same rules, thresholds and settings as the Windows Playback
    /// class and WindowUtil: a window covers a display when it is maximized, fullscreen or covers
    /// 95% of the work area; the grid algorithm pauses when the uncovered share of the work area
    /// drops to the configured threshold.
    /// </summary>
    public sealed class LinuxPlayback : IPlayback
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        /// <summary>Share of a display a single window has to cover to count as covering it (WindowUtil default).</summary>
        public const double CoverThreshold = 0.95;

        private readonly IUserSettingsService userSettings;
        private readonly IDisplayManager displayManager;
        private readonly WaylandMonitorService monitor;
        private readonly PowerStateService power;
        private readonly KdeActivityTracker activities;
        private readonly Timer timer;
        private readonly object tickLock = new object();
        private int deferDepth;
        private bool running;
        private bool disposed;
        private PlaybackPolicy policy = PlaybackPolicy.automatic;

        public PlaybackPolicy WallpaperPlaybackPolicy
        {
            get => policy;
            set
            {
                policy = value;
                PlaybackPolicyChanged?.Invoke(this, policy);
            }
        }

        /// <summary>False when the compositor offers no window tracking protocol; only system-state pauses work then.</summary>
        public bool IsWindowTrackingAvailable => monitor.Capabilities.HasToplevelTracking;

        public event EventHandler<PlaybackPolicy> PlaybackPolicyChanged;
        public event EventHandler<WallpaperControlEventArgs> WallpaperControlChanged;

        public LinuxPlayback(IUserSettingsService userSettings, IDisplayManager displayManager, WaylandMonitorService monitor,
            PowerStateService power, KdeActivityTracker activities)
        {
            this.userSettings = userSettings;
            this.displayManager = displayManager;
            this.monitor = monitor;
            this.power = power;
            this.activities = activities;
            timer = new Timer(_ => Tick(), null, Timeout.Infinite, Timeout.Infinite);
        }

        public void Start()
        {
            running = true;
            var interval = Math.Max(250, userSettings.Settings.ProcessTimerInterval);
            timer.Change(0, interval);
        }

        public void Stop()
        {
            running = false;
            timer.Change(Timeout.Infinite, Timeout.Infinite);
        }

        public IDisposable DeferPlayback()
        {
            Interlocked.Increment(ref deferDepth);
            return new Deferrer(this);
        }

        private sealed class Deferrer : IDisposable
        {
            private LinuxPlayback owner;
            public Deferrer(LinuxPlayback owner) => this.owner = owner;
            public void Dispose()
            {
                var o = Interlocked.Exchange(ref owner, null);
                if (o != null)
                    Interlocked.Decrement(ref o.deferDepth);
            }
        }

        private void Tick()
        {
            if (!running || disposed || Volatile.Read(ref deferDepth) > 0)
                return;
            if (!Monitor.TryEnter(tickLock))
                return;
            try
            {
                if (IsPauseDueToSystemState())
                {
                    PauseWallpapers();
                    return;
                }

                if (!IsWindowTrackingAvailable)
                {
                    // Nothing to inspect: keep playing with the global volume.
                    PlayWallpapers();
                    SetWallpaperVolume(userSettings.Settings.AudioVolumeGlobal);
                    return;
                }

                var windows = VisibleWindows();
                switch (userSettings.Settings.ProcessMonitorAlgorithm)
                {
                    case ProcessMonitorAlgorithm.foreground:
                        EvaluateByForegroundWindow(windows);
                        break;
                    case ProcessMonitorAlgorithm.all:
                        EvaluateByVisibleWindows(windows, (display, ws) => IsDisplayCoveredByAnyWindow(ws, display.WorkingArea));
                        break;
                    case ProcessMonitorAlgorithm.grid:
                        EvaluateByVisibleWindows(windows, (display, ws) => GridCoverage(ws, display.WorkingArea,
                            userSettings.Settings.ProcessMonitorGridTileSize, userSettings.Settings.ProcessMonitorGridTileCoverageThreshold));
                        break;
                    case ProcessMonitorAlgorithm.gamemode:
                        EvaluateByGameMode(windows);
                        break;
                }
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
            }
            finally
            {
                Monitor.Exit(tickLock);
            }
        }

        /// <summary>
        /// Windows that can cover the desktop right now, the counterpart of WindowUtil.GetVisibleTopLevelWindows:
        /// none while KWin shows the desktop, otherwise the mapped, titled, non-minimized windows on the current
        /// virtual desktop and activity that are neither shell chrome nor Lively's own previews.
        /// </summary>
        private List<WaylandToplevel> VisibleWindows()
        {
            if (monitor.IsShowDesktopActive)
                return new List<WaylandToplevel>();
            var activity = activities.CurrentActivity;
            return monitor.Toplevels.Where(w => IsVisibleTopLevelWindow(w, activity)).ToList();
        }

        public static bool IsVisibleTopLevelWindow(WaylandToplevel window, string currentActivity)
        {
            if (window.Minimized || window.SkipTaskbar || !window.OnCurrentDesktop)
                return false;
            if (string.IsNullOrEmpty(window.Title))
                return false;
            if (currentActivity != null && window.Activities.Count != 0 && !window.Activities.Contains(currentActivity))
                return false;
            return !IsShellWindow(window) && !IsLivelyWindow(window);
        }

        public static bool IsShellWindow(WaylandToplevel w)
        {
            var app = (w.AppId ?? string.Empty).ToLowerInvariant();
            return app == "plasmashell" || app == "org.kde.plasmashell" || app == "krunner" || app == "org.kde.krunner"
                || app == "waybar" || app == "swaybg" || app == "hyprpaper" || app == "kwin_wayland" || app == "org.kde.kwin";
        }

        public static bool IsLivelyWindow(WaylandToplevel w)
        {
            var app = (w.AppId ?? string.Empty).ToLowerInvariant();
            return app.StartsWith("lively", StringComparison.Ordinal) || app == "lively.ui.avalonia";
        }

        private void EvaluateByForegroundWindow(List<WaylandToplevel> windows)
        {
            var foreground = windows.FirstOrDefault(w => w.Activated);
            var isValidWindow = foreground != null;
            var foregroundDisplay = displayManager.PrimaryDisplayMonitor;
            var isFullScreenPause = userSettings.Settings.AppFullscreenPause == AppRules.pause;
            var isFocusedAppPause = userSettings.Settings.AppFocusPause == AppRules.pause;
            var isCovered = false;

            if (isValidWindow)
            {
                foregroundDisplay = DisplayOf(foreground) ?? displayManager.PrimaryDisplayMonitor;
                isCovered = Covers(foreground, foregroundDisplay.WorkingArea);
            }

            if (isValidWindow && IsPauseRuleApp(foreground))
            {
                PauseWallpapers();
            }
            else if (!isValidWindow)
            {
                // The desktop, a shell window or one of our own previews has the focus.
                PlayWallpapers();
                SetWallpaperVolume(userSettings.Settings.AudioVolumeGlobal);
            }
            else
            {
                var shouldPause = isFullScreenPause && (isFocusedAppPause || isCovered);
                switch (userSettings.Settings.DisplayPauseSettings)
                {
                    case DisplayPause.perdisplay:
                        foreach (var display in displayManager.DisplayMonitors.ToList())
                        {
                            if (foregroundDisplay.Equals(display))
                            {
                                if (shouldPause) PauseWallpaper(foregroundDisplay); else PlayWallpaper(foregroundDisplay);
                            }
                            else
                            {
                                PlayWallpaper(display);
                            }
                        }
                        break;
                    case DisplayPause.all:
                        if (shouldPause) PauseWallpapers(); else PlayWallpapers();
                        break;
                }

                SetWallpaperVolume(userSettings.Settings.AudioOnlyOnDesktop ? 0 : userSettings.Settings.AudioVolumeGlobal);
            }
        }

        private void EvaluateByVisibleWindows(List<WaylandToplevel> windows, Func<DisplayMonitor, List<WaylandToplevel>, bool> coverageCheck)
        {
            if (windows.Exists(IsPauseRuleApp))
            {
                PauseWallpapers();
                return;
            }

            var displays = displayManager.DisplayMonitors.ToList();
            var map = displays.ToDictionary(d => d, d => windows.Where(w => IsOnDisplay(w, d)).ToList());
            var effectiveDisplayPause = userSettings.Settings.WallpaperArrangement == WallpaperArrangement.duplicate
                ? DisplayPause.all
                : userSettings.Settings.DisplayPauseSettings;

            switch (effectiveDisplayPause)
            {
                case DisplayPause.perdisplay:
                    if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.span)
                    {
                        var pauseAll = displays.All(d => ShouldPause(d, map[d], coverageCheck));
                        if (pauseAll) PauseWallpapers(); else PlayWallpapers();
                    }
                    else
                    {
                        foreach (var d in displays)
                        {
                            if (ShouldPause(d, map[d], coverageCheck)) PauseWallpaper(d); else PlayWallpaper(d);
                        }
                    }
                    break;
                case DisplayPause.all:
                    var pauseAny = displays.Any(d => ShouldPause(d, map[d], coverageCheck));
                    if (pauseAny) PauseWallpapers(); else PlayWallpapers();
                    break;
            }

            var effectiveAudio = userSettings.Settings.WallpaperArrangement == WallpaperArrangement.per
                ? userSettings.Settings.DisplayAudioOutput
                : DisplayAudioMode.all;
            var selectedAudioDisplay = displays.FirstOrDefault(x => userSettings.Settings.SelectedAudioOutputDisplay != null && x.Equals(userSettings.Settings.SelectedAudioOutputDisplay))
                ?? displayManager.PrimaryDisplayMonitor;
            foreach (var d in displays)
            {
                var isDesktop = map[d].Count == 0;
                var volume = isDesktop ? userSettings.Settings.AudioVolumeGlobal
                    : (userSettings.Settings.AudioOnlyOnDesktop ? 0 : userSettings.Settings.AudioVolumeGlobal);
                switch (effectiveAudio)
                {
                    case DisplayAudioMode.selection:
                        if (!displayManager.IsMultiScreen() || d.Equals(selectedAudioDisplay))
                            SetWallpaperVolume(volume, d);
                        else
                            SetWallpaperVolume(0, d);
                        break;
                    case DisplayAudioMode.all:
                        SetWallpaperVolume(volume, d);
                        break;
                }
            }
        }

        private void EvaluateByGameMode(List<WaylandToplevel> windows)
        {
            var foreground = windows.FirstOrDefault(w => w.Activated);
            if (foreground != null && foreground.Fullscreen)
            {
                PauseWallpapers();
                return;
            }
            PlayWallpapers();
            SetWallpaperVolume(userSettings.Settings.AudioVolumeGlobal);
        }

        private bool ShouldPause(DisplayMonitor display, List<WaylandToplevel> windowsOnDisplay, Func<DisplayMonitor, List<WaylandToplevel>, bool> coverageCheck)
        {
            var isFullScreenPause = userSettings.Settings.AppFullscreenPause == AppRules.pause;
            var isFocusedAppPause = userSettings.Settings.AppFocusPause == AppRules.pause;
            var isDesktop = windowsOnDisplay.Count == 0;
            var isCovered = coverageCheck(display, windowsOnDisplay);
            return isFullScreenPause && ((isFocusedAppPause && !isDesktop) || isCovered);
        }

        private bool IsPauseRuleApp(WaylandToplevel window)
        {
            if (userSettings.AppRules.Count == 0)
                return false;
            var appId = window.AppId ?? string.Empty;
            var shortName = appId.Contains('.') ? appId.Substring(appId.LastIndexOf('.') + 1) : appId;
            foreach (var rule in userSettings.AppRules)
            {
                if (string.Equals(rule.AppName, appId, StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(rule.AppName, shortName, StringComparison.OrdinalIgnoreCase))
                {
                    return rule.Rule == AppRules.pause;
                }
            }
            return false;
        }

        private DisplayMonitor DisplayOf(WaylandToplevel window)
        {
            var displays = displayManager.DisplayMonitors.ToList();
            if (window.Geometry != null && window.Geometry.Length == 4)
            {
                var rect = ToRect(window.Geometry);
                var best = displays.OrderByDescending(d => Area(Rectangle.Intersect(rect, d.Bounds))).FirstOrDefault();
                if (best != null && Area(Rectangle.Intersect(rect, best.Bounds)) > 0)
                    return best;
            }
            return displays.FirstOrDefault(d => window.Outputs.Contains(d.DeviceId));
        }

        private static bool IsOnDisplay(WaylandToplevel window, DisplayMonitor display)
        {
            if (window.Geometry != null && window.Geometry.Length == 4)
                return Area(Rectangle.Intersect(ToRect(window.Geometry), display.Bounds)) > 0;
            return window.Outputs.Contains(display.DeviceId);
        }

        /// <summary>
        /// WindowUtil.IsDisplayCoveredByWindow: a maximized (IsZoomed) or fullscreen window covers its display,
        /// otherwise the window rectangle has to cover 95% of the area.
        /// </summary>
        public static bool Covers(WaylandToplevel window, Rectangle area)
        {
            if (window.Fullscreen || window.Maximized)
                return true;
            return window.Geometry != null && window.Geometry.Length == 4
                && IsWindowCoveringTarget(ToRect(window.Geometry), area, CoverThreshold);
        }

        /// <summary>WindowUtil.IsDisplayCoveredByAnyWindow.</summary>
        public static bool IsDisplayCoveredByAnyWindow(List<WaylandToplevel> windows, Rectangle area)
        {
            return windows.Exists(w => Covers(w, area));
        }

        /// <summary>WindowUtil.IsWindowCoveringTarget: share of the target area under the window rectangle.</summary>
        public static bool IsWindowCoveringTarget(Rectangle windowRect, Rectangle targetArea, double threshold)
        {
            var targetSize = (long)targetArea.Width * targetArea.Height;
            if (targetSize <= 0)
                return false;
            var intersection = Rectangle.Intersect(windowRect, targetArea);
            var ratio = ((long)intersection.Width * intersection.Height) / (double)targetSize;
            return ratio >= threshold;
        }

        /// <summary>
        /// WindowUtil.IsDisplayCoveredByWindowGrid: any maximized or fullscreen window covers the display; a single
        /// window covering 95% of it does too; otherwise the area is split into tiles and the display counts as
        /// covered once the share of tiles no window touches is down to <paramref name="threshold"/>.
        /// </summary>
        public static bool GridCoverage(List<WaylandToplevel> windows, Rectangle area, int tileSize, double threshold)
        {
            if (windows is null || windows.Count == 0)
                return false;
            if (windows.Exists(w => w.Fullscreen || w.Maximized))
                return true;

            tileSize = Math.Max(1, tileSize);
            var cols = (int)Math.Ceiling(area.Width / (double)tileSize);
            var rows = (int)Math.Ceiling(area.Height / (double)tileSize);
            if (cols <= 0 || rows <= 0)
                return false;
            var totalTiles = rows * cols;
            var coveredCount = 0;
            var covered = new bool[rows, cols];

            foreach (var window in windows)
            {
                if (window.Geometry == null || window.Geometry.Length != 4)
                    continue;
                var rect = ToRect(window.Geometry);
                if (rect.Width <= 0 || rect.Height <= 0)
                    continue;

                if (IsWindowCoveringTarget(rect, area, CoverThreshold))
                    return true;

                var xStart = Math.Max(0, (rect.Left - area.Left) / tileSize);
                var xEnd = Math.Min(cols - 1, (rect.Right - area.Left - 1) / tileSize);
                var yStart = Math.Max(0, (rect.Top - area.Top) / tileSize);
                var yEnd = Math.Min(rows - 1, (rect.Bottom - area.Top - 1) / tileSize);

                for (var y = yStart; y <= yEnd; y++)
                {
                    for (var x = xStart; x <= xEnd; x++)
                    {
                        if (covered[y, x])
                            continue;
                        covered[y, x] = true;
                        coveredCount++;
                        if ((double)(totalTiles - coveredCount) / totalTiles <= threshold)
                            return true;
                    }
                }
            }

            return false;
        }

        private static Rectangle ToRect(int[] g) => new Rectangle(g[0], g[1], g[2], g[3]);
        private static long Area(Rectangle r) => r.IsEmpty ? 0 : (long)r.Width * r.Height;

        private bool IsPauseDueToSystemState()
        {
            if (WallpaperPlaybackPolicy == PlaybackPolicy.alwaysPaused || power.IsSessionLocked)
                return true;
            if (userSettings.Settings.BatteryPause == AppRules.pause && power.IsOnBattery)
                return true;
            if (userSettings.Settings.PowerSaveModePause == AppRules.pause && power.IsPowerSaver)
                return true;
            return false;
        }

        private void PauseWallpapers() => WallpaperControlChanged?.Invoke(this, new WallpaperControlEventArgs(WallpaperControlAction.Pause));
        private void PlayWallpapers() => WallpaperControlChanged?.Invoke(this, new WallpaperControlEventArgs(WallpaperControlAction.Play));
        private void PauseWallpaper(DisplayMonitor d) => WallpaperControlChanged?.Invoke(this, new WallpaperControlEventArgs(WallpaperControlAction.Pause, d));
        private void PlayWallpaper(DisplayMonitor d) => WallpaperControlChanged?.Invoke(this, new WallpaperControlEventArgs(WallpaperControlAction.Play, d));
        private void SetWallpaperVolume(int volume) => WallpaperControlChanged?.Invoke(this, new WallpaperControlEventArgs(WallpaperControlAction.SetVolume, null, volume));
        private void SetWallpaperVolume(int volume, DisplayMonitor d) => WallpaperControlChanged?.Invoke(this, new WallpaperControlEventArgs(WallpaperControlAction.SetVolume, d, volume));

        public void Dispose()
        {
            if (disposed) return;
            disposed = true;
            timer.Dispose();
        }
    }
}
