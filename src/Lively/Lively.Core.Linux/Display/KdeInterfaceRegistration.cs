using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Display
{
    /// <summary>
    /// KWin only hands the org_kde_plasma_window_management global to executables whose desktop
    /// file declares it in X-KDE-Wayland-Interfaces. This writes that desktop file for the monitor
    /// helper and refreshes the KService cache so KWin sees it. The file is named after the
    /// canonical helper path and its content is byte for byte what src/native/lively-wl-monitor/
    /// kde-register.sh writes, so build directories, build/dist and installs are registered side by
    /// side and the script and the core never rewrite each other's file.
    /// </summary>
    public sealed class KdeInterfaceRegistration
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        public const string WaylandInterfaces = "org_kde_plasma_window_management,org_kde_plasma_virtual_desktop_management";
        private const string DesktopFilePrefix = "lively-wl-monitor-";

        private readonly string helperPath;

        /// <summary>Canonical path of the helper, as KWin sees it in /proc/&lt;pid&gt;/exe.</summary>
        public string HelperPath => helperPath;

        public string DesktopFilePath { get; }

        public KdeInterfaceRegistration(string helperPath)
        {
            this.helperPath = Canonicalize(helperPath);
            var dataHome = Environment.GetEnvironmentVariable("XDG_DATA_HOME");
            if (string.IsNullOrEmpty(dataHome))
                dataHome = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".local", "share");
            DesktopFilePath = Path.Combine(dataHome, "applications", DesktopFileNameFor(this.helperPath));
        }

        public static bool IsKdeSession()
        {
            var desktop = Environment.GetEnvironmentVariable("XDG_CURRENT_DESKTOP") ?? string.Empty;
            return desktop.Split(':').Any(d => d.Equals("KDE", StringComparison.OrdinalIgnoreCase));
        }

        /// <summary>
        /// lively-wl-monitor-&lt;first eight hex digits of SHA-1(path)&gt;.desktop, the name kde-register.sh derives too.
        /// </summary>
        public static string DesktopFileNameFor(string canonicalPath)
        {
            var hash = Convert.ToHexString(SHA1.HashData(Encoding.UTF8.GetBytes(canonicalPath))).Substring(0, 8).ToLowerInvariant();
            return DesktopFilePrefix + hash + ".desktop";
        }

        public static string DesktopFileContentFor(string canonicalPath) =>
            "[Desktop Entry]\n" +
            "Type=Application\n" +
            "Name=Lively Wallpaper window monitor\n" +
            "Comment=Lets Lively Wallpaper see which windows are open so it can pause wallpapers under fullscreen apps\n" +
            $"Exec={canonicalPath}\n" +
            "NoDisplay=true\n" +
            "Terminal=false\n" +
            $"X-KDE-Wayland-Interfaces={WaylandInterfaces}\n";

        public string DesktopFileContent => DesktopFileContentFor(helperPath);

        [DllImport("libc", EntryPoint = "realpath", SetLastError = true)]
        private static extern IntPtr RealPath(string path, IntPtr resolved);

        [DllImport("libc", EntryPoint = "free")]
        private static extern void Free(IntPtr ptr);

        /// <summary>
        /// Resolves every symlink and relative component, giving the path KWin compares against and realpath(1) prints.
        /// </summary>
        public static string Canonicalize(string path)
        {
            var resolved = RealPath(path, IntPtr.Zero);
            if (resolved == IntPtr.Zero)
                throw new FileNotFoundException($"realpath failed for {path} (errno {Marshal.GetLastWin32Error()}).", path);
            try
            {
                return Marshal.PtrToStringUTF8(resolved);
            }
            finally
            {
                Free(resolved);
            }
        }

        /// <summary>
        /// Makes sure the registration exists for the helper path and that KWin exposes the interface.
        /// </summary>
        public async Task EnsureAsync()
        {
            var content = DesktopFileContent;
            var current = File.Exists(DesktopFilePath) ? File.ReadAllText(DesktopFilePath) : null;
            if (current != content)
            {
                Directory.CreateDirectory(Path.GetDirectoryName(DesktopFilePath));
                File.WriteAllText(DesktopFilePath, content);
                Logger.Info($"Registered {helperPath} for {WaylandInterfaces} via {DesktopFilePath}");
                await RebuildServiceCacheAsync();
            }

            if (await ProbeAsync())
                return;

            // The desktop file was already there but KWin does not honour it (cache from another
            // session environment, or KWin has not reloaded yet): rebuild and wait once more.
            Logger.Info("KWin does not expose the window list yet; rebuilding the KService cache.");
            await RebuildServiceCacheAsync();
            if (!await ProbeAsync())
                Logger.Error($"KWin still does not expose org_kde_plasma_window_management to {helperPath}. Fullscreen/focus pausing will be unavailable until KWin reloads its service cache (log out and in).");
        }

        /// <summary>Runs the helper once until it reports the plasma toplevel protocol (up to 10 s).</summary>
        private async Task<bool> ProbeAsync()
        {
            for (var attempt = 0; attempt < 20; attempt++)
            {
                try
                {
                    using var probe = Process.Start(new ProcessStartInfo
                    {
                        FileName = helperPath,
                        ArgumentList = { "--once" },
                        UseShellExecute = false,
                        RedirectStandardOutput = true,
                        RedirectStandardError = true,
                    });
                    var firstLine = await probe.StandardOutput.ReadLineAsync();
                    await probe.StandardOutput.ReadToEndAsync();
                    await probe.WaitForExitAsync();
                    if (firstLine != null && firstLine.Contains("\"toplevel_protocol\":\"plasma\"", StringComparison.Ordinal))
                        return true;
                }
                catch (Exception ex)
                {
                    Logger.Warn($"Probe of {helperPath} failed: {ex.Message}");
                    return false;
                }
                await Task.Delay(500);
            }
            return false;
        }

        /// <summary>
        /// kbuildsycoca6 must run with the XDG data variables of the session that started KWin,
        /// because the cache file name is derived from them.
        /// </summary>
        private static async Task RebuildServiceCacheAsync()
        {
            var startInfo = new ProcessStartInfo
            {
                FileName = "kbuildsycoca6",
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            };
            foreach (var kv in await SessionEnvironmentAsync())
                startInfo.Environment[kv.Key] = kv.Value;

            try
            {
                using var process = Process.Start(startInfo);
                var stderr = await process.StandardError.ReadToEndAsync();
                await process.StandardOutput.ReadToEndAsync();
                await process.WaitForExitAsync();
                if (process.ExitCode != 0)
                    Logger.Warn($"kbuildsycoca6 exited with {process.ExitCode}: {stderr}");
            }
            catch (Exception ex)
            {
                Logger.Error($"kbuildsycoca6 could not be run ({ex.Message}); KWin will not expose the window list until the service cache is rebuilt.");
            }
        }

        private static async Task<Dictionary<string, string>> SessionEnvironmentAsync()
        {
            var result = new Dictionary<string, string>();
            try
            {
                using var process = Process.Start(new ProcessStartInfo
                {
                    FileName = "systemctl",
                    ArgumentList = { "--user", "show-environment" },
                    UseShellExecute = false,
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                });
                var output = await process.StandardOutput.ReadToEndAsync();
                await process.WaitForExitAsync();
                foreach (var line in output.Split('\n', StringSplitOptions.RemoveEmptyEntries))
                {
                    foreach (var name in new[] { "XDG_DATA_DIRS", "XDG_DATA_HOME" })
                    {
                        if (line.StartsWith(name + "=", StringComparison.Ordinal))
                            result[name] = line.Substring(name.Length + 1);
                    }
                }
            }
            catch (Exception ex)
            {
                Logger.Info($"systemctl --user show-environment unavailable ({ex.Message}); using the current environment.");
            }
            return result;
        }
    }
}
