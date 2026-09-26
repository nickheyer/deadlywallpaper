using Lively.Common.Exceptions;
using Lively.Common.Extensions;
using Lively.Common.Factories;
using Lively.Common.Services;
using Lively.Core.Display;
using Lively.Core.Linux.Backends;
using Lively.Core.Suspend;
using Lively.Factories;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Message;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Core
{
    /// <summary>
    /// The Linux desktop core: owns the running wallpapers, their layout persistence and display
    /// change handling. The compositor-specific work (how a wallpaper reaches the desktop) lives in
    /// the <see cref="IWallpaperBackend"/>. Mirrors WinDesktopCore without the WorkerW plumbing.
    /// </summary>
    public sealed class LinuxDesktopCore : IDesktopCore
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly SemaphoreSlim loadingLock = new SemaphoreSlim(1, 1);
        private readonly List<IWallpaper> wallpapers = new List<IWallpaper>(2);
        private readonly List<WallpaperLayoutModel> wallpapersDisconnected = new List<WallpaperLayoutModel>();
        private readonly object layoutWriteLock = new object();
        private bool isInitialized;
        private bool disposed;

        private readonly IUserSettingsService userSettings;
        private readonly IDisplayManager displayManager;
        private readonly IPlayback playback;
        private readonly IWallpaperPluginFactory wallpaperFactory;
        private readonly IWallpaperLibraryFactory wallpaperLibraryFactory;
        private readonly IWallpaperBackend backend;
        private readonly IResourceService i18n;

        public ReadOnlyCollection<IWallpaper> Wallpapers => wallpapers.AsReadOnly();

        /// <summary>
        /// There is no WorkerW on Linux. The gRPC layer only checks this for "is the core initialised",
        /// so it is non-zero once the backend is connected.
        /// </summary>
        public IntPtr DesktopWorkerW => isInitialized ? new IntPtr(1) : IntPtr.Zero;

        public IWallpaperBackend Backend => backend;

        public event EventHandler WallpaperChanged;
        public event EventHandler<Exception> WallpaperError;
        public event EventHandler WallpaperReset;

        public LinuxDesktopCore(IUserSettingsService userSettings,
            IDisplayManager displayManager,
            IPlayback playback,
            IWallpaperPluginFactory wallpaperFactory,
            IWallpaperLibraryFactory wallpaperLibraryFactory,
            IWallpaperBackend backend,
            IResourceService i18n)
        {
            this.userSettings = userSettings;
            this.displayManager = displayManager;
            this.playback = playback;
            this.wallpaperFactory = wallpaperFactory;
            this.wallpaperLibraryFactory = wallpaperLibraryFactory;
            this.backend = backend;
            this.i18n = i18n;

            this.displayManager.DisplayUpdated += DisplayManager_DisplayUpdated;
            this.WallpaperChanged += (s, e) => SaveWallpaperLayout();
            this.playback.WallpaperControlChanged += Playback_WallpaperControlChanged;
        }

        public async Task InitializeAsync()
        {
            await backend.InitializeAsync();
            isInitialized = true;
            WallpaperReset?.Invoke(this, EventArgs.Empty);
        }

        /// <summary>Reports a problem to the UI without stopping anything (used for platform capability gaps).</summary>
        public void ReportError(Exception error) => WallpaperError?.Invoke(this, error);

        public async Task SetWallpaperAsync(LibraryModel wallpaper, DisplayMonitor display)
        {
            await loadingLock.WaitAsync();
            try
            {
                Logger.Info($"Setting wallpaper: {wallpaper.Title} | {wallpaper.FilePath}");

                var fileExists = !wallpaper.LivelyInfo.IsAbsolutePath || wallpaper.LivelyInfo.Type.IsOnlineWallpaper() || File.Exists(wallpaper.FilePath);
                if (!fileExists)
                {
                    Logger.Info($"Skipping wallpaper, file {wallpaper.LivelyInfo.FileName} not found.");
                    WallpaperError?.Invoke(this, new WallpaperNotFoundException($"{i18n.GetString("TextFileNotFound")}\n{wallpaper.LivelyInfo.FileName}"));
                    WallpaperChanged?.Invoke(this, EventArgs.Empty);
                    return;
                }

                IWallpaper current = null;
                try
                {
                    switch (userSettings.Settings.WallpaperArrangement)
                    {
                        case WallpaperArrangement.per:
                            CloseWallpaper(display, fireEvent: false);
                            current = wallpaperFactory.CreateWallpaper(wallpaper, display, userSettings.Settings.WallpaperArrangement);
                            await StartAsync(current);
                            break;
                        case WallpaperArrangement.span:
                            CloseAllWallpapers(fireEvent: false);
                            current = wallpaperFactory.CreateWallpaper(wallpaper, display, userSettings.Settings.WallpaperArrangement);
                            await StartAsync(current);
                            break;
                        case WallpaperArrangement.duplicate:
                            CloseAllWallpapers(fireEvent: false);
                            foreach (var screen in displayManager.DisplayMonitors.ToList())
                            {
                                current = wallpaperFactory.CreateWallpaper(wallpaper, screen, userSettings.Settings.WallpaperArrangement);
                                await StartAsync(current);
                            }
                            foreach (var item in wallpapers)
                            {
                                if (!item.Screen.IsPrimary)
                                {
                                    Logger.Info($"Disabling audio track on screen {item.Screen.DeviceName} (duplicate.)");
                                    item.SetMute(true);
                                }
                                item.SetPlaybackPos(0, PlaybackPosType.absolutePercent);
                            }
                            break;
                    }
                    WallpaperChanged?.Invoke(this, EventArgs.Empty);
                }
                catch (Exception ex)
                {
                    Logger.Error(ex);
                    WallpaperError?.Invoke(this, ex);
                    WallpaperChanged?.Invoke(this, EventArgs.Empty);
                    current?.Dispose();
                }
            }
            finally
            {
                loadingLock.Release();
            }
        }

        private async Task StartAsync(IWallpaper wallpaper)
        {
            wallpaper.Exited += Wallpaper_Exited;
            await wallpaper.ShowAsync();
            // Web pages that do not handle resize are reloaded, like on Windows.
            if (wallpaper.Category.IsWebWallpaper())
                wallpaper.SetPlaybackPos(0, PlaybackPosType.absolutePercent);
            wallpapers.Add(wallpaper);
        }

        private void Wallpaper_Exited(object sender, EventArgs e)
        {
            var wallpaper = sender as IWallpaper;
            if (wallpaper == null)
                return;

            bool wasRunning;
            lock (layoutWriteLock)
            {
                wasRunning = wallpapers.Remove(wallpaper);
            }
            if (!wasRunning)
                return; // closed by us

            Logger.Error($"Wallpaper exited unexpectedly: {wallpaper.Model.Title} on {wallpaper.Screen?.DeviceName}");
            WallpaperError?.Invoke(this, new WallpaperPluginException($"{wallpaper.Model.Title} stopped unexpectedly. See the log for the host output."));
            WallpaperChanged?.Invoke(this, EventArgs.Empty);
        }

        public async Task ResetWallpaperAsync()
        {
            await loadingLock.WaitAsync();
            try
            {
                Logger.Info("Restarting wallpaper service..");
                var original = Wallpapers.ToList();
                CloseAllWallpapers(false);
                isInitialized = false;
                await backend.InitializeAsync();
                isInitialized = true;
                WallpaperReset?.Invoke(this, EventArgs.Empty);
                foreach (var item in original)
                {
                    _ = SetWallpaperAsync(item.Model, item.Screen);
                    if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.duplicate)
                        break;
                }
            }
            finally
            {
                loadingLock.Release();
            }
        }

        public async Task RestartWallpaper()
        {
            var original = Wallpapers.ToList();
            CloseAllWallpapers(false);
            foreach (var item in original)
            {
                await SetWallpaperAsync(item.Model, item.Screen);
                if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.duplicate)
                    break;
            }
        }

        public async Task RestartWallpaper(DisplayMonitor display)
        {
            var original = Wallpapers.Where(x => x.Screen.Equals(display)).ToList();
            CloseWallpaper(display, false);
            foreach (var item in original)
            {
                await SetWallpaperAsync(item.Model, item.Screen);
                if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.duplicate)
                    break;
            }
        }

        private void SaveWallpaperLayout()
        {
            lock (layoutWriteLock)
            {
                userSettings.WallpaperLayout.Clear();
                foreach (var wallpaper in wallpapers)
                    userSettings.WallpaperLayout.Add(new WallpaperLayoutModel(wallpaper.Screen, wallpaper.Model.LivelyInfoFolderPath));
                if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.per)
                    userSettings.WallpaperLayout.AddRange(wallpapersDisconnected);
                try
                {
                    userSettings.Save<List<WallpaperLayoutModel>>();
                }
                catch (Exception e)
                {
                    Logger.Error(e.ToString());
                }
            }
        }

        private async void DisplayManager_DisplayUpdated(object sender, EventArgs e)
        {
            if (!isInitialized)
                return;

            await loadingLock.WaitAsync();
            try
            {
                using (playback.DeferPlayback())
                {
                    Logger.Info("Display settings changed, screen(s):");
                    foreach (var x in displayManager.DisplayMonitors.ToList())
                        Logger.Info($"{x.DeviceName} {x.Bounds}");
                    await backend.OnDisplaysChangedAsync();
                    RefreshWallpaper();
                    RestoreDisconnectedWallpapers();
                }
            }
            finally
            {
                loadingLock.Release();
            }
        }

        private void RefreshWallpaper()
        {
            try
            {
                var allScreens = displayManager.DisplayMonitors.ToList();
                var orphans = wallpapers.FindAll(w => allScreens.Find(s => w.Screen.Equals(s)) == null);

                userSettings.Settings.SelectedDisplay =
                    allScreens.Find(x => userSettings.Settings.SelectedDisplay != null && userSettings.Settings.SelectedDisplay.Equals(x))
                    ?? displayManager.PrimaryDisplayMonitor;
                userSettings.Save<SettingsModel>();

                switch (userSettings.Settings.WallpaperArrangement)
                {
                    case WallpaperArrangement.per:
                        if (orphans.Count != 0)
                        {
                            var newOrphans = orphans.FindAll(o => wallpapersDisconnected.Find(d => d.Display.Equals(o.Screen)) == null);
                            foreach (var item in newOrphans)
                                wallpapersDisconnected.Add(new WallpaperLayoutModel(item.Screen, item.Model.LivelyInfoFolderPath));
                            foreach (var x in orphans)
                            {
                                Logger.Info($"Disconnected Screen: {x.Screen.DeviceName} {x.Screen.Bounds}");
                                x.Exited -= Wallpaper_Exited;
                                x.Close();
                                x.Dispose();
                            }
                            wallpapers.RemoveAll(x => orphans.Contains(x));
                        }
                        break;
                    case WallpaperArrangement.duplicate:
                        if (orphans.Count != 0)
                        {
                            foreach (var x in orphans)
                            {
                                Logger.Info($"Disconnected Screen: {x.Screen.DeviceName} {x.Screen.Bounds}");
                                x.Exited -= Wallpaper_Exited;
                                x.Close();
                                x.Dispose();
                            }
                            wallpapers.RemoveAll(x => orphans.Contains(x));
                        }
                        break;
                    case WallpaperArrangement.span:
                        break;
                }

                UpdateWallpaperRect();
            }
            catch (Exception ex)
            {
                Logger.Error(ex.ToString());
            }
            finally
            {
                WallpaperChanged?.Invoke(this, EventArgs.Empty);
            }
        }

        private void UpdateWallpaperRect()
        {
            // Layer-shell surfaces and Plasma containments follow their output automatically; only a
            // spanned wallpaper depends on the overall geometry and must be re-created with new slices.
            if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.span && wallpapers.Count != 0)
            {
                Logger.Info("Virtual screen changed, restarting spanned wallpaper.");
                var model = wallpapers[0].Model;
                CloseAllWallpapers(false);
                _ = SetWallpaperAsync(model, displayManager.PrimaryDisplayMonitor);
                return;
            }

            foreach (var screen in displayManager.DisplayMonitors.ToList())
            {
                var index = wallpapers.FindIndex(x => x.Screen.Equals(screen));
                if (index != -1)
                    wallpapers[index].Screen = screen;
            }
        }

        private void RestoreDisconnectedWallpapers()
        {
            try
            {
                switch (userSettings.Settings.WallpaperArrangement)
                {
                    case WallpaperArrangement.per:
                        var toRestore = wallpapersDisconnected.FindAll(w => displayManager.DisplayMonitors.FirstOrDefault(s => w.Display.Equals(s)) != null);
                        RestoreWallpaper(toRestore);
                        break;
                    case WallpaperArrangement.span:
                        break;
                    case WallpaperArrangement.duplicate:
                        if (displayManager.DisplayMonitors.Count > Wallpapers.Count && Wallpapers.Count != 0)
                        {
                            var newScreen = displayManager.DisplayMonitors.FirstOrDefault(screen => Wallpapers.FirstOrDefault(wp => wp.Screen.Equals(screen)) == null);
                            if (newScreen != null)
                                _ = SetWallpaperAsync(Wallpapers[0].Model, newScreen);
                        }
                        break;
                }
            }
            catch (Exception e)
            {
                Logger.Error("Failed to restore disconnected wallpaper(s): " + e);
            }
        }

        private void RestoreWallpaper(List<WallpaperLayoutModel> layouts)
        {
            foreach (var layout in layouts.ToList())
            {
                LibraryModel libraryItem;
                try
                {
                    libraryItem = wallpaperLibraryFactory.CreateFromDirectory(layout.LivelyInfoPath);
                }
                catch (Exception e)
                {
                    Logger.Info($"Skipping restoration of {layout.LivelyInfoPath} | {e.Message}");
                    wallpapersDisconnected.Remove(layout);
                    continue;
                }

                var screen = displayManager.DisplayMonitors.FirstOrDefault(x => x.Equals(layout.Display));
                if (screen == null)
                {
                    Logger.Info($"Screen missing, skipping restoration of {layout.LivelyInfoPath} | {layout.Display.DeviceName}");
                    if (!wallpapersDisconnected.Contains(layout))
                        wallpapersDisconnected.Add(new WallpaperLayoutModel(layout.Display, layout.LivelyInfoPath));
                }
                else
                {
                    Logger.Info($"Restoring wallpaper {libraryItem.Title} | {libraryItem.LivelyInfoFolderPath}");
                    _ = SetWallpaperAsync(libraryItem, screen);
                    wallpapersDisconnected.Remove(layout);
                }
            }
        }

        public void RestoreWallpaper()
        {
            try
            {
                var layout = userSettings.WallpaperLayout;
                if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.span ||
                    userSettings.Settings.WallpaperArrangement == WallpaperArrangement.duplicate)
                {
                    if (layout.Count != 0)
                    {
                        var libraryItem = wallpaperLibraryFactory.CreateFromDirectory(layout[0].LivelyInfoPath);
                        _ = SetWallpaperAsync(libraryItem, displayManager.PrimaryDisplayMonitor);
                    }
                }
                else
                {
                    RestoreWallpaper(layout);
                }
            }
            catch (Exception e)
            {
                Logger.Error($"Failed to restore wallpaper: {e}");
            }
        }

        public void CloseAllWallpapers() => CloseAllWallpapers(fireEvent: true);

        private void CloseAllWallpapers(bool fireEvent)
        {
            if (wallpapers.Count == 0)
                return;
            foreach (var x in wallpapers.ToList())
            {
                x.Exited -= Wallpaper_Exited;
                x.Close();
                x.Dispose();
            }
            wallpapers.Clear();
            if (fireEvent)
                WallpaperChanged?.Invoke(this, EventArgs.Empty);
        }

        public void CloseWallpaper(DisplayMonitor display) => CloseWallpaper(display, fireEvent: true);

        private void CloseWallpaper(DisplayMonitor display, bool fireEvent)
        {
            var matches = wallpapers.FindAll(x => x.Screen.Equals(display));
            if (matches.Count == 0)
                return;
            CloseMatches(matches, fireEvent);
        }

        public void CloseWallpaper(WallpaperType type)
        {
            var matches = wallpapers.FindAll(x => x.Category == type);
            if (matches.Count == 0)
                return;
            CloseMatches(matches, fireEvent: true);
        }

        public void CloseWallpaper(LibraryModel wp) => CloseWallpaper(wp, fireEvent: true);

        private void CloseWallpaper(LibraryModel wp, bool fireEvent)
        {
            var matches = wallpapers.FindAll(x => x.Model.LivelyInfoFolderPath == wp.LivelyInfoFolderPath);
            if (matches.Count == 0)
                return;
            CloseMatches(matches, fireEvent);
        }

        private void CloseMatches(List<IWallpaper> matches, bool fireEvent)
        {
            foreach (var x in matches)
            {
                x.Exited -= Wallpaper_Exited;
                x.Close();
                x.Dispose();
            }
            wallpapers.RemoveAll(x => matches.Contains(x));
            if (fireEvent)
                WallpaperChanged?.Invoke(this, EventArgs.Empty);
        }

        public void SendMessageWallpaper(string info_path, IpcMessage msg)
        {
            foreach (var x in wallpapers.ToList())
                if (x.Model.LivelyInfoFolderPath == info_path)
                    x.SendMessage(msg);
        }

        public void SendMessageWallpaper(DisplayMonitor display, string info_path, IpcMessage msg)
        {
            foreach (var x in wallpapers.ToList())
                if (x.Screen.Equals(display) && info_path == x.Model.LivelyInfoFolderPath)
                    x.SendMessage(msg);
        }

        private void Playback_WallpaperControlChanged(object sender, WallpaperControlEventArgs e)
        {
            try
            {
                foreach (var wallpaper in wallpapers.ToList())
                {
                    if (e.Display != null && !wallpaper.Screen.Equals(e.Display))
                        continue;

                    switch (e.Action)
                    {
                        case WallpaperControlAction.Pause:
                            wallpaper.Pause();
                            break;
                        case WallpaperControlAction.Play:
                            wallpaper.Play();
                            break;
                        case WallpaperControlAction.SetVolume:
                            wallpaper.SetVolume(e.Volume ?? 0);
                            break;
                    }
                }
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
            }
        }

        public void Dispose()
        {
            if (disposed)
                return;
            disposed = true;
            displayManager.DisplayUpdated -= DisplayManager_DisplayUpdated;
            playback.WallpaperControlChanged -= Playback_WallpaperControlChanged;
            CloseAllWallpapers(false);
            backend.RestoreDesktopAsync().GetAwaiter().GetResult();
            backend.Dispose();
        }
    }
}
