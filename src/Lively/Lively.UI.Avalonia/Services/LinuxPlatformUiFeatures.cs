using Lively.Common.Factories;
using Lively.Models;
using Lively.Models.Enums;
using Lively.UI.Shared.Services;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Services
{
    public class LinuxPlatformUiFeatures : IPlatformUiFeatures
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly IApplicationsFactory appFactory;

        public LinuxPlatformUiFeatures(IApplicationsFactory appFactory)
        {
            this.appFactory = appFactory;
        }

        [DllImport("libc", EntryPoint = "geteuid")]
        private static extern uint GetEffectiveUserId();

        [DllImport("libc", EntryPoint = "getuid")]
        private static extern uint GetUserId();

        public bool IsElevated => GetEffectiveUserId() == 0;

        public bool IsPackaged => false;

        public string[] ApplicationFileExtensions => ["*", ".desktop"];

        /// <summary>
        /// Processes of the current user that are attached to the display server, one entry per executable.
        /// Helpers living under /usr/lib are only listed when a .desktop entry launches them.
        /// </summary>
        public IEnumerable<ApplicationModel> GetRunningApplications()
        {
            var uid = GetUserId().ToString();
            var seenExecutables = new HashSet<string>(StringComparer.Ordinal);
            var selfPid = Environment.ProcessId;

            foreach (var procDir in Directory.EnumerateDirectories("/proc"))
            {
                if (!int.TryParse(Path.GetFileName(procDir), out var pid) || pid == selfPid)
                    continue;

                string exe;
                try
                {
                    if (!IsOwnedBy(procDir, uid) || !IsAttachedToDisplay(procDir))
                        continue;

                    exe = new FileInfo(Path.Combine(procDir, "exe")).LinkTarget;
                }
                catch (Exception)
                {
                    // Process exited or is not readable.
                    continue;
                }

                if (string.IsNullOrEmpty(exe) || !seenExecutables.Add(exe))
                    continue;

                var isLibraryHelper = exe.StartsWith("/usr/lib", StringComparison.Ordinal) || exe.StartsWith("/usr/libexec", StringComparison.Ordinal);
                if (isLibraryHelper && LinuxIconLookup.FindDesktopEntryForExecutable(exe) == null)
                    continue;

                ApplicationModel app;
                try
                {
                    using var process = Process.GetProcessById(pid);
                    app = appFactory.CreateApp(process);
                }
                catch (Exception ex)
                {
                    Logger.Debug($"Skipping process {pid}: {ex.Message}");
                    continue;
                }

                if (app != null)
                    yield return app;
            }
        }

        public bool SupportsDesktopIconToggle => false;

        public void SetDesktopIconVisibility(bool isVisible)
        {
            throw new PlatformNotSupportedException("Desktop icon visibility is managed by the desktop shell on Linux.");
        }

        public bool SupportsScreensaver => false;

        public bool IsScreensaverRegistered()
        {
            throw new PlatformNotSupportedException("Lively cannot be registered as the screensaver on Linux.");
        }

        public void OpenSystemScreensaverSettings()
        {
            throw new PlatformNotSupportedException("Lively cannot be registered as the screensaver on Linux.");
        }

        public bool SupportsSystemColorSettings => GetColorSettingsCommand() != null;

        public void OpenSystemColorSettings()
        {
            var command = GetColorSettingsCommand()
                ?? throw new PlatformNotSupportedException("No supported system settings application found for colours.");

            var startInfo = new ProcessStartInfo
            {
                FileName = command[0],
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            };
            for (int i = 1; i < command.Length; i++)
                startInfo.ArgumentList.Add(command[i]);
            Process.Start(startInfo);
        }

        /// <summary>
        /// The Linux core has exactly one player per media kind (mpv and the WebKitGTK host), so there is nothing to pick.
        /// </summary>
        public bool SupportsPlayerSelection => false;

        public bool IsPlayerAvailable(LivelyMediaPlayer player) => player == LivelyMediaPlayer.mpv;

        public bool IsGifPlayerAvailable(LivelyGifPlayer player) => player == LivelyGifPlayer.mpv;

        public bool IsWebBrowserAvailable(LivelyWebBrowser browser) => browser == LivelyWebBrowser.webview2;

        /// <summary>
        /// Stream wallpapers need yt-dlp (or its predecessor youtube-dl) on PATH.
        /// </summary>
        public bool IsStreamDownloaderAvailable =>
            File.Exists(LinuxIconLookup.ResolveExecutablePath("yt-dlp")) || File.Exists(LinuxIconLookup.ResolveExecutablePath("youtube-dl"));

        public bool SupportsTaskbarTheme => false;

        /// <summary>
        /// Wayland sessions are local; a remote viewer (KRDP, VNC) mirrors the same session and the core has no
        /// signal that distinguishes it, so the rule is not offered.
        /// </summary>
        public bool SupportsRemoteDesktopPause => false;

        public bool SupportsWebViewRuntimeInstall => false;

        /// <summary>
        /// The Linux web wallpaper host renders with WebKitGTK, which is a distribution package and not installed by the UI.
        /// </summary>
        public bool IsWebViewRuntimeAvailable => true;

        public string WebViewRuntimeDownloadUrl => "https://webkitgtk.org/";

        public Task<bool> TryInstallWebViewRuntimeAsync() => Task.FromResult(false);

        private static bool IsOwnedBy(string procDir, string uid)
        {
            foreach (var line in File.ReadLines(Path.Combine(procDir, "status")))
            {
                if (!line.StartsWith("Uid:", StringComparison.Ordinal))
                    continue;

                var parts = line.Substring(4).Split(['\t', ' '], StringSplitOptions.RemoveEmptyEntries);
                return parts.Length > 0 && parts[0] == uid;
            }
            return false;
        }

        private static bool IsAttachedToDisplay(string procDir)
        {
            var environ = File.ReadAllBytes(Path.Combine(procDir, "environ"));
            if (environ.Length == 0)
                return false;

            var text = System.Text.Encoding.UTF8.GetString(environ);
            foreach (var variable in text.Split('\0', StringSplitOptions.RemoveEmptyEntries))
            {
                if (variable.StartsWith("WAYLAND_DISPLAY=", StringComparison.Ordinal) || variable.StartsWith("DISPLAY=", StringComparison.Ordinal))
                    return true;
            }
            return false;
        }

        private static string[] GetColorSettingsCommand()
        {
            var desktop = Environment.GetEnvironmentVariable("XDG_CURRENT_DESKTOP") ?? string.Empty;
            if (desktop.Contains("KDE", StringComparison.OrdinalIgnoreCase))
            {
                var settings = LinuxIconLookup.ResolveExecutablePath("systemsettings");
                if (File.Exists(settings))
                    return [settings, "kcm_colors"];
            }
            if (desktop.Contains("GNOME", StringComparison.OrdinalIgnoreCase))
            {
                var settings = LinuxIconLookup.ResolveExecutablePath("gnome-control-center");
                if (File.Exists(settings))
                    return [settings, "appearance"];
            }
            return null;
        }
    }
}
