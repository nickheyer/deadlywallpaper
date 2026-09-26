using Lively.Common.Extensions;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Message;
using System.Collections.Generic;
using System.Globalization;

namespace Lively.Core.Linux.Wallpapers
{
    /// <summary>
    /// HTML / URL wallpapers rendered by lively-web-host (WebKitGTK on a layer-shell surface).
    /// Mirrors Lively.Core.Wallpapers.WebWebView2 on Windows.
    /// </summary>
    public sealed class WebHostWallpaper : HostProcessWallpaper
    {
        public sealed class Options
        {
            public string HostPath { get; set; }
            public string OutputName { get; set; }
            public SpanGeometry? Span { get; set; }
            public bool Interactive { get; set; }
            public string DebugPort { get; set; }
            public AppTheme Theme { get; set; } = AppTheme.Dark;
            public int Volume { get; set; }
            public string UserDataDir { get; set; }
            public MpvHostWallpaper.Size? WindowedSize { get; set; }
            public bool Verbose { get; set; }
        }

        private readonly Options options;
        private readonly string path;
        private readonly WallpaperArguments arguments;
        private int currentVolume;
        private bool isMuted;

        protected override string HostName => "Web";

        /// <summary>Which live data feeds this wallpaper asked for (PROTOCOL.md lsp_* messages).</summary>
        public bool WantsSystemInformation => arguments.SystemInformation;
        public bool WantsNowPlaying => arguments.NowPlaying;
        public bool WantsAudio => Category == WallpaperType.webaudio;

        public WebHostWallpaper(string path, LibraryModel model, DisplayMonitor display, string livelyPropertyCopyPath, Options options)
            : base(model, display, livelyPropertyCopyPath, options.WindowedSize != null)
        {
            this.path = path;
            this.options = options;
            arguments = WallpaperArguments.Parse(model.LivelyInfo.Arguments);
            currentVolume = options.Volume;
        }

        protected override (string fileName, IReadOnlyList<string> arguments) BuildCommandLine()
        {
            var args = new List<string>();
            if (options.WindowedSize is MpvHostWallpaper.Size size)
            {
                args.Add("--windowed");
                args.Add(string.Format(CultureInfo.InvariantCulture, "{0}x{1}", size.Width, size.Height));
                args.Add("--title");
                args.Add($"{Model.Title} - Lively Wallpaper");
            }
            else
            {
                args.Add("--output");
                args.Add(options.OutputName);
                if (options.Span is SpanGeometry span)
                {
                    args.Add("--span");
                    args.Add(span.ToArgument());
                }
                if (options.Interactive)
                    args.Add("--interactive");
            }

            if (!string.IsNullOrEmpty(LivelyPropertyCopyPath))
            {
                args.Add("--property");
                args.Add(LivelyPropertyCopyPath);
            }
            args.Add("--volume");
            args.Add(options.Volume.ToString(CultureInfo.InvariantCulture));
            args.Add("--type");
            args.Add(Category.IsOnlineWallpaper() ? "online" : "local");
            args.Add("--color-scheme");
            args.Add(options.Theme == AppTheme.Light ? "light" : "dark");
            if (!string.IsNullOrWhiteSpace(options.DebugPort) && int.TryParse(options.DebugPort, out _))
            {
                args.Add("--debug");
                args.Add(options.DebugPort);
            }
            if (!string.IsNullOrEmpty(options.UserDataDir))
            {
                args.Add("--user-data");
                args.Add(options.UserDataDir);
            }
            if (arguments.PauseMedia || Category == WallpaperType.videostream)
                args.Add("--pause-media");
            if (arguments.PauseEvent)
                args.Add("--pause-event");
            if (arguments.SystemInformation)
                args.Add("--sysinfo");
            if (arguments.NowPlaying)
                args.Add("--nowplaying");
            if (WantsAudio)
                args.Add("--audio");
            if (options.Verbose || arguments.VerboseLog)
                args.Add("--verbose");

            args.Add(path);
            return (options.HostPath, args);
        }

        public override void SetVolume(int volume)
        {
            currentVolume = volume;
            if (!isMuted)
                base.SetVolume(volume);
        }

        public override void SetMute(bool mute)
        {
            // Web hosts only have a mute switch, so mute is modelled as volume 0 (same as WebView2).
            isMuted = mute;
            base.SetVolume(mute ? 0 : currentVolume);
        }

        public override void SetPlaybackPos(float pos, PlaybackPosType type)
        {
            if (pos == 0 && type != PlaybackPosType.relativePercent)
                SendMessage(new LivelyReloadCmd());
        }
    }
}
