using Newtonsoft.Json;
using Newtonsoft.Json.Linq;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Display
{
    /// <summary>
    /// One Wayland output as reported by lively-wl-monitor (PROTOCOL.md section 6).
    /// </summary>
    public sealed class WaylandOutput
    {
        [JsonProperty("name")] public string Name { get; set; }
        [JsonProperty("description")] public string Description { get; set; }
        [JsonProperty("make")] public string Make { get; set; }
        [JsonProperty("model")] public string Model { get; set; }
        [JsonProperty("x")] public int X { get; set; }
        [JsonProperty("y")] public int Y { get; set; }
        [JsonProperty("width")] public int Width { get; set; }
        [JsonProperty("height")] public int Height { get; set; }
        [JsonProperty("scale")] public double Scale { get; set; }
        [JsonProperty("transform")] public int Transform { get; set; }
        [JsonProperty("physical_width_mm")] public int PhysicalWidthMm { get; set; }
        [JsonProperty("physical_height_mm")] public int PhysicalHeightMm { get; set; }
        [JsonProperty("refresh_mhz")] public int RefreshMilliHz { get; set; }
    }

    /// <summary>
    /// One toplevel window as reported by lively-wl-monitor.
    /// </summary>
    public sealed class WaylandToplevel
    {
        [JsonProperty("id")] public string Id { get; set; }
        [JsonProperty("app_id")] public string AppId { get; set; }
        [JsonProperty("title")] public string Title { get; set; }
        [JsonProperty("activated")] public bool Activated { get; set; }
        [JsonProperty("fullscreen")] public bool Fullscreen { get; set; }
        [JsonProperty("maximized")] public bool Maximized { get; set; }
        [JsonProperty("minimized")] public bool Minimized { get; set; }
        [JsonProperty("skip_taskbar")] public bool SkipTaskbar { get; set; }
        [JsonProperty("outputs")] public List<string> Outputs { get; set; } = new List<string>();
        /// <summary>[x, y, w, h] in logical coordinates, or null when the protocol has no geometry.</summary>
        [JsonProperty("geometry")] public int[] Geometry { get; set; }
        /// <summary>Ids of the virtual desktops the window is on; empty means every desktop.</summary>
        [JsonProperty("virtual_desktops")] public List<string> VirtualDesktops { get; set; } = new List<string>();
        /// <summary>Ids of the activities the window is on; empty means every activity.</summary>
        [JsonProperty("activities")] public List<string> Activities { get; set; } = new List<string>();
        /// <summary>True when the window is on every desktop or on a currently activated one.</summary>
        [JsonProperty("on_current_desktop")] public bool OnCurrentDesktop { get; set; } = true;
    }

    public sealed class WaylandCapabilities
    {
        [JsonProperty("layer_shell")] public bool LayerShell { get; set; }
        [JsonProperty("plasma_shell")] public bool PlasmaShell { get; set; }
        [JsonProperty("toplevel_protocol")] public string ToplevelProtocol { get; set; } = "none";

        public bool HasToplevelTracking => ToplevelProtocol == "wlr" || ToplevelProtocol == "plasma";
    }

    /// <summary>
    /// Owns the lively-wl-monitor helper process and exposes its output and window state stream.
    /// A helper that exits on its own (compositor hiccup, crash) is started again with an exponential
    /// back-off, and its fresh capabilities, outputs and windows replace the stale ones.
    /// </summary>
    public sealed class WaylandMonitorService : IDisposable
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private static readonly TimeSpan MaxRestartDelay = TimeSpan.FromSeconds(10);

        private readonly string helperPath;
        private readonly object sync = new object();
        private readonly TaskCompletionSource<bool> firstOutputs = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource<bool> firstCapabilities = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
        private Process process;
        private bool disposed;
        private int restartAttempts;

        public IReadOnlyList<WaylandOutput> Outputs { get; private set; } = Array.Empty<WaylandOutput>();
        public IReadOnlyList<WaylandToplevel> Toplevels { get; private set; } = Array.Empty<WaylandToplevel>();
        public WaylandCapabilities Capabilities { get; private set; } = new WaylandCapabilities();
        /// <summary>KWin's "show desktop" mode: the desktop is raised above every window.</summary>
        public bool IsShowDesktopActive { get; private set; }

        public event EventHandler OutputsChanged;
        public event EventHandler ToplevelsChanged;
        /// <summary>Raised when the helper exited on its own; the service starts it again.</summary>
        public event EventHandler<int> HelperExited;
        /// <summary>Raised once a restarted helper reported its outputs again.</summary>
        public event EventHandler HelperRestarted;

        public WaylandMonitorService(NativeHelperLocator helpers)
        {
            helperPath = helpers.Resolve("lively-wl-monitor");
        }

        /// <summary>
        /// Starts the helper and waits until the first capabilities and outputs lines arrived.
        /// </summary>
        public async Task StartAsync(TimeSpan timeout)
        {
            lock (sync)
            {
                if (process != null)
                    throw new InvalidOperationException("Monitor already started.");
                process = StartProcess();
            }

            using var cts = new CancellationTokenSource(timeout);
            using (cts.Token.Register(() =>
            {
                firstCapabilities.TrySetException(new TimeoutException("lively-wl-monitor did not report capabilities in time."));
                firstOutputs.TrySetException(new TimeoutException("lively-wl-monitor did not report outputs in time."));
            }))
            {
                await firstCapabilities.Task;
                await firstOutputs.Task;
            }
        }

        private Process StartProcess()
        {
            var started = new Process
            {
                EnableRaisingEvents = true,
                StartInfo = new ProcessStartInfo
                {
                    FileName = helperPath,
                    UseShellExecute = false,
                    RedirectStandardInput = true,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                }
            };
            started.OutputDataReceived += Process_OutputDataReceived;
            started.ErrorDataReceived += (s, e) => { if (!string.IsNullOrEmpty(e.Data)) Logger.Warn($"wl-monitor: {e.Data}"); };
            started.Exited += Process_Exited;
            started.Start();
            started.BeginOutputReadLine();
            started.BeginErrorReadLine();
            return started;
        }

        private void Process_Exited(object sender, EventArgs e)
        {
            var exited = (Process)sender;
            int code;
            try
            {
                code = exited.ExitCode;
            }
            catch (InvalidOperationException)
            {
                code = -1;
            }

            bool ownsRestart;
            lock (sync)
            {
                ownsRestart = !disposed && ReferenceEquals(process, exited);
                if (ownsRestart)
                    process = null;
            }

            firstCapabilities.TrySetException(new InvalidOperationException($"lively-wl-monitor exited with code {code} before reporting capabilities."));
            firstOutputs.TrySetException(new InvalidOperationException($"lively-wl-monitor exited with code {code} before reporting outputs."));
            if (!ownsRestart)
                return;

            Logger.Error($"lively-wl-monitor exited with code {code}; starting it again.");
            exited.Dispose();
            HelperExited?.Invoke(this, code);
            _ = RestartAsync();
        }

        private async Task RestartAsync()
        {
            while (true)
            {
                var attempt = Interlocked.Increment(ref restartAttempts);
                var delay = TimeSpan.FromSeconds(Math.Min(MaxRestartDelay.TotalSeconds, Math.Pow(2, attempt - 1)));
                await Task.Delay(delay);

                lock (sync)
                {
                    if (disposed || process != null)
                        return;
                    try
                    {
                        process = StartProcess();
                    }
                    catch (Exception ex)
                    {
                        Logger.Error($"Starting lively-wl-monitor again failed (attempt {attempt}): {ex.Message}");
                        continue;
                    }
                }
                Logger.Info($"lively-wl-monitor started again (attempt {attempt}).");
                return;
            }
        }

        private void Process_OutputDataReceived(object sender, DataReceivedEventArgs e)
        {
            if (string.IsNullOrWhiteSpace(e.Data))
                return;

            JObject jo;
            try
            {
                jo = JObject.Parse(e.Data);
            }
            catch (JsonException ex)
            {
                Logger.Error($"wl-monitor produced invalid JSON: {ex.Message} :: {e.Data}");
                return;
            }

            switch ((string)jo["event"])
            {
                case "capabilities":
                    Capabilities = jo.ToObject<WaylandCapabilities>();
                    Logger.Info($"Wayland capabilities: layer_shell={Capabilities.LayerShell} plasma_shell={Capabilities.PlasmaShell} toplevels={Capabilities.ToplevelProtocol}");
                    firstCapabilities.TrySetResult(true);
                    break;
                case "outputs":
                    Outputs = (jo["outputs"]?.ToObject<List<WaylandOutput>>() ?? new List<WaylandOutput>()).AsReadOnly();
                    Logger.Info($"Wayland outputs: {string.Join(", ", Outputs.Select(o => $"{o.Name} {o.Width}x{o.Height}@{o.X},{o.Y} x{o.Scale:0.##}"))}");
                    firstOutputs.TrySetResult(true);
                    OutputsChanged?.Invoke(this, EventArgs.Empty);
                    if (Interlocked.Exchange(ref restartAttempts, 0) != 0)
                        HelperRestarted?.Invoke(this, EventArgs.Empty);
                    break;
                case "toplevels":
                    IsShowDesktopActive = (bool?)jo["show_desktop"] ?? false;
                    Toplevels = (jo["toplevels"]?.ToObject<List<WaylandToplevel>>() ?? new List<WaylandToplevel>()).AsReadOnly();
                    ToplevelsChanged?.Invoke(this, EventArgs.Empty);
                    break;
                default:
                    Logger.Warn($"wl-monitor unknown event: {e.Data}");
                    break;
            }
        }

        public void Dispose()
        {
            Process running;
            lock (sync)
            {
                if (disposed)
                    return;
                disposed = true;
                running = process;
                process = null;
            }
            if (running == null)
                return;

            try
            {
                // Closing stdin asks the helper to exit; kill if it does not.
                running.StandardInput.Close();
                if (!running.WaitForExit(1000))
                    running.Kill();
            }
            catch (Exception ex)
            {
                Logger.Warn($"Stopping lively-wl-monitor: {ex.Message}");
            }
            finally
            {
                running.Dispose();
            }
        }
    }

    /// <summary>
    /// Finds the native helper binaries: LIVELY_NATIVE_DIR, then plugins/native next to the core, then PATH.
    /// </summary>
    public sealed class NativeHelperLocator
    {
        private readonly string[] searchDirs;

        public NativeHelperLocator()
        {
            var dirs = new List<string>();
            var env = Environment.GetEnvironmentVariable("LIVELY_NATIVE_DIR");
            if (!string.IsNullOrEmpty(env))
                dirs.Add(env);
            dirs.Add(Path.Combine(AppContext.BaseDirectory, "plugins", "native"));
            var path = Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
            dirs.AddRange(path.Split(':', StringSplitOptions.RemoveEmptyEntries));
            searchDirs = dirs.ToArray();
        }

        public string Resolve(string name)
        {
            foreach (var dir in searchDirs)
            {
                var candidate = Path.Combine(dir, name);
                if (File.Exists(candidate))
                    return candidate;
            }
            throw new FileNotFoundException($"Native helper '{name}' not found. Searched: {string.Join(", ", searchDirs.Take(2))} and PATH. Build it with `make native`.", name);
        }
    }
}
