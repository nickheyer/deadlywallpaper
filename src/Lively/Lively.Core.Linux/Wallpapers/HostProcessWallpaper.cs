using Lively.Common.Exceptions;
using Lively.Core.Linux.Hosting;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Message;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Wallpapers
{
    /// <summary>
    /// IWallpaper implemented by a native host process speaking the JSON-lines protocol of
    /// src/native/PROTOCOL.md over stdin/stdout. Subclasses build the command line and map
    /// Lively property messages to host commands.
    /// </summary>
    public abstract class HostProcessWallpaper : IWallpaper
    {
        protected static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private static int globalCount;

        private readonly TaskCompletionSource<Exception> startupTcs = new TaskCompletionSource<Exception>(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource<bool> loadedTcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly object stdinLock = new object();
        private readonly List<Action<IpcMessage>> listeners = new List<Action<IpcMessage>>();
        private Process process;
        private int? exitCode;
        private int closeRequested;
        private bool isPaused;

        protected readonly int UniqueId = Interlocked.Increment(ref globalCount);
        protected abstract string HostName { get; }

        public event EventHandler Exited;
        public event EventHandler Loaded;

        public bool IsExited { get; private set; }
        public bool IsLoaded { get; private set; }
        public WallpaperType Category => Model.LivelyInfo.Type;
        public LibraryModel Model { get; }
        public IntPtr Handle => IntPtr.Zero;
        public IntPtr InputHandle => IntPtr.Zero;
        public int? Pid { get; private set; }
        public DisplayMonitor Screen { get; set; }
        public string LivelyPropertyCopyPath { get; }
        /// <summary>True when the host runs in a normal window (preview/edit dialogs) instead of the desktop layer.</summary>
        public bool IsWindowed { get; }

        protected HostProcessWallpaper(LibraryModel model, DisplayMonitor display, string livelyPropertyCopyPath, bool isWindowed)
        {
            Model = model;
            Screen = display;
            LivelyPropertyCopyPath = livelyPropertyCopyPath;
            IsWindowed = isWindowed;
        }

        /// <summary>Executable path and the argument list for the host.</summary>
        protected abstract (string fileName, IReadOnlyList<string> arguments) BuildCommandLine();

        /// <summary>Exception to surface when the host exits before it reported the wallpaper as loaded.</summary>
        protected virtual Exception MapExitCode(int code)
        {
            return code switch
            {
                2 => new WallpaperPluginException($"{HostName}: bad arguments (exit 2)."),
                3 => new WallpaperPluginException($"{HostName}: the compositor does not provide a required Wayland protocol (exit 3)."),
                4 => new ScreenNotFoundException($"{HostName}: output {Screen?.DeviceId} not found (exit 4)."),
                5 => new WallpaperPluginException($"{HostName}: renderer initialisation failed (exit 5)."),
                6 => new WallpaperFileException($"{HostName}: the wallpaper could not be loaded (exit 6)."),
                _ => new WallpaperPluginException($"{HostName}: exited unexpectedly with code {code}."),
            };
        }

        public async Task ShowAsync()
        {
            var (fileName, arguments) = BuildCommandLine();
            process = new Process
            {
                EnableRaisingEvents = true,
                StartInfo = new ProcessStartInfo
                {
                    FileName = fileName,
                    UseShellExecute = false,
                    RedirectStandardInput = true,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    StandardInputEncoding = new UTF8Encoding(false),
                    StandardOutputEncoding = Encoding.UTF8,
                    StandardErrorEncoding = Encoding.UTF8,
                    WorkingDirectory = Path.GetDirectoryName(fileName) ?? AppContext.BaseDirectory,
                }
            };
            foreach (var arg in arguments)
                process.StartInfo.ArgumentList.Add(arg);

            process.Exited += Process_Exited;
            process.OutputDataReceived += Process_OutputDataReceived;
            process.ErrorDataReceived += (s, e) => { if (!string.IsNullOrEmpty(e.Data)) Logger.Info($"{HostName}{UniqueId} stderr: {e.Data}"); };

            Logger.Info($"{HostName}{UniqueId}: starting {fileName} {string.Join(" ", arguments)}");
            try
            {
                process.Start();
            }
            catch (Exception ex)
            {
                throw new WallpaperPluginNotFoundException($"Failed to start {fileName}: {ex.Message}", ex);
            }
            Pid = process.Id;
            process.BeginOutputReadLine();
            process.BeginErrorReadLine();

            // Wait for msg_wploaded (or an early exit). The hosts also send msg_hwnd first; we only need loaded.
            var error = await startupTcs.Task;
            if (error != null)
            {
                Terminate();
                throw error;
            }
        }

        private void Process_Exited(object sender, EventArgs e)
        {
            exitCode = process?.ExitCode;
            Logger.Info($"{HostName}{UniqueId}: exited with code {exitCode}");
            IsExited = true;
            if (!IsLoaded)
                startupTcs.TrySetResult(MapExitCode(exitCode ?? -1));
            loadedTcs.TrySetResult(false);
            process?.Dispose();
            Exited?.Invoke(this, EventArgs.Empty);
        }

        private void Process_OutputDataReceived(object sender, DataReceivedEventArgs e)
        {
            if (string.IsNullOrEmpty(e.Data))
                return;

            var msg = HostProtocol.TryParse(e.Data);
            if (msg == null)
            {
                Logger.Info($"{HostName}{UniqueId}: {e.Data}");
                return;
            }

            switch (msg)
            {
                case LivelyMessageConsole console:
                    if (console.Category == ConsoleMessageType.error)
                        Logger.Error($"{HostName}{UniqueId}: {console.Message}");
                    else
                        Logger.Info($"{HostName}{UniqueId}: {console.Message}");
                    break;
                case LivelyMessageHwnd _:
                    Logger.Info($"{HostName}{UniqueId}: host is up");
                    break;
                case LivelyMessageWallpaperLoaded loaded:
                    if (loaded.Success)
                    {
                        IsLoaded = true;
                        OnLoaded();
                        loadedTcs.TrySetResult(true);
                        startupTcs.TrySetResult(null);
                        Loaded?.Invoke(this, EventArgs.Empty);
                    }
                    else
                    {
                        startupTcs.TrySetResult(new WallpaperFileException($"{HostName}: wallpaper failed to load."));
                    }
                    break;
            }

            Action<IpcMessage>[] snapshot;
            lock (listeners)
                snapshot = listeners.ToArray();
            foreach (var listener in snapshot)
                listener(msg);
        }

        /// <summary>Called once when the host reports the content as loaded (property restoration happens here).</summary>
        protected virtual void OnLoaded() { }

        protected IDisposable Subscribe(Action<IpcMessage> listener)
        {
            lock (listeners)
                listeners.Add(listener);
            return new Unsubscriber(() => { lock (listeners) listeners.Remove(listener); });
        }

        private sealed class Unsubscriber : IDisposable
        {
            private Action action;
            public Unsubscriber(Action action) => this.action = action;
            public void Dispose() { action?.Invoke(); action = null; }
        }

        /// <summary>Writes one protocol line to the host. Silently dropped after exit (the host is gone).</summary>
        public void SendMessage(IpcMessage obj)
        {
            if (IsExited || process == null)
                return;

            var line = HostProtocol.Serialize(obj);
            try
            {
                lock (stdinLock)
                {
                    process.StandardInput.WriteLine(line);
                    process.StandardInput.Flush();
                }
            }
            catch (IOException ex)
            {
                Logger.Error($"{HostName}{UniqueId}: stdin write failed: {ex.Message}");
            }
            catch (ObjectDisposedException)
            {
                // Process exited between the check and the write.
            }
        }

        /// <summary>
        /// Asks the host to exit (cmd_close) and kills it if it is still there after the grace period.
        /// </summary>
        public void Close()
        {
            if (IsExited || Interlocked.Exchange(ref closeRequested, 1) != 0)
                return;
            SendMessage(new LivelyCloseCmd());
            // Give the host a moment to exit gracefully, then make sure.
            _ = Task.Run(async () =>
            {
                await Task.Delay(1500);
                if (!IsExited)
                    Terminate();
            });
        }

        public void Terminate()
        {
            if (IsExited)
                return;
            try
            {
                process?.Kill(entireProcessTree: true);
            }
            catch (InvalidOperationException)
            {
                // Already gone, or exited and disposed meanwhile (ObjectDisposedException derives from this).
            }
        }

        public virtual void Pause()
        {
            if (isPaused)
                return;
            isPaused = true;
            SendMessage(new LivelySuspendCmd());
        }

        public virtual void Play()
        {
            if (!isPaused)
                return;
            isPaused = false;
            SendMessage(new LivelyResumeCmd());
        }

        public virtual void SetVolume(int volume) => SendMessage(new LivelyVolumeCmd { Volume = volume });

        public abstract void SetMute(bool mute);

        public abstract void SetPlaybackPos(float pos, PlaybackPosType type);

        public async Task ScreenCapture(string filePath)
        {
            var target = Path.GetExtension(filePath) != ".jpg" ? filePath + ".jpg" : filePath;
            var tcs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            using var subscription = Subscribe(msg =>
            {
                if (msg is LivelyMessageScreenshot shot && shot.FileName == Path.GetFileName(target))
                    tcs.TrySetResult(shot.Success);
            });

            Logger.Info($"{HostName}{UniqueId}: taking screenshot {target}");
            SendMessage(new LivelyScreenshotCmd { FilePath = target, Format = ScreenshotFormat.jpeg, Delay = 0 });

            using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(10));
            using (cts.Token.Register(() => tcs.TrySetException(new TimeoutException("Screenshot timed out."))))
            {
                if (!await tcs.Task)
                    throw new InvalidOperationException("The wallpaper host failed to take a screenshot.");
            }
        }

        /// <summary>
        /// Same as <see cref="Close"/>: the host gets cmd_close and the grace period before it is killed.
        /// </summary>
        public void Dispose()
        {
            Close();
        }
    }
}
