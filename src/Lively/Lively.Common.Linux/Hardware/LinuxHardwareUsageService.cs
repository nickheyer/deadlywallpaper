using Lively.Common.Services;
using Lively.Models.Services;
using NLog;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.Common.Linux.Hardware
{
    /// <summary>
    /// Raises <see cref="HWMonitor"/> once per second with CPU, GPU, RAM and default-route network figures read from procfs and sysfs.
    /// </summary>
    public sealed class LinuxHardwareUsageService : IHardwareUsageService
    {
        private static readonly Logger logger = LogManager.GetCurrentClassLogger();
        private static readonly TimeSpan SampleInterval = TimeSpan.FromSeconds(1);

        public event EventHandler<HardwareUsageEventArgs> HWMonitor = delegate { };

        /// <summary>
        /// True when the primary GPU exposes a utilisation source (amdgpu gpu_busy_percent, nvidia-smi, or i915 GT frequency ratio).
        /// When false, <see cref="HardwareUsageEventArgs.CurrentGpu3D"/> is always 0 and only the GPU name is reported.
        /// </summary>
        public bool HasGpuUtilization => gpu?.UtilizationSource != null;

        private readonly string procRoot;
        private readonly string sysRoot;
        private readonly string nameCpu;
        private readonly long totalRamMb;
        private readonly GpuDevice gpu;
        private CancellationTokenSource cts;

        public LinuxHardwareUsageService() : this("/proc", "/sys") { }

        /// <param name="procRoot">Mount point of procfs (normally <c>/proc</c>).</param>
        /// <param name="sysRoot">Mount point of sysfs (normally <c>/sys</c>).</param>
        public LinuxHardwareUsageService(string procRoot, string sysRoot)
        {
            this.procRoot = procRoot ?? throw new ArgumentNullException(nameof(procRoot));
            this.sysRoot = sysRoot ?? throw new ArgumentNullException(nameof(sysRoot));

            nameCpu = ParseCpuModelName(File.ReadAllText(Path.Combine(procRoot, "cpuinfo")));
            totalRamMb = ParseMemInfo(File.ReadAllText(Path.Combine(procRoot, "meminfo"))).totalMb;
            gpu = GpuDevice.FindPrimary(procRoot, sysRoot);
            if (gpu is null)
                logger.Warn("No DRM GPU found under {0}/class/drm; GPU name and utilisation are unavailable.", sysRoot);
            else if (gpu.UtilizationSource is null)
                logger.Warn("GPU '{0}' ({1}) exposes no utilisation source; CurrentGpu3D stays 0.", gpu.Name, gpu.Driver);
            else
                logger.Info("GPU '{0}' utilisation read from {1}.", gpu.Name, gpu.UtilizationSource.Description);
        }

        public void Start()
        {
            if (cts != null)
                throw new InvalidOperationException("Service once stopped cannot be restarted!");
            cts = new CancellationTokenSource();
            _ = Task.Run(() => MonitorLoopAsync(cts.Token));
        }

        public void Stop()
        {
            cts?.Cancel();
        }

        private async Task MonitorLoopAsync(CancellationToken token)
        {
            try
            {
                var previousCpu = ParseProcStatCpu(File.ReadAllText(Path.Combine(procRoot, "stat")));
                var previousInterface = ParseDefaultRouteInterface(File.ReadAllText(Path.Combine(procRoot, "net", "route")));
                var previousNet = previousInterface is null ? default : ParseNetDev(File.ReadAllText(Path.Combine(procRoot, "net", "dev")), previousInterface);
                var stopwatch = Stopwatch.StartNew();

                while (true)
                {
                    await Task.Delay(SampleInterval, token).ConfigureAwait(false);
                    HardwareUsageEventArgs sample;
                    try
                    {
                        var currentCpu = ParseProcStatCpu(File.ReadAllText(Path.Combine(procRoot, "stat")));
                        var memory = ParseMemInfo(File.ReadAllText(Path.Combine(procRoot, "meminfo")));
                        var currentInterface = ParseDefaultRouteInterface(File.ReadAllText(Path.Combine(procRoot, "net", "route")));
                        var currentNet = currentInterface is null ? default : ParseNetDev(File.ReadAllText(Path.Combine(procRoot, "net", "dev")), currentInterface);
                        var elapsedSeconds = stopwatch.Elapsed.TotalSeconds;
                        stopwatch.Restart();

                        var sameInterface = currentInterface != null && string.Equals(currentInterface, previousInterface, StringComparison.Ordinal);
                        sample = new HardwareUsageEventArgs
                        {
                            NameCpu = nameCpu,
                            NameGpu = gpu?.Name,
                            NameNetCard = currentInterface,
                            TotalRam = totalRamMb,
                            CurrentCpu = CpuUsagePercent(previousCpu, currentCpu),
                            CurrentRamAvail = memory.availableMb,
                            CurrentGpu3D = gpu?.UtilizationSource != null ? gpu.UtilizationSource.ReadPercent() : 0f,
                            CurrentNetDown = sameInterface ? BytesPerSecond(previousNet.rxBytes, currentNet.rxBytes, elapsedSeconds) : 0f,
                            CurrentNetUp = sameInterface ? BytesPerSecond(previousNet.txBytes, currentNet.txBytes, elapsedSeconds) : 0f,
                        };

                        previousCpu = currentCpu;
                        previousInterface = currentInterface;
                        previousNet = currentNet;
                    }
                    catch (Exception ex)
                    {
                        logger.Error(ex, "Hardware sample failed; no HWMonitor event is raised for this interval.");
                        continue;
                    }

                    try
                    {
                        HWMonitor?.Invoke(this, sample);
                    }
                    catch (Exception ex)
                    {
                        logger.Error(ex, "An HWMonitor handler threw.");
                    }
                }
            }
            catch (OperationCanceledException)
            {
                cts.Dispose();
            }
            catch (Exception ex)
            {
                logger.Error(ex, "Hardware monitor loop stopped.");
                cts.Dispose();
            }
        }

        private static float BytesPerSecond(long previous, long current, double elapsedSeconds)
        {
            if (elapsedSeconds <= 0)
                throw new ArgumentOutOfRangeException(nameof(elapsedSeconds), elapsedSeconds, "A positive interval is required to compute a rate.");
            // A counter below its previous value means the interface's counters were reset; everything counted since then is the delta.
            var delta = current >= previous ? current - previous : current;
            return (float)(delta / elapsedSeconds);
        }

        #region parsers

        /// <summary>Aggregate jiffies from the first <c>cpu</c> line of /proc/stat.</summary>
        public readonly record struct CpuSample(long Idle, long Total);

        /// <summary>Parses the aggregate <c>cpu</c> line of /proc/stat.</summary>
        public static CpuSample ParseProcStatCpu(string procStatText)
        {
            if (procStatText is null)
                throw new ArgumentNullException(nameof(procStatText));

            foreach (var rawLine in procStatText.Split('\n'))
            {
                var line = rawLine.Trim();
                if (!line.StartsWith("cpu ", StringComparison.Ordinal))
                    continue;
                var fields = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (fields.Length < 5)
                    throw new FormatException($"/proc/stat cpu line has too few fields: '{line}'");
                // user nice system idle iowait irq softirq steal (guest and guest_nice are already included in user/nice).
                var values = fields.Skip(1).Take(8).Select(f => long.Parse(f, CultureInfo.InvariantCulture)).ToArray();
                long idle = values[3] + (values.Length > 4 ? values[4] : 0);
                long total = values.Sum();
                return new CpuSample(idle, total);
            }
            throw new FormatException("/proc/stat contains no aggregate 'cpu' line.");
        }

        /// <summary>Total CPU usage percentage between two /proc/stat samples.</summary>
        public static float CpuUsagePercent(CpuSample previous, CpuSample current)
        {
            long totalDelta = current.Total - previous.Total;
            long idleDelta = current.Idle - previous.Idle;
            if (totalDelta <= 0)
                return 0f;
            var busy = (double)(totalDelta - idleDelta) / totalDelta * 100.0;
            return (float)Math.Clamp(busy, 0.0, 100.0);
        }

        /// <summary>Parses MemTotal and MemAvailable (kB in the file) from /proc/meminfo into megabytes.</summary>
        public static (long totalMb, float availableMb) ParseMemInfo(string memInfoText)
        {
            if (memInfoText is null)
                throw new ArgumentNullException(nameof(memInfoText));

            long? totalKb = null, availableKb = null;
            foreach (var rawLine in memInfoText.Split('\n'))
            {
                var line = rawLine.Trim();
                if (line.StartsWith("MemTotal:", StringComparison.Ordinal))
                    totalKb = ParseKb(line);
                else if (line.StartsWith("MemAvailable:", StringComparison.Ordinal))
                    availableKb = ParseKb(line);
                if (totalKb.HasValue && availableKb.HasValue)
                    break;
            }
            if (!totalKb.HasValue)
                throw new FormatException("/proc/meminfo has no MemTotal line.");
            if (!availableKb.HasValue)
                throw new FormatException("/proc/meminfo has no MemAvailable line.");
            return (totalKb.Value / 1024, availableKb.Value / 1024f);

            static long ParseKb(string line)
            {
                var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length < 2)
                    throw new FormatException($"Malformed /proc/meminfo line: '{line}'");
                return long.Parse(parts[1], CultureInfo.InvariantCulture);
            }
        }

        /// <summary>
        /// The interface carrying the default route in /proc/net/route (destination 00000000 with the UP and GATEWAY flags set),
        /// choosing the lowest metric when several exist. Null when there is no default route.
        /// </summary>
        public static string ParseDefaultRouteInterface(string routeText)
        {
            if (routeText is null)
                throw new ArgumentNullException(nameof(routeText));

            string best = null;
            long bestMetric = long.MaxValue;
            foreach (var rawLine in routeText.Split('\n').Skip(1))
            {
                var fields = rawLine.Split(new[] { '\t', ' ' }, StringSplitOptions.RemoveEmptyEntries);
                if (fields.Length < 7)
                    continue;
                var destination = fields[1];
                var flags = Convert.ToInt32(fields[3], 16);
                var metric = long.Parse(fields[6], CultureInfo.InvariantCulture);
                const int upAndGateway = 0x0003;
                if (destination != "00000000" || (flags & upAndGateway) != upAndGateway)
                    continue;
                if (metric < bestMetric)
                {
                    bestMetric = metric;
                    best = fields[0];
                }
            }
            return best;
        }

        /// <summary>Received and transmitted byte counters of <paramref name="interfaceName"/> from /proc/net/dev.</summary>
        public static (long rxBytes, long txBytes) ParseNetDev(string netDevText, string interfaceName)
        {
            if (netDevText is null)
                throw new ArgumentNullException(nameof(netDevText));
            if (string.IsNullOrEmpty(interfaceName))
                throw new ArgumentException("Interface name is required.", nameof(interfaceName));

            foreach (var rawLine in netDevText.Split('\n'))
            {
                var colon = rawLine.IndexOf(':');
                if (colon < 0)
                    continue;
                if (!string.Equals(rawLine[..colon].Trim(), interfaceName, StringComparison.Ordinal))
                    continue;
                var counters = rawLine[(colon + 1)..].Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (counters.Length < 9)
                    throw new FormatException($"/proc/net/dev line for {interfaceName} has too few counters: '{rawLine}'");
                return (long.Parse(counters[0], CultureInfo.InvariantCulture), long.Parse(counters[8], CultureInfo.InvariantCulture));
            }
            throw new FormatException($"/proc/net/dev has no line for interface '{interfaceName}'.");
        }

        /// <summary>The first <c>model name</c> entry of /proc/cpuinfo.</summary>
        public static string ParseCpuModelName(string cpuInfoText)
        {
            if (cpuInfoText is null)
                throw new ArgumentNullException(nameof(cpuInfoText));

            foreach (var rawLine in cpuInfoText.Split('\n'))
            {
                var colon = rawLine.IndexOf(':');
                if (colon < 0)
                    continue;
                var key = rawLine[..colon].Trim();
                if (key == "model name" || key == "Model" || key == "cpu model")
                    return rawLine[(colon + 1)..].Trim();
            }
            throw new FormatException("/proc/cpuinfo has no 'model name' entry.");
        }

        #endregion

        #region gpu

        /// <summary>A per-second GPU utilisation reader.</summary>
        private interface IGpuUtilizationSource
        {
            string Description { get; }
            float ReadPercent();
        }

        private sealed class GpuDevice
        {
            private static readonly Regex CardDirectory = new(@"^card(\d+)$", RegexOptions.Compiled);
            private static readonly string[] PciIdsPaths =
            {
                "/usr/share/hwdata/pci.ids",
                "/usr/share/misc/pci.ids",
                "/usr/share/pci.ids",
                "/var/lib/pciutils/pci.ids",
            };

            public int Index { get; init; }
            public string Name { get; init; }
            public string Driver { get; init; }
            public bool BootVga { get; init; }
            public IGpuUtilizationSource UtilizationSource { get; init; }

            /// <summary>
            /// Enumerates /sys/class/drm/card* and picks the GPU to report: one with a utilisation source first,
            /// then the boot VGA device, then the lowest card index. Null when there is no DRM card.
            /// </summary>
            public static GpuDevice FindPrimary(string procRoot, string sysRoot)
            {
                var drmRoot = Path.Combine(sysRoot, "class", "drm");
                if (!Directory.Exists(drmRoot))
                    return null;

                var devices = new List<GpuDevice>();
                foreach (var cardDir in Directory.GetDirectories(drmRoot))
                {
                    var match = CardDirectory.Match(Path.GetFileName(cardDir));
                    if (!match.Success)
                        continue;
                    var deviceDir = Path.Combine(cardDir, "device");
                    var vendorPath = Path.Combine(deviceDir, "vendor");
                    if (!File.Exists(vendorPath))
                        continue;

                    var index = int.Parse(match.Groups[1].Value, CultureInfo.InvariantCulture);
                    var vendorId = ReadHexId(vendorPath);
                    var deviceId = ReadHexId(Path.Combine(deviceDir, "device"));
                    var driver = ReadLinkName(Path.Combine(deviceDir, "driver"));
                    var pciAddress = ReadLinkName(deviceDir);
                    var bootVga = ReadTrimmed(Path.Combine(deviceDir, "boot_vga")) == "1";
                    var name = ResolveName(procRoot, deviceDir, vendorId, deviceId, driver, pciAddress);
                    var source = ProbeUtilizationSource(cardDir, deviceDir, vendorId, driver, pciAddress);
                    devices.Add(new GpuDevice { Index = index, Name = name, Driver = driver, BootVga = bootVga, UtilizationSource = source });
                    logger.Info("DRM card{0}: {1} (driver {2}, boot_vga {3}, utilisation source: {4})", index, name, driver, bootVga, source?.Description ?? "none");
                }

                return devices
                    .OrderByDescending(d => d.UtilizationSource != null)
                    .ThenByDescending(d => d.BootVga)
                    .ThenBy(d => d.Index)
                    .FirstOrDefault();
            }

            private static IGpuUtilizationSource ProbeUtilizationSource(string cardDir, string deviceDir, string vendorId, string driver, string pciAddress)
            {
                var amdBusy = Path.Combine(deviceDir, "gpu_busy_percent");
                if (File.Exists(amdBusy))
                    return new SysfsPercentSource(amdBusy);

                if (driver == "nvidia" || vendorId == "10de")
                {
                    var nvidia = NvidiaSmiSource.TryCreate(pciAddress);
                    if (nvidia != null)
                        return nvidia;
                }

                var actual = Path.Combine(cardDir, "gt_act_freq_mhz");
                var requested = Path.Combine(cardDir, "gt_cur_freq_mhz");
                var max = Path.Combine(cardDir, "gt_max_freq_mhz");
                var rp0 = Path.Combine(cardDir, "gt_RP0_freq_mhz");
                var currentPath = File.Exists(actual) ? actual : File.Exists(requested) ? requested : null;
                var maxPath = File.Exists(max) ? max : File.Exists(rp0) ? rp0 : null;
                if (currentPath != null && maxPath != null)
                    return new FrequencyRatioSource(currentPath, maxPath);

                return null;
            }

            private static string ResolveName(string procRoot, string deviceDir, string vendorId, string deviceId, string driver, string pciAddress)
            {
                if (driver == "nvidia" && pciAddress != null)
                {
                    var info = Path.Combine(procRoot, "driver", "nvidia", "gpus", pciAddress, "information");
                    if (File.Exists(info))
                    {
                        foreach (var line in File.ReadLines(info))
                        {
                            if (line.StartsWith("Model:", StringComparison.Ordinal))
                                return line["Model:".Length..].Trim();
                        }
                    }
                }

                var productName = ReadTrimmed(Path.Combine(deviceDir, "product_name"));
                if (!string.IsNullOrEmpty(productName))
                    return productName;

                var pciName = LookupPciIds(vendorId, deviceId);
                if (pciName != null)
                    return pciName;

                var label = ReadTrimmed(Path.Combine(deviceDir, "label"));
                if (!string.IsNullOrEmpty(label))
                    return label;

                logger.Warn("No pci.ids database found at {0}; reporting GPU by PCI id.", string.Join(", ", PciIdsPaths));
                return $"{KnownVendorName(vendorId)} device {deviceId} ({driver ?? "no driver"})";
            }

            private static string KnownVendorName(string vendorId) => vendorId switch
            {
                "8086" => "Intel",
                "10de" => "NVIDIA",
                "1002" => "AMD",
                "15ad" => "VMware",
                "1af4" => "Virtio",
                "1234" => "QEMU",
                _ => $"PCI vendor {vendorId}",
            };

            private static string LookupPciIds(string vendorId, string deviceId)
            {
                var path = PciIdsPaths.FirstOrDefault(File.Exists);
                if (path is null)
                    return null;

                string vendorName = null;
                var inVendor = false;
                foreach (var line in File.ReadLines(path))
                {
                    if (line.Length == 0 || line[0] == '#')
                        continue;
                    if (line[0] != '\t')
                    {
                        if (inVendor)
                            break;
                        if (line.Length > 6 && line.StartsWith(vendorId, StringComparison.OrdinalIgnoreCase) && line[4] == ' ')
                        {
                            vendorName = line[4..].Trim();
                            inVendor = true;
                        }
                        continue;
                    }
                    if (!inVendor || line.Length < 2 || line[1] == '\t')
                        continue;
                    var body = line[1..];
                    if (body.Length > 6 && body.StartsWith(deviceId, StringComparison.OrdinalIgnoreCase) && body[4] == ' ')
                        return $"{vendorName} {body[4..].Trim()}";
                }
                return vendorName is null ? null : $"{vendorName} device {deviceId}";
            }

            private static string ReadHexId(string path)
            {
                var text = ReadTrimmed(path) ?? throw new FileNotFoundException("Missing sysfs id file.", path);
                return (text.StartsWith("0x", StringComparison.OrdinalIgnoreCase) ? text[2..] : text).ToLowerInvariant();
            }

            private static string ReadTrimmed(string path) => File.Exists(path) ? File.ReadAllText(path).Trim() : null;

            private static string ReadLinkName(string path)
            {
                var info = new FileInfo(path);
                if (info.LinkTarget != null)
                    return Path.GetFileName(info.LinkTarget.TrimEnd('/'));
                var dir = new DirectoryInfo(path);
                if (dir.LinkTarget != null)
                    return Path.GetFileName(dir.LinkTarget.TrimEnd('/'));
                return Directory.Exists(path) ? Path.GetFileName(Path.GetFullPath(path).TrimEnd('/')) : null;
            }
        }

        /// <summary>amdgpu's <c>gpu_busy_percent</c>: an integer percentage.</summary>
        private sealed class SysfsPercentSource : IGpuUtilizationSource
        {
            private readonly string path;
            public SysfsPercentSource(string path) => this.path = path;
            public string Description => path;
            public float ReadPercent() => float.Parse(File.ReadAllText(path).Trim(), CultureInfo.InvariantCulture);
        }

        /// <summary>i915 GT frequency as a percentage of the maximum frequency.</summary>
        private sealed class FrequencyRatioSource : IGpuUtilizationSource
        {
            private readonly string currentPath;
            private readonly string maxPath;

            public FrequencyRatioSource(string currentPath, string maxPath)
            {
                this.currentPath = currentPath;
                this.maxPath = maxPath;
            }

            public string Description => $"{currentPath} / {maxPath}";

            public float ReadPercent()
            {
                var current = double.Parse(File.ReadAllText(currentPath).Trim(), CultureInfo.InvariantCulture);
                var max = double.Parse(File.ReadAllText(maxPath).Trim(), CultureInfo.InvariantCulture);
                if (max <= 0)
                    throw new InvalidOperationException($"{maxPath} reports a non-positive maximum frequency ({max}).");
                return (float)Math.Clamp(current / max * 100.0, 0.0, 100.0);
            }
        }

        /// <summary><c>nvidia-smi --query-gpu=utilization.gpu</c> for one PCI device.</summary>
        private sealed class NvidiaSmiSource : IGpuUtilizationSource
        {
            private const int TimeoutMs = 2000;
            private readonly string pciAddress;

            private NvidiaSmiSource(string pciAddress) => this.pciAddress = pciAddress;

            public string Description => $"nvidia-smi -i {pciAddress}";

            public static NvidiaSmiSource TryCreate(string pciAddress)
            {
                if (pciAddress is null || !IsOnPath("nvidia-smi"))
                    return null;
                var source = new NvidiaSmiSource(pciAddress);
                try
                {
                    source.ReadPercent();
                    return source;
                }
                catch (Exception ex)
                {
                    logger.Warn(ex, "nvidia-smi is installed but cannot read utilisation for {0}.", pciAddress);
                    return null;
                }
            }

            public float ReadPercent()
            {
                var psi = new ProcessStartInfo("nvidia-smi")
                {
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    CreateNoWindow = true,
                };
                psi.ArgumentList.Add("-i");
                psi.ArgumentList.Add(pciAddress);
                psi.ArgumentList.Add("--query-gpu=utilization.gpu");
                psi.ArgumentList.Add("--format=csv,noheader,nounits");

                using var process = Process.Start(psi)
                    ?? throw new InvalidOperationException("Failed to start nvidia-smi.");
                var stdoutTask = process.StandardOutput.ReadToEndAsync();
                var stderrTask = process.StandardError.ReadToEndAsync();
                if (!process.WaitForExit(TimeoutMs))
                {
                    process.Kill(entireProcessTree: true);
                    throw new TimeoutException($"nvidia-smi did not answer within {TimeoutMs} ms.");
                }
                process.WaitForExit();
                var stdout = stdoutTask.GetAwaiter().GetResult().Trim();
                if (process.ExitCode != 0)
                    throw new InvalidOperationException($"nvidia-smi exited with code {process.ExitCode}: {stderrTask.GetAwaiter().GetResult().Trim()}");
                return float.Parse(stdout, CultureInfo.InvariantCulture);
            }

            private static bool IsOnPath(string executable)
            {
                var path = Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
                return path.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries)
                    .Any(dir => File.Exists(Path.Combine(dir, executable)));
            }
        }

        #endregion
    }
}
