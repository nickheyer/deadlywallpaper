using Lively.Common;
using Lively.Core.Linux.Hosting;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Message;
using Newtonsoft.Json;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;

namespace Lively.Core.Linux.Wallpapers
{
    /// <summary>
    /// Video, gif, still picture and stream wallpapers rendered by lively-mpv-host.
    /// Mirrors Lively.Core.Wallpapers.VideoMpvPlayer on Windows.
    /// </summary>
    public sealed class MpvHostWallpaper : HostProcessWallpaper
    {
        public sealed class Options
        {
            public string HostPath { get; set; }
            public string OutputName { get; set; }
            public SpanGeometry? Span { get; set; }
            public bool HwAccel { get; set; } = true;
            public WallpaperScaler Scaler { get; set; } = WallpaperScaler.uniformFill;
            public StreamQualitySuggestion StreamQuality { get; set; } = StreamQualitySuggestion.Highest;
            public string ConfigDir { get; set; }
            /// <summary>Windowed preview size; null renders on the desktop layer.</summary>
            public Size? WindowedSize { get; set; }
            public bool Verbose { get; set; }
        }

        public readonly struct Size
        {
            public int Width { get; }
            public int Height { get; }
            public Size(int width, int height) { Width = width; Height = height; }
        }

        private readonly Options options;
        private readonly string filePath;
        private bool isMuted;

        protected override string HostName => "Mpv";

        public MpvHostWallpaper(string filePath, LibraryModel model, DisplayMonitor display, string livelyPropertyCopyPath, Options options)
            : base(model, display, livelyPropertyCopyPath, options.WindowedSize != null)
        {
            this.filePath = filePath;
            this.options = options;
        }

        protected override (string fileName, IReadOnlyList<string> arguments) BuildCommandLine()
        {
            var args = new List<string>();
            if (options.WindowedSize is Size size)
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
            }

            if (!string.IsNullOrEmpty(LivelyPropertyCopyPath))
            {
                args.Add("--property");
                args.Add(LivelyPropertyCopyPath);
            }
            // Startup volume is 0; the playback monitor sets the real volume once running (same as Windows).
            args.Add("--volume");
            args.Add("0");
            args.Add("--hwdec");
            args.Add(options.HwAccel ? "auto-safe" : "no");
            args.Add("--scaler");
            args.Add(ScalerName(options.Scaler));

            switch (Category)
            {
                case WallpaperType.picture:
                    args.Add("--image");
                    break;
                case WallpaperType.videostream:
                    args.Add("--ytdl-format");
                    args.Add(YtdlFormat(options.StreamQuality));
                    break;
            }

            if (!string.IsNullOrEmpty(options.ConfigDir) && Directory.Exists(options.ConfigDir))
            {
                args.Add("--config-dir");
                args.Add(options.ConfigDir);
            }
            if (options.Verbose)
                args.Add("--verbose");

            args.Add(filePath);
            return (options.HostPath, args);
        }

        public override void SetMute(bool mute)
        {
            // Same trick as Windows: "mute" is a Lively property, so switch the audio track instead.
            isMuted = mute;
            SendMessage(new HostMpvCommand("set_property", "aid", mute ? "no" : "1"));
        }

        public override void SetPlaybackPos(float pos, PlaybackPosType type)
        {
            if (Category == WallpaperType.picture)
                return;

            var mode = type == PlaybackPosType.absolutePercent ? "absolute-percent" : "relative-percent";
            SendMessage(new HostMpvCommand("seek", pos, mode));
        }

        public static string ScalerName(WallpaperScaler scaler) => scaler switch
        {
            WallpaperScaler.none => "none",
            WallpaperScaler.fill => "fill",
            WallpaperScaler.uniform => "uniform",
            _ => "uniformFill",
        };

        // Same table as VideoMpvPlayer.GetYtDlMpvArg on Windows.
        public static string YtdlFormat(StreamQualitySuggestion quality) => quality switch
        {
            StreamQualitySuggestion.Lowest => "bestvideo[height<=144]+bestaudio/best",
            StreamQualitySuggestion.Low => "bestvideo[height<=240]+bestaudio/best",
            StreamQualitySuggestion.LowMedium => "bestvideo[height<=360]+bestaudio/best",
            StreamQualitySuggestion.Medium => "bestvideo[height<=480]+bestaudio/best",
            StreamQualitySuggestion.MediumHigh => "bestvideo[height<=720]+bestaudio/best",
            StreamQualitySuggestion.High => "bestvideo[height<=1080]+bestaudio/best",
            _ => "bestvideo+bestaudio/best",
        };
    }
}
