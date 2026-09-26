using Lively.Common.Services;
using Lively.Core.Linux.Plasma;
using Lively.Core.Linux.Wallpapers;
using Lively.Models.Message;
using Lively.Models.Services;
using System;
using System.Collections.Generic;
using System.Linq;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// On Windows every web player captures audio / hardware / now-playing data itself. On Linux the
    /// core collects them once and pushes lsp_* messages to the wallpapers that asked for them.
    /// </summary>
    public sealed class WallpaperFeedService : IDisposable
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly IDesktopCore desktopCore;
        private readonly IAudioVisualizerService audio;
        private readonly IHardwareUsageService hardware;
        private readonly INowPlayingService nowPlaying;
        private readonly IUserSettingsService userSettings;
        private readonly object sync = new object();
        private bool audioRunning, hardwareRunning, nowPlayingRunning;
        private bool disposed;

        public WallpaperFeedService(IDesktopCore desktopCore, IAudioVisualizerService audio, IHardwareUsageService hardware,
            INowPlayingService nowPlaying, IUserSettingsService userSettings)
        {
            this.desktopCore = desktopCore;
            this.audio = audio;
            this.hardware = hardware;
            this.nowPlaying = nowPlaying;
            this.userSettings = userSettings;

            audio.AudioDataAvailable += Audio_AudioDataAvailable;
            hardware.HWMonitor += Hardware_HWMonitor;
            nowPlaying.NowPlayingTrackChanged += NowPlaying_TrackChanged;
            desktopCore.WallpaperChanged += (s, e) => UpdateSubscriptions();
        }

        private static IEnumerable<IWallpaper> Leaves(IEnumerable<IWallpaper> wallpapers)
        {
            foreach (var w in wallpapers)
            {
                if (w is CompositeWallpaper composite)
                    foreach (var part in composite.Parts)
                        yield return part;
                else
                    yield return w;
            }
        }

        private static bool Wants(IWallpaper w, Func<WebHostWallpaper, bool> web, Func<PlasmaWallpaper, bool> plasma) => w switch
        {
            WebHostWallpaper h => web(h),
            PlasmaWallpaper p => plasma(p),
            _ => false,
        };

        private List<IWallpaper> Consumers(Func<WebHostWallpaper, bool> web, Func<PlasmaWallpaper, bool> plasma) =>
            Leaves(desktopCore.Wallpapers).Where(w => w.IsLoaded && !w.IsExited && Wants(w, web, plasma)).ToList();

        private void UpdateSubscriptions()
        {
            lock (sync)
            {
                if (disposed) return;
                var wantAudio = Consumers(w => w.WantsAudio, p => p.WantsAudio).Count > 0;
                var wantHardware = Consumers(w => w.WantsSystemInformation, p => p.WantsSystemInformation).Count > 0;
                var wantNowPlaying = Consumers(w => w.WantsNowPlaying, p => p.WantsNowPlaying).Count > 0;

                Toggle(ref audioRunning, wantAudio, () => audio.Start(userSettings.Settings.VisualizerAudioDeviceId), audio.Stop, "audio visualizer");
                Toggle(ref hardwareRunning, wantHardware, hardware.Start, hardware.Stop, "hardware usage");
                Toggle(ref nowPlayingRunning, wantNowPlaying, nowPlaying.Start, nowPlaying.Stop, "now playing");
            }
        }

        private static void Toggle(ref bool running, bool wanted, Action start, Action stop, string name)
        {
            if (wanted == running)
                return;
            try
            {
                if (wanted) start(); else stop();
                running = wanted;
                Logger.Info($"{name} feed {(wanted ? "started" : "stopped")}");
            }
            catch (Exception ex)
            {
                Logger.Error($"Failed to {(wanted ? "start" : "stop")} the {name} feed: {ex.Message}");
            }
        }

        private void Audio_AudioDataAvailable(object sender, double[] data)
        {
            var msg = new LivelySystemAudio { Data = data };
            foreach (var w in Consumers(w => w.WantsAudio, p => p.WantsAudio))
                w.SendMessage(msg);
        }

        private void Hardware_HWMonitor(object sender, HardwareUsageEventArgs e)
        {
            var msg = new LivelySystemInformation { Info = e };
            foreach (var w in Consumers(w => w.WantsSystemInformation, p => p.WantsSystemInformation))
                w.SendMessage(msg);
        }

        private void NowPlaying_TrackChanged(object sender, NowPlayingEventArgs e)
        {
            var msg = new LivelySystemNowPlaying { Info = e };
            foreach (var w in Consumers(w => w.WantsNowPlaying, p => p.WantsNowPlaying))
                w.SendMessage(msg);
        }

        public void Dispose()
        {
            lock (sync)
            {
                if (disposed) return;
                disposed = true;
                audio.AudioDataAvailable -= Audio_AudioDataAvailable;
                hardware.HWMonitor -= Hardware_HWMonitor;
                nowPlaying.NowPlayingTrackChanged -= NowPlaying_TrackChanged;
                if (audioRunning) audio.Stop();
                if (hardwareRunning) hardware.Stop();
                if (nowPlayingRunning) nowPlaying.Stop();
            }
        }
    }
}
