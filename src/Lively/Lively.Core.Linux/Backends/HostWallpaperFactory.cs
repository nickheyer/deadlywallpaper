using Lively.Common;
using Lively.Common.Exceptions;
using Lively.Common.Extensions;
using Lively.Common.Services;
using Lively.Core.Display;
using Lively.Core.Linux.Display;
using Lively.Core.Linux.Wallpapers;
using Lively.Factories;
using Lively.Models;
using Lively.Models.Enums;
using System;
using System.IO;
using System.Linq;

namespace Lively.Core.Linux.Backends
{
    /// <summary>
    /// Builds host-process wallpapers (lively-mpv-host / lively-web-host) from user settings.
    /// Shared by the layer-shell backend (desktop rendering) and the Plasma backend (windowed previews).
    /// </summary>
    public sealed class HostWallpaperFactory
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly IUserSettingsService userSettings;
        private readonly IDisplayManager displayManager;
        private readonly ILivelyPropertyFactory propertyFactory;
        private readonly NativeHelperLocator helpers;
        private readonly bool verboseHosts = Environment.GetEnvironmentVariable("LIVELY_HOST_VERBOSE") == "1";

        public HostWallpaperFactory(IUserSettingsService userSettings, IDisplayManager displayManager, ILivelyPropertyFactory propertyFactory, NativeHelperLocator helpers)
        {
            this.userSettings = userSettings;
            this.displayManager = displayManager;
            this.propertyFactory = propertyFactory;
            this.helpers = helpers;
        }

        /// <summary>
        /// Wallpaper types that need a foreign window adopted into the desktop; impossible on Wayland.
        /// </summary>
        public static bool IsProgramWallpaper(WallpaperType type) =>
            type == WallpaperType.app || type == WallpaperType.unity || type == WallpaperType.unityaudio ||
            type == WallpaperType.godot || type == WallpaperType.bizhawk;

        public static void ThrowIfUnsupported(LibraryModel model)
        {
            if (IsProgramWallpaper(model.LivelyInfo.Type))
                throw new WallpaperNotAllowedException(
                    $"'{model.Title}' is a {model.LivelyInfo.Type} wallpaper. Program wallpapers need a window adopted into the desktop, which Wayland does not allow; only video, gif, picture, stream and web wallpapers run on Linux.");
        }

        /// <summary>
        /// Creates one host for one output. <paramref name="span"/> is set for slices of a spanned wallpaper.
        /// </summary>
        public IWallpaper CreateForOutput(LibraryModel model, DisplayMonitor display, WallpaperArrangement arrangement, SpanGeometry? span)
        {
            ThrowIfUnsupported(model);
            var propertyCopy = propertyFactory.CreateLivelyPropertyFolder(model, display, arrangement, userSettings);
            var outputName = LinuxDisplayManager.OutputName(display);

            switch (model.LivelyInfo.Type)
            {
                case WallpaperType.web:
                case WallpaperType.webaudio:
                case WallpaperType.url:
                    return CreateWeb(model, display, propertyCopy, outputName, span, arrangement, windowed: null);
                case WallpaperType.video:
                case WallpaperType.gif:
                case WallpaperType.picture:
                    return CreateMpv(model, display, propertyCopy, outputName, span, windowed: null);
                case WallpaperType.videostream:
                    return HasYoutubeDl()
                        ? CreateMpv(model, display, propertyCopy, outputName, span, windowed: null)
                        : CreateWeb(model, display, propertyCopy, outputName, span, arrangement, windowed: null);
                default:
                    throw new WallpaperPluginException($"No Linux player for wallpaper type {model.LivelyInfo.Type}.");
            }
        }

        /// <summary>
        /// Creates a host that renders in a normal window (wallpaper preview and the add/edit dialogs).
        /// </summary>
        public IWallpaper CreateWindowed(LibraryModel model, DisplayMonitor display)
        {
            ThrowIfUnsupported(model);
            var propertyCopy = propertyFactory.CreateLivelyPropertyFolder(model, display, WallpaperArrangement.per, userSettings);
            var size = PreviewSize(display);

            switch (model.LivelyInfo.Type)
            {
                case WallpaperType.web:
                case WallpaperType.webaudio:
                case WallpaperType.url:
                    return CreateWeb(model, display, propertyCopy, null, null, WallpaperArrangement.per, size);
                case WallpaperType.videostream when !HasYoutubeDl():
                    return CreateWeb(model, display, propertyCopy, null, null, WallpaperArrangement.per, size);
                default:
                    return CreateMpv(model, display, propertyCopy, null, null, size);
            }
        }

        private MpvHostWallpaper CreateMpv(LibraryModel model, DisplayMonitor display, string propertyCopy, string outputName, SpanGeometry? span, MpvHostWallpaper.Size? windowed)
        {
            var options = new MpvHostWallpaper.Options
            {
                HostPath = helpers.Resolve("lively-mpv-host"),
                OutputName = outputName,
                Span = span,
                HwAccel = userSettings.Settings.VideoPlayerHwAccel,
                Scaler = userSettings.Settings.WallpaperScaling,
                StreamQuality = userSettings.Settings.StreamQuality,
                ConfigDir = Path.Combine(Constants.CommonPaths.TempVideoDir, "portable_config"),
                WindowedSize = windowed,
                Verbose = verboseHosts,
            };
            return new MpvHostWallpaper(model.FilePath, model, display, propertyCopy, options);
        }

        private WebHostWallpaper CreateWeb(LibraryModel model, DisplayMonitor display, string propertyCopy, string outputName, SpanGeometry? span, WallpaperArrangement arrangement, MpvHostWallpaper.Size? windowed)
        {
            var options = new WebHostWallpaper.Options
            {
                HostPath = helpers.Resolve("lively-web-host"),
                OutputName = outputName,
                Span = span,
                Interactive = userSettings.Settings.InputForward != InputForwardMode.off && model.LivelyInfo.Type.IsDeviceInputAllowed(),
                DebugPort = userSettings.Settings.WebDebugPort,
                Theme = userSettings.Settings.ApplicationTheme,
                Volume = userSettings.Settings.AudioVolumeGlobal,
                UserDataDir = WebUserDataDir(arrangement, display, windowed != null),
                WindowedSize = windowed,
                Verbose = verboseHosts,
            };
            return new WebHostWallpaper(model.FilePath, model, display, propertyCopy, options);
        }

        private string WebUserDataDir(WallpaperArrangement arrangement, DisplayMonitor display, bool isWindowed)
        {
            // Persistent website data (like the WebView2 user data folder) only when the user opted into disk cache.
            if (userSettings.Settings.CefDiskCache && !isWindowed)
            {
                var key = arrangement == WallpaperArrangement.per ? display.Index.ToString() : arrangement.ToString();
                var dir = Path.Combine(Constants.CommonPaths.AppDataDir, "WebKit", key);
                Directory.CreateDirectory(dir);
                return dir;
            }
            var temp = Path.Combine(Constants.CommonPaths.TempDir, "webkit-" + Path.GetRandomFileName());
            Directory.CreateDirectory(temp);
            return temp;
        }

        private static MpvHostWallpaper.Size PreviewSize(DisplayMonitor display)
        {
            var width = Math.Max(640, Math.Min(1280, display.Bounds.Width * 2 / 3));
            return new MpvHostWallpaper.Size(width, width * 9 / 16);
        }

        private static bool HasYoutubeDl()
        {
            var path = Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
            return path.Split(':', StringSplitOptions.RemoveEmptyEntries)
                .Any(dir => File.Exists(Path.Combine(dir, "yt-dlp")) || File.Exists(Path.Combine(dir, "youtube-dl")));
        }
    }
}
