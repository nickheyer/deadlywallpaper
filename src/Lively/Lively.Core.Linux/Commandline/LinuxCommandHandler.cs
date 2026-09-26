using CommandLine;
using Lively.Commandline;
using Lively.Common;
using Lively.Common.Factories;
using Lively.Common.Helpers.Storage;
using Lively.Common.Services;
using Lively.Core.Display;
using Lively.Core.Suspend;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Message;
using Newtonsoft.Json.Linq;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using static Lively.Common.CommandlineArgs;

namespace Lively.Core.Linux.Commandline
{
    /// <summary>
    /// Command line automation (livelycu / second instance arguments). Same commands as the Windows
    /// CommandHandler; Windows-only options answer with a logged error instead of doing nothing.
    /// </summary>
    public sealed class LinuxCommandHandler : ICommandHandler
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly IWallpaperLibraryFactory wallpaperLibraryFactory;
        private readonly IUserSettingsService userSettings;
        private readonly IDesktopCore desktopCore;
        private readonly IDisplayManager displayManager;
        private readonly IPlayback playbackMonitor;
        private readonly IRunnerService runner;
        private readonly IStartupService startup;
        private readonly IAppLifetimeService appLifetime;
        private readonly Random rng = new Random();

        public LinuxCommandHandler(IWallpaperLibraryFactory wallpaperLibraryFactory, IUserSettingsService userSettings, IDesktopCore desktopCore,
            IDisplayManager displayManager, IPlayback playbackMonitor, IRunnerService runner, IStartupService startup, IAppLifetimeService appLifetime)
        {
            this.wallpaperLibraryFactory = wallpaperLibraryFactory;
            this.userSettings = userSettings;
            this.desktopCore = desktopCore;
            this.displayManager = displayManager;
            this.playbackMonitor = playbackMonitor;
            this.runner = runner;
            this.startup = startup;
            this.appLifetime = appLifetime;
        }

        public void ParseArgs(string[] args)
        {
            Parser.Default.ParseArguments<AppOptions, SetWallpaperOptions, CustomiseWallpaperOptions, CloseWallpaperOptions, ScreenSaverOptions, SeekWallpaperOptions, ScreenshotOptions>(args)
                .WithParsed<AppOptions>(async opts => await RunAppOptions(opts))
                .WithParsed<SetWallpaperOptions>(async opts => await RunSetWallpaperOptions(opts))
                .WithParsed<CloseWallpaperOptions>(RunCloseWallpaperOptions)
                .WithParsed<SeekWallpaperOptions>(RunSeekWallpaperOptions)
                .WithParsed<CustomiseWallpaperOptions>(RunCustomiseWallpaperOptions)
                .WithParsed<ScreenSaverOptions>(_ => Logger.Error("Screensaver commands are not available on Linux."))
                .WithParsed<ScreenshotOptions>(async opts => await RunScreenshotOptions(opts))
                .WithNotParsed(errs => Logger.Error($"Command line parse error: {string.Join(", ", errs.Select(e => e.Tag))}"));
        }

        private async Task RunAppOptions(AppOptions opts)
        {
            if (opts.ShowApp != null)
            {
                if ((bool)opts.ShowApp) runner.ShowUI(); else runner.CloseUI();
            }

            if (!string.IsNullOrEmpty(opts.Volume) && float.TryParse(opts.Volume, out float val))
            {
                if (opts.Volume.StartsWith('+') || opts.Volume.StartsWith('-'))
                    userSettings.Settings.AudioVolumeGlobal = Clamp(userSettings.Settings.AudioVolumeGlobal + Clamp((int)val, -100, 100), 0, 100);
                else
                    userSettings.Settings.AudioVolumeGlobal = Clamp((int)val, 0, 100);
            }

            if (opts.Play != null)
                playbackMonitor.WallpaperPlaybackPolicy = (bool)opts.Play ? PlaybackPolicy.automatic : PlaybackPolicy.alwaysPaused;

            if (opts.Startup != null)
                _ = await startup.TrySetStartupAsync((bool)opts.Startup);

            if (opts.ShowIcons != null)
                Logger.Error("--showIcons is not available on Linux: desktop icons are managed by the desktop environment.");

            if (opts.ShutdownApp != null)
                appLifetime.Quit();

            if (opts.RestartApp != null)
                Logger.Error("--restart is not supported; stop and start the core instead.");

            if (!string.IsNullOrEmpty(opts.WallpaperArrangement))
            {
                desktopCore.CloseAllWallpapers();
                userSettings.Settings.WallpaperArrangement = opts.WallpaperArrangement switch
                {
                    "per" => WallpaperArrangement.per,
                    "span" => WallpaperArrangement.span,
                    "duplicate" => WallpaperArrangement.duplicate,
                    _ => WallpaperArrangement.per,
                };
                userSettings.Save<SettingsModel>();
            }
        }

        private async Task RunSetWallpaperOptions(SetWallpaperOptions opts)
        {
            if (opts.File == null)
                return;

            if (opts.IsReload)
            {
                var screen = opts.Monitor != null ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == (int)opts.Monitor) : null;
                if (screen != null) await desktopCore.RestartWallpaper(screen); else await desktopCore.RestartWallpaper();
                return;
            }

            if (opts.IsRandom)
            {
                await SetRandomWallpapers(opts.Monitor);
                return;
            }

            if (Directory.Exists(opts.File))
            {
                var screen = opts.Monitor != null
                    ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == (int)opts.Monitor)
                    : displayManager.PrimaryDisplayMonitor;
                var di = new DirectoryInfo(opts.File);
                if (di.Parent != null && di.Parent.FullName.Contains(userSettings.Settings.WallpaperDir, StringComparison.OrdinalIgnoreCase))
                {
                    var libraryItem = wallpaperLibraryFactory.CreateFromDirectory(opts.File);
                    if (screen != null)
                        await desktopCore.SetWallpaperAsync(libraryItem, screen);
                }
                else
                {
                    Logger.Error($"Refusing to set wallpaper from outside the library: {opts.File}");
                }
                return;
            }

            if (File.Exists(opts.File))
            {
                var screen = opts.Monitor != null
                    ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == (int)opts.Monitor)
                    : displayManager.PrimaryDisplayMonitor;
                if (screen == null)
                    return;

                var libraryItem = GetWallpapers().FirstOrDefault(x => x.FilePath != null && x.FilePath.Equals(opts.File, StringComparison.Ordinal));
                if (libraryItem != null)
                {
                    await desktopCore.SetWallpaperAsync(libraryItem, screen);
                    return;
                }

                Logger.Info("Wallpaper not found in library, importing as new file..");
                var dir = Path.Combine(userSettings.Settings.WallpaperDir, Constants.CommonPartialPaths.WallpaperInstallTempDir, Path.GetRandomFileName());
                var metadata = await wallpaperLibraryFactory.CreateMediaWallpaperPackageAsync(opts.File, dir, true);
                if (metadata != null)
                {
                    var model = wallpaperLibraryFactory.CreateFromDirectory(dir);
                    await desktopCore.SetWallpaperAsync(model, screen);
                }
                else
                {
                    Logger.Error($"Unsupported file for import: {opts.File}");
                }
            }
        }

        private async Task SetRandomWallpapers(int? monitorIndex)
        {
            switch (userSettings.Settings.WallpaperArrangement)
            {
                case WallpaperArrangement.per:
                    {
                        var screen = monitorIndex != null ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == monitorIndex) : null;
                        if (screen != null)
                        {
                            var wallpapers = GetRandomWallpaper().Take(2).ToList();
                            if (wallpapers.Count == 0) return;
                            var current = desktopCore.Wallpapers.FirstOrDefault(x => x.Screen.Equals(screen));
                            var pick = wallpapers.Count > 1 && current?.Model.LivelyInfoFolderPath == wallpapers[0].LivelyInfoFolderPath ? wallpapers[1] : wallpapers[0];
                            await desktopCore.SetWallpaperAsync(pick, screen);
                        }
                        else
                        {
                            var screens = displayManager.DisplayMonitors.ToList();
                            var wallpapers = GetRandomWallpaper().Take(screens.Count * 2).ToList();
                            if (wallpapers.Count == 0) return;
                            var used = new List<LibraryModel>();
                            foreach (var s in screens)
                            {
                                var current = desktopCore.Wallpapers.FirstOrDefault(x => x.Screen.Equals(s));
                                var pick = wallpapers.FirstOrDefault(x => (current == null || x.LivelyInfoFolderPath != current.Model.LivelyInfoFolderPath) && !used.Contains(x))
                                    ?? wallpapers.FirstOrDefault(x => !used.Contains(x))
                                    ?? wallpapers[0];
                                used.Add(pick);
                                await desktopCore.SetWallpaperAsync(pick, s);
                            }
                        }
                    }
                    break;
                case WallpaperArrangement.span:
                case WallpaperArrangement.duplicate:
                    {
                        var wallpapers = GetRandomWallpaper().Take(2).ToList();
                        if (wallpapers.Count == 0) return;
                        var current = desktopCore.Wallpapers.FirstOrDefault();
                        var pick = wallpapers.Count > 1 && current?.Model.LivelyInfoFolderPath == wallpapers[0].LivelyInfoFolderPath ? wallpapers[1] : wallpapers[0];
                        await desktopCore.SetWallpaperAsync(pick, displayManager.PrimaryDisplayMonitor);
                    }
                    break;
            }
        }

        private void RunCloseWallpaperOptions(CloseWallpaperOptions opts)
        {
            if (opts.Monitor == null)
                return;
            var id = (int)opts.Monitor;
            if (id == -1 || userSettings.Settings.WallpaperArrangement != WallpaperArrangement.per)
            {
                desktopCore.CloseAllWallpapers();
                return;
            }
            var screen = displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == id);
            if (screen != null)
                desktopCore.CloseWallpaper(screen);
        }

        private void RunSeekWallpaperOptions(SeekWallpaperOptions opts)
        {
            var screen = opts.Monitor != null
                ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == (int)opts.Monitor)
                : displayManager.PrimaryDisplayMonitor;
            if (screen == null || opts.Param == null)
                return;
            var wp = desktopCore.Wallpapers.FirstOrDefault(x => x.Screen.Equals(screen));
            if (wp == null || !float.TryParse(opts.Param, out float val))
                return;

            var relative = opts.Param.StartsWith('+') || opts.Param.StartsWith('-');
            var type = relative ? PlaybackPosType.relativePercent : PlaybackPosType.absolutePercent;
            var value = relative ? Clamp(val, -100, 100) : Clamp(val, 0, 100);
            foreach (var wallpaper in desktopCore.Wallpapers)
            {
                var matches = userSettings.Settings.WallpaperArrangement == WallpaperArrangement.per
                    ? wallpaper.Screen.Equals(screen)
                    : wallpaper.Model == wp.Model;
                if (matches)
                    wallpaper.SetPlaybackPos(value, type);
            }
        }

        private void RunCustomiseWallpaperOptions(CustomiseWallpaperOptions opts)
        {
            if (opts.Param == null)
                return;
            var screen = opts.Monitor != null
                ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == (int)opts.Monitor)
                : displayManager.PrimaryDisplayMonitor;
            if (screen == null)
                return;
            var wp = desktopCore.Wallpapers.FirstOrDefault(x => x.Screen.Equals(screen));
            if (wp == null || wp.LivelyPropertyCopyPath == null)
                return;

            var split = opts.Param.Split('=', 2);
            if (split.Length != 2)
            {
                Logger.Error($"--property expects name=value, got '{opts.Param}'");
                return;
            }
            string name = split[0], val = split[1], ctype = null;
            var lp = JObject.Parse(File.ReadAllText(wp.LivelyPropertyCopyPath));
            foreach (var item in lp)
            {
                if (item.Key.Equals(name, StringComparison.Ordinal))
                {
                    ctype = item.Value["type"].ToString();
                    val = ctype.Equals("folderDropdown", StringComparison.OrdinalIgnoreCase) ? Path.Combine(item.Value["folder"].ToString(), val) : val;
                    break;
                }
            }

            IpcMessage msg = null;
            ctype = ctype == null && name.Equals("lively_default_settings_reload", StringComparison.OrdinalIgnoreCase) ? "button" : ctype;
            if (ctype == null)
            {
                Logger.Error($"Property '{name}' not found in {wp.LivelyPropertyCopyPath}");
                return;
            }

            switch (ctype.ToLowerInvariant())
            {
                case "button":
                    if (name.Equals("lively_default_settings_reload", StringComparison.OrdinalIgnoreCase))
                    {
                        if (RestoreOriginalPropertyFile(wp.Model, wp.LivelyPropertyCopyPath))
                            msg = new LivelyButton { Name = name, IsDefault = true };
                    }
                    else
                    {
                        msg = new LivelyButton { Name = name };
                    }
                    break;
                case "checkbox":
                    msg = new LivelyCheckbox { Name = name, Value = val == "true" };
                    lp[name]["value"] = val == "true";
                    break;
                case "slider":
                    if (double.TryParse(val, out var sliderValue))
                    {
                        msg = new LivelySlider { Name = name, Value = sliderValue, Step = (double?)lp[name]["step"] ?? 1 };
                        lp[name]["value"] = sliderValue;
                    }
                    break;
                case "dropdown":
                    if (int.TryParse(val, out var index))
                    {
                        msg = new LivelyDropdown { Name = name, Value = index };
                        lp[name]["value"] = index;
                    }
                    break;
                case "scalerdropdown":
                    if (int.TryParse(val, out var scalerIndex))
                    {
                        msg = new LivelyDropdownScaler { Name = name, Value = scalerIndex };
                        lp[name]["value"] = scalerIndex;
                    }
                    break;
                case "folderdropdown":
                    msg = new LivelyFolderDropdown { Name = name, Value = val };
                    lp[name]["value"] = val;
                    break;
                case "textbox":
                    msg = new LivelyTextBox { Name = name, Value = val };
                    lp[name]["value"] = val;
                    break;
                case "color":
                    msg = new LivelyColorPicker { Name = name, Value = val };
                    lp[name]["value"] = val;
                    break;
            }

            if (msg == null)
                return;
            if (!(msg is LivelyButton))
                JsonUtil.Write(wp.LivelyPropertyCopyPath, lp);
            if (userSettings.Settings.WallpaperArrangement == WallpaperArrangement.per)
                desktopCore.SendMessageWallpaper(screen, wp.Model.LivelyInfoFolderPath, msg);
            else
                desktopCore.SendMessageWallpaper(wp.Model.LivelyInfoFolderPath, msg);
        }

        private async Task RunScreenshotOptions(ScreenshotOptions opts)
        {
            if (opts.File == null)
                return;
            var screen = opts.Monitor != null
                ? displayManager.DisplayMonitors.FirstOrDefault(x => x.Index == (int)opts.Monitor)
                : displayManager.PrimaryDisplayMonitor;
            var wallpaper = userSettings.Settings.WallpaperArrangement == WallpaperArrangement.per
                ? desktopCore.Wallpapers.FirstOrDefault(x => screen != null && x.Screen.Equals(screen))
                : desktopCore.Wallpapers.FirstOrDefault();
            if (wallpaper == null)
            {
                Logger.Error("No wallpaper running for the screenshot request.");
                return;
            }
            await wallpaper.ScreenCapture(opts.File);
        }

        private static bool RestoreOriginalPropertyFile(LibraryModel model, string copyPath)
        {
            if (model.LivelyPropertyPath == null || !File.Exists(model.LivelyPropertyPath))
                return false;
            File.Copy(model.LivelyPropertyPath, copyPath, true);
            return true;
        }

        private IEnumerable<LibraryModel> GetRandomWallpaper()
        {
            var dir = new List<string>();
            string[] folderPaths =
            {
                Path.Combine(userSettings.Settings.WallpaperDir, Constants.CommonPartialPaths.WallpaperInstallDir),
                Path.Combine(userSettings.Settings.WallpaperDir, Constants.CommonPartialPaths.WallpaperInstallTempDir),
            };
            foreach (var folder in folderPaths)
                if (Directory.Exists(folder))
                    dir.AddRange(Directory.GetDirectories(folder, "*", SearchOption.TopDirectoryOnly));

            // Fisher-Yates shuffle, then yield the ones that parse.
            for (var i = dir.Count - 1; i > 0; i--)
            {
                var j = rng.Next(i + 1);
                (dir[i], dir[j]) = (dir[j], dir[i]);
            }
            foreach (var path in dir)
            {
                LibraryModel item = null;
                try { item = wallpaperLibraryFactory.CreateFromDirectory(path); }
                catch (Exception ex) { Logger.Info($"Skipping {path}: {ex.Message}"); }
                if (item != null)
                    yield return item;
            }
        }

        private IEnumerable<LibraryModel> GetWallpapers()
        {
            string[] folderPaths =
            {
                Path.Combine(userSettings.Settings.WallpaperDir, Constants.CommonPartialPaths.WallpaperInstallDir),
                Path.Combine(userSettings.Settings.WallpaperDir, Constants.CommonPartialPaths.WallpaperInstallTempDir),
            };
            foreach (var folder in folderPaths)
            {
                if (!Directory.Exists(folder)) continue;
                foreach (var path in Directory.GetDirectories(folder, "*", SearchOption.TopDirectoryOnly))
                {
                    LibraryModel item = null;
                    try { item = wallpaperLibraryFactory.CreateFromDirectory(path); }
                    catch (Exception ex) { Logger.Info($"Skipping {path}: {ex.Message}"); }
                    if (item != null)
                        yield return item;
                }
            }
        }

        private static int Clamp(int value, int min, int max) => Math.Max(min, Math.Min(max, value));
        private static float Clamp(float value, float min, float max) => Math.Max(min, Math.Min(max, value));
    }
}
