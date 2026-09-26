using Lively.Common.Exceptions;
using Lively.Common.Extensions;
using Lively.Common.Helpers;
using Lively.Core.Linux.Hosting;
using Lively.Core.Linux.Wallpapers;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.LivelyControls;
using Lively.Models.Message;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Plasma
{
    /// <summary>
    /// A wallpaper rendered by the Plasma wallpaper plugin inside plasmashell's desktop window,
    /// controlled over the WebSocket channel (PROTOCOL.md section 7).
    /// </summary>
    public sealed class PlasmaWallpaper : IWallpaper
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private static int globalCount;

        private readonly PlasmaShellScripting shell;
        private readonly WallpaperSocketServer server;
        private readonly PlasmaDesktopRegistry registry;
        private readonly Backends.PlasmaDesktopCoordinator coordinator;
        private int desktopId;
        private string previousPlugin;
        private bool desktopResolved;
        private readonly SpanGeometry? span;
        private readonly WallpaperScaler scaler;
        private readonly bool interactive;
        private readonly int uniqueId = Interlocked.Increment(ref globalCount);
        private readonly TaskCompletionSource<Exception> loadedTcs = new TaskCompletionSource<Exception>(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly List<Action<IpcMessage>> listeners = new List<Action<IpcMessage>>();
        private readonly string instance = Guid.NewGuid().ToString("N");
        private WallpaperConnection connection;
        private int currentVolume;
        private bool isMuted;
        private bool isPaused;
        private bool closed;

        public event EventHandler Exited;
        public event EventHandler Loaded;

        public bool IsExited { get; private set; }
        public bool IsLoaded { get; private set; }
        public WallpaperType Category => Model.LivelyInfo.Type;
        public LibraryModel Model { get; }
        public IntPtr Handle => IntPtr.Zero;
        public IntPtr InputHandle => IntPtr.Zero;
        public int? Pid => null;
        public DisplayMonitor Screen { get; set; }
        public string LivelyPropertyCopyPath { get; }

        public bool WantsSystemInformation { get; }
        public bool WantsNowPlaying { get; }
        public bool WantsAudio => Category == WallpaperType.webaudio;

        public PlasmaWallpaper(LibraryModel model, DisplayMonitor display, string livelyPropertyCopyPath,
            PlasmaShellScripting shell, WallpaperSocketServer server, PlasmaDesktopRegistry registry,
            Backends.PlasmaDesktopCoordinator coordinator, SpanGeometry? span, WallpaperScaler scaler, int volume, bool interactive)
        {
            Model = model;
            Screen = display;
            LivelyPropertyCopyPath = livelyPropertyCopyPath;
            this.shell = shell;
            this.server = server;
            this.registry = registry;
            this.coordinator = coordinator;
            this.span = span;
            this.scaler = scaler;
            this.interactive = interactive;
            currentVolume = volume;

            var args = WallpaperArguments.Parse(model.LivelyInfo.Arguments);
            WantsSystemInformation = args.SystemInformation;
            WantsNowPlaying = args.NowPlaying;
        }

        public async Task ShowAsync()
        {
            // The containment covering the display is looked up now, on the D-Bus thread pool, never
            // synchronously inside the factory.
            var desktops = await shell.ListDesktopsAsync();
            var desktop = Backends.PlasmaBackend.FindDesktop(desktops, Screen)
                ?? throw new ScreenNotFoundException($"No Plasma desktop containment covers display {Screen.DeviceName} ({Screen.Bounds}). Plasma desktops: {string.Join(", ", desktops.Select(d => $"#{d.Id}@{d.Geometry}"))}");
            desktopId = desktop.Id;
            previousPlugin = registry.PreviousPluginFor(desktop.Id, desktop.WallpaperPlugin);
            desktopResolved = true;

            // A wallpaper closed on this desktop a moment ago is still restoring the plugin; apply after it.
            await coordinator.WaitForDesktopAsync(desktopId);

            server.Register(instance, OnConnected);
            registry.Remember(desktopId, previousPlugin);

            var config = new Dictionary<string, object>
            {
                ["Source"] = Model.FilePath,
                ["Kind"] = KindName(Category),
                ["CoreSocket"] = server.Url,
                ["Instance"] = instance,
                ["Scaler"] = MpvHostWallpaper.ScalerName(scaler),
                ["Volume"] = currentVolume,
                ["Interactive"] = interactive,
                ["SpanX"] = span?.X ?? 0,
                ["SpanY"] = span?.Y ?? 0,
                ["SpanWidth"] = span?.Width ?? 0,
                ["SpanHeight"] = span?.Height ?? 0,
                ["SpanVirtualWidth"] = span?.VirtualWidth ?? 0,
                ["SpanVirtualHeight"] = span?.VirtualHeight ?? 0,
            };
            Logger.Info($"Plasma{uniqueId}: applying {Model.Title} to desktop {desktopId} (instance {instance})");
            await shell.ApplyAsync(desktopId, config);

            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(30));
            using (cts.Token.Register(() => loadedTcs.TrySetResult(new WallpaperPluginException(
                "The Plasma wallpaper plugin did not connect within 30 seconds. Check that com.lively.wallpaper is installed under ~/.local/share/plasma/wallpapers (make plasma-install) and that qt6-multimedia / qt6-webengine are present."))))
            {
                var error = await loadedTcs.Task;
                if (error != null)
                {
                    await RestoreAsync();
                    throw error;
                }
            }
        }

        private void OnConnected(WallpaperConnection conn)
        {
            var previous = Interlocked.Exchange(ref connection, conn);
            previous?.Close();
            Logger.Info($"Plasma{uniqueId}: plugin connected");
            conn.MessageReceived += (s, text) => HandleMessage(text);
            conn.Closed += (s, e) =>
            {
                if (closed)
                    return;
                // plasmashell restarts reconnect on their own; the wallpaper stays configured.
                Logger.Warn($"Plasma{uniqueId}: plugin connection closed; waiting for it to reconnect.");
            };
        }

        private void HandleMessage(string text)
        {
            var msg = HostProtocol.TryParse(text);
            if (msg == null)
            {
                Logger.Info($"Plasma{uniqueId}: {text}");
                return;
            }

            switch (msg)
            {
                case LivelyMessageConsole console:
                    if (console.Category == ConsoleMessageType.error)
                        Logger.Error($"Plasma{uniqueId}: {console.Message}");
                    else
                        Logger.Info($"Plasma{uniqueId}: {console.Message}");
                    break;
                case LivelyMessageWallpaperLoaded loaded:
                    if (loaded.Success)
                    {
                        if (!IsLoaded)
                        {
                            IsLoaded = true;
                            SendLivelyProperties();
                            loadedTcs.TrySetResult(null);
                            Loaded?.Invoke(this, EventArgs.Empty);
                        }
                        else
                        {
                            // Reconnected after a plasmashell restart: put the state back.
                            SendLivelyProperties();
                            if (isPaused) Send(new LivelySuspendCmd());
                            Send(new LivelyVolumeCmd { Volume = isMuted ? 0 : currentVolume });
                        }
                    }
                    else
                    {
                        loadedTcs.TrySetResult(new WallpaperFileException("The Plasma wallpaper plugin could not load the wallpaper."));
                    }
                    break;
            }

            Action<IpcMessage>[] snapshot;
            lock (listeners)
                snapshot = listeners.ToArray();
            foreach (var listener in snapshot)
                listener(msg);
        }

        /// <summary>The plugin does not read LivelyProperties.json itself; the core replays the saved values.</summary>
        private void SendLivelyProperties()
        {
            if (string.IsNullOrEmpty(LivelyPropertyCopyPath) || !File.Exists(LivelyPropertyCopyPath))
                return;
            try
            {
                LivelyPropertyUtil.LoadProperty(LivelyPropertyCopyPath, control =>
                {
                    IpcMessage msg = control switch
                    {
                        SliderModel s => new LivelySlider { Name = s.Name, Value = s.Value, Step = s.Step },
                        CheckboxModel c => new LivelyCheckbox { Name = c.Name, Value = c.Value },
                        TextboxModel t => new LivelyTextBox { Name = t.Name, Value = t.Value },
                        DropdownModel d => new LivelyDropdown { Name = d.Name, Value = d.Value },
                        ColorPickerModel cp => new LivelyColorPicker { Name = cp.Name, Value = cp.Value },
                        ScalerDropdownModel sd => new LivelyDropdownScaler { Name = sd.Name, Value = sd.Value },
                        FolderDropdownModel fd => new LivelyFolderDropdown { Name = fd.Name, Value = fd.Value },
                        _ => null,
                    };
                    if (msg != null)
                        Send(msg);
                });
            }
            catch (Exception ex)
            {
                Logger.Error($"Plasma{uniqueId}: failed to replay LivelyProperties: {ex.Message}");
            }
        }

        private void Send(IpcMessage msg)
        {
            var conn = connection;
            if (conn == null || !conn.IsOpen || IsExited)
                return;
            _ = conn.SendAsync(HostProtocol.Serialize(msg));
        }

        public void SendMessage(IpcMessage obj)
        {
            Send(obj);
            if (obj is LivelyButton button && button.IsDefault)
                SendLivelyProperties();
        }

        public void Pause()
        {
            if (isPaused) return;
            isPaused = true;
            Send(new LivelySuspendCmd());
        }

        public void Play()
        {
            if (!isPaused) return;
            isPaused = false;
            Send(new LivelyResumeCmd());
        }

        public void SetVolume(int volume)
        {
            currentVolume = volume;
            if (!isMuted)
                Send(new LivelyVolumeCmd { Volume = volume });
        }

        public void SetMute(bool mute)
        {
            isMuted = mute;
            Send(new LivelyVolumeCmd { Volume = mute ? 0 : currentVolume });
        }

        public void SetPlaybackPos(float pos, PlaybackPosType type)
        {
            if (Category.IsWebWallpaper() || Category == WallpaperType.url)
            {
                if (pos == 0 && type != PlaybackPosType.relativePercent)
                    Send(new LivelyReloadCmd());
                return;
            }
            if (Category == WallpaperType.picture)
                return;
            var mode = type == PlaybackPosType.absolutePercent ? "absolute-percent" : "relative-percent";
            Send(new HostMpvCommand("seek", pos, mode));
        }

        public async Task ScreenCapture(string filePath)
        {
            var target = Path.GetExtension(filePath) != ".jpg" ? filePath + ".jpg" : filePath;
            var tcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            void Listener(IpcMessage msg)
            {
                if (msg is LivelyMessageScreenshot shot && shot.FileName == Path.GetFileName(target))
                    tcs.TrySetResult(shot.Success);
            }
            lock (listeners) listeners.Add(Listener);
            try
            {
                Send(new LivelyScreenshotCmd { FilePath = target, Format = ScreenshotFormat.jpeg, Delay = 0 });
                using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10));
                using (cts.Token.Register(() => tcs.TrySetException(new TimeoutException("Screenshot timed out."))))
                {
                    if (!await tcs.Task)
                        throw new InvalidOperationException("The Plasma wallpaper plugin failed to take a screenshot.");
                }
            }
            finally
            {
                lock (listeners) listeners.Remove(Listener);
            }
        }

        public void Close()
        {
            if (closed) return;
            closed = true;
            Send(new LivelyCloseCmd());
            var restore = RestoreAsync();
            if (desktopResolved)
                coordinator.TrackClose(desktopId, restore);
        }

        public void Terminate() => Close();

        private async Task RestoreAsync()
        {
            server.Unregister(instance);
            connection?.Close();
            if (!desktopResolved)
            {
                // ShowAsync never reached the desktop: nothing was changed on the shell.
                IsExited = true;
                Exited?.Invoke(this, EventArgs.Empty);
                return;
            }
            try
            {
                await shell.RestorePluginAsync(desktopId, previousPlugin);
                registry.Forget(desktopId);
            }
            catch (Exception ex)
            {
                Logger.Error($"Plasma{uniqueId}: failed to restore previous wallpaper plugin '{previousPlugin}' on desktop {desktopId}: {ex.Message}");
            }
            finally
            {
                if (!IsExited)
                {
                    IsExited = true;
                    Exited?.Invoke(this, EventArgs.Empty);
                }
            }
        }

        public void Dispose() => Close();

        public static string KindName(WallpaperType type) => type switch
        {
            WallpaperType.video => "video",
            WallpaperType.gif => "gif",
            WallpaperType.picture => "picture",
            WallpaperType.videostream => "videostream",
            WallpaperType.url => "url",
            WallpaperType.web => "web",
            WallpaperType.webaudio => "web",
            _ => "none",
        };
    }

    /// <summary>
    /// Remembers which desktops Lively switched and what plugin they had, persisted to disk so a
    /// crashed core can put the desktop back on the next start.
    /// </summary>
    public sealed class PlasmaDesktopRegistry
    {
        private readonly string path;
        private readonly object sync = new object();
        private readonly Dictionary<int, string> previous = new Dictionary<int, string>();

        public PlasmaDesktopRegistry(string path)
        {
            this.path = path;
            if (File.Exists(path))
            {
                var loaded = Newtonsoft.Json.JsonConvert.DeserializeObject<Dictionary<int, string>>(File.ReadAllText(path));
                if (loaded != null)
                    foreach (var kv in loaded)
                        previous[kv.Key] = kv.Value;
            }
        }

        public IReadOnlyDictionary<int, string> Snapshot()
        {
            lock (sync) return new Dictionary<int, string>(previous);
        }

        public void Remember(int desktopId, string plugin)
        {
            lock (sync)
            {
                if (!previous.ContainsKey(desktopId) && plugin != PlasmaShellScripting.PluginId)
                    previous[desktopId] = plugin;
                Save();
            }
        }

        public string PreviousPluginFor(int desktopId, string currentPlugin)
        {
            lock (sync)
                return previous.TryGetValue(desktopId, out var p) ? p : currentPlugin;
        }

        public void Forget(int desktopId)
        {
            lock (sync)
            {
                previous.Remove(desktopId);
                Save();
            }
        }

        private void Save()
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path));
            if (previous.Count == 0)
            {
                if (File.Exists(path)) File.Delete(path);
                return;
            }
            File.WriteAllText(path, Newtonsoft.Json.JsonConvert.SerializeObject(previous));
        }
    }
}
