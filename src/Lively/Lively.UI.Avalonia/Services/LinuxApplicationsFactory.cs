using Lively.Common;
using Lively.Common.Factories;
using Lively.Models;
using System;
using System.Diagnostics;
using System.IO;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Builds <see cref="ApplicationModel"/>s from /proc entries, .desktop files or executable paths.
    /// </summary>
    public class LinuxApplicationsFactory : IApplicationsFactory
    {
        private readonly string cacheDir = Path.Combine(Constants.CommonPaths.TempDir, "icons");

        public ApplicationModel CreateApp(IntPtr hwnd)
        {
            throw new PlatformNotSupportedException("Window handles are not available on Wayland; enumerate running processes through /proc instead.");
        }

        public ApplicationModel CreateApp(Process process)
        {
            if (process is null)
                return null;

            try
            {
                var procDir = $"/proc/{process.Id}";
                var exe = ReadLink(Path.Combine(procDir, "exe"));
                if (string.IsNullOrEmpty(exe))
                    return null;

                var comm = File.ReadAllText(Path.Combine(procDir, "comm")).Trim();
                var model = new ApplicationModel
                {
                    AppName = string.IsNullOrEmpty(comm) ? Path.GetFileName(exe) : comm,
                    AppPath = exe,
                };

                var entry = LinuxIconLookup.FindDesktopEntryForExecutable(exe);
                if (entry?.Name != null)
                    model.AppName = entry.Name;
                model.AppIcon = CacheIcon(model.AppName, entry?.Icon ?? Path.GetFileName(exe));
                return model;
            }
            catch
            {
                //Failed to retrieve process information.
                return null;
            }
        }

        public ApplicationModel CreateApp(string path)
        {
            if (string.IsNullOrWhiteSpace(path))
                return null;

            if (path.EndsWith(".desktop", StringComparison.OrdinalIgnoreCase))
            {
                var entry = LinuxIconLookup.ParseDesktopEntry(path);
                if (entry?.ExecutableName == null)
                    return null;

                var exePath = LinuxIconLookup.ResolveExecutablePath(entry.ExecutableName);
                return new ApplicationModel
                {
                    AppName = entry.Name ?? Path.GetFileNameWithoutExtension(path),
                    AppPath = exePath,
                    AppIcon = CacheIcon(entry.Name ?? Path.GetFileNameWithoutExtension(path), entry.Icon),
                };
            }

            var model = new ApplicationModel
            {
                AppName = Path.GetFileNameWithoutExtension(path),
                AppPath = path
            };
            var matching = LinuxIconLookup.FindDesktopEntryForExecutable(path);
            model.AppIcon = CacheIcon(model.AppName, matching?.Icon ?? Path.GetFileName(path));
            return model;
        }

        private string CacheIcon(string appName, string iconName)
        {
            try
            {
                var resolved = LinuxIconLookup.ResolveIcon(iconName);
                if (resolved == null)
                    return null;

                Directory.CreateDirectory(cacheDir);
                var iconPath = Path.Combine(cacheDir, SanitizeFileName(appName) + ".png");
                if (!File.Exists(iconPath))
                    File.Copy(resolved, iconPath);
                return iconPath;
            }
            catch
            {
                // Model is still useful without icon.
                return null;
            }
        }

        private static string SanitizeFileName(string name)
        {
            foreach (var c in Path.GetInvalidFileNameChars())
                name = name.Replace(c, '_');
            return name;
        }

        private static string ReadLink(string path)
        {
            try
            {
                var target = new FileInfo(path).LinkTarget;
                if (string.IsNullOrEmpty(target))
                    return null;

                const string deletedSuffix = " (deleted)";
                return target.EndsWith(deletedSuffix, StringComparison.Ordinal) ? target.Substring(0, target.Length - deletedSuffix.Length) : target;
            }
            catch
            {
                return null;
            }
        }
    }
}
