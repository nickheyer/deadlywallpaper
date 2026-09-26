using ImageMagick;
using Lively.Common;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;

namespace Lively.UI.Avalonia.Services
{
    public sealed class DesktopEntry
    {
        public string Path { get; set; }
        public string Name { get; set; }
        public string Exec { get; set; }
        public string Icon { get; set; }
        public bool NoDisplay { get; set; }

        /// <summary>
        /// First token of Exec without field codes, e.g. "/usr/bin/firefox" or "firefox".
        /// </summary>
        public string ExecutableName
        {
            get
            {
                if (string.IsNullOrWhiteSpace(Exec))
                    return null;
                var trimmed = Exec.Trim();
                var token = trimmed.StartsWith("\"") ? trimmed.Substring(1, Math.Max(0, trimmed.IndexOf('"', 1) - 1)) : trimmed.Split(' ')[0];
                if (token == "env" || token == "sh")
                {
                    // env VAR=x program ... -> program
                    var rest = trimmed.Split(' ', StringSplitOptions.RemoveEmptyEntries).Skip(1).FirstOrDefault(x => !x.Contains('='));
                    return rest;
                }
                return token;
            }
        }
    }

    /// <summary>
    /// Freedesktop lookups: icon theme icons, .desktop entries, mime icons and the XDG thumbnail cache.
    /// </summary>
    public static class LinuxIconLookup
    {
        private static readonly int[] preferredSizes = [64, 48, 128, 96, 256, 32, 24, 22, 16];
        private static readonly string[] contexts = ["apps", "devices", "mimetypes", "places", "status", "categories", "actions", "legacy"];
        private static readonly string cacheDir = Path.Combine(Constants.CommonPaths.TempDir, "icons");
        private static readonly object desktopEntriesGate = new object();
        private static List<DesktopEntry> desktopEntriesCache;

        public static IEnumerable<string> DataDirectories
        {
            get
            {
                var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
                var dataHome = Environment.GetEnvironmentVariable("XDG_DATA_HOME");
                yield return string.IsNullOrEmpty(dataHome) ? Path.Combine(home, ".local", "share") : dataHome;
                var dataDirs = Environment.GetEnvironmentVariable("XDG_DATA_DIRS");
                if (string.IsNullOrEmpty(dataDirs))
                    dataDirs = "/usr/local/share:/usr/share";
                foreach (var dir in dataDirs.Split(':', StringSplitOptions.RemoveEmptyEntries))
                    yield return dir;
                yield return Path.Combine(home, ".local", "share", "flatpak", "exports", "share");
                yield return "/var/lib/flatpak/exports/share";
            }
        }

        /// <summary>
        /// Resolves an icon name or absolute icon path to a PNG file usable by the UI, or null when no icon exists.
        /// SVG theme icons are rasterised into the temp icon cache.
        /// </summary>
        public static string ResolveIcon(string iconNameOrPath, int preferredSize = 64)
        {
            if (string.IsNullOrWhiteSpace(iconNameOrPath))
                return null;

            if (Path.IsPathRooted(iconNameOrPath))
                return File.Exists(iconNameOrPath) ? ToRasterIcon(iconNameOrPath, preferredSize) : null;

            var name = Path.GetFileNameWithoutExtension(iconNameOrPath);
            foreach (var theme in GetThemeSearchOrder())
            {
                foreach (var dataDir in DataDirectories)
                {
                    var themeDir = Path.Combine(dataDir, "icons", theme);
                    if (!Directory.Exists(themeDir))
                        continue;

                    var found = FindInTheme(themeDir, name, preferredSize);
                    if (found != null)
                        return ToRasterIcon(found, preferredSize);
                }
            }

            foreach (var dataDir in DataDirectories)
            {
                foreach (var ext in new[] { ".png", ".svg", ".xpm" })
                {
                    var pixmap = Path.Combine(dataDir, "pixmaps", name + ext);
                    if (File.Exists(pixmap))
                        return ToRasterIcon(pixmap, preferredSize);
                }
            }
            return null;
        }

        /// <summary>
        /// Icon for a file based on its mime type (through the icon theme), or null.
        /// </summary>
        public static string ResolveMimeIcon(string filePath, int preferredSize = 64)
        {
            var mime = QueryMimeType(filePath);
            if (string.IsNullOrEmpty(mime))
                return null;

            var candidates = new List<string> { mime.Replace('/', '-') };
            var slash = mime.IndexOf('/');
            if (slash > 0)
                candidates.Add(mime.Substring(0, slash) + "-x-generic");
            candidates.Add("text-x-generic");

            foreach (var candidate in candidates)
            {
                var icon = ResolveIcon(candidate, preferredSize);
                if (icon != null)
                    return icon;
            }
            return null;
        }

        /// <summary>
        /// Thumbnail generated by the desktop file manager for the file (freedesktop thumbnail spec), or null.
        /// </summary>
        public static string FindXdgThumbnail(string filePath)
        {
            var uri = new Uri(Path.GetFullPath(filePath)).AbsoluteUri;
            var hash = Convert.ToHexString(MD5.HashData(Encoding.UTF8.GetBytes(uri))).ToLowerInvariant();
            var cacheHome = Environment.GetEnvironmentVariable("XDG_CACHE_HOME");
            if (string.IsNullOrEmpty(cacheHome))
                cacheHome = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".cache");

            foreach (var size in new[] { "large", "normal", "x-large", "xx-large" })
            {
                var candidate = Path.Combine(cacheHome, "thumbnails", size, hash + ".png");
                if (File.Exists(candidate))
                    return candidate;
            }
            return null;
        }

        public static IReadOnlyList<DesktopEntry> GetDesktopEntries()
        {
            lock (desktopEntriesGate)
            {
                if (desktopEntriesCache != null)
                    return desktopEntriesCache;

                var entries = new List<DesktopEntry>();
                foreach (var dataDir in DataDirectories)
                {
                    var appsDir = Path.Combine(dataDir, "applications");
                    if (!Directory.Exists(appsDir))
                        continue;

                    IEnumerable<string> files;
                    try
                    {
                        files = Directory.EnumerateFiles(appsDir, "*.desktop", SearchOption.AllDirectories);
                    }
                    catch
                    {
                        continue;
                    }

                    foreach (var file in files)
                    {
                        var entry = ParseDesktopEntry(file);
                        if (entry != null)
                            entries.Add(entry);
                    }
                }
                desktopEntriesCache = entries;
                return entries;
            }
        }

        public static DesktopEntry ParseDesktopEntry(string path)
        {
            try
            {
                var entry = new DesktopEntry { Path = path };
                var inMainGroup = false;
                foreach (var rawLine in File.ReadLines(path))
                {
                    var line = rawLine.Trim();
                    if (line.StartsWith("["))
                    {
                        inMainGroup = line.Equals("[Desktop Entry]", StringComparison.Ordinal);
                        continue;
                    }
                    if (!inMainGroup || line.Length == 0 || line.StartsWith("#"))
                        continue;

                    var eq = line.IndexOf('=');
                    if (eq <= 0)
                        continue;

                    var key = line.Substring(0, eq).Trim();
                    var value = line.Substring(eq + 1).Trim();
                    switch (key)
                    {
                        case "Name":
                            entry.Name ??= value;
                            break;
                        case "Exec":
                            entry.Exec = value;
                            break;
                        case "Icon":
                            entry.Icon = value;
                            break;
                        case "NoDisplay":
                            entry.NoDisplay = value.Equals("true", StringComparison.OrdinalIgnoreCase);
                            break;
                    }
                }
                return entry.Name == null && entry.Exec == null ? null : entry;
            }
            catch
            {
                return null;
            }
        }

        /// <summary>
        /// Desktop entry whose Exec launches the given executable path, or null.
        /// </summary>
        public static DesktopEntry FindDesktopEntryForExecutable(string executablePath)
        {
            if (string.IsNullOrWhiteSpace(executablePath))
                return null;

            var fileName = Path.GetFileName(executablePath);
            DesktopEntry byName = null;
            foreach (var entry in GetDesktopEntries())
            {
                var exec = entry.ExecutableName;
                if (string.IsNullOrEmpty(exec))
                    continue;

                if (string.Equals(exec, executablePath, StringComparison.Ordinal))
                    return entry;
                if (byName == null && string.Equals(Path.GetFileName(exec), fileName, StringComparison.Ordinal))
                    byName = entry;
            }
            return byName;
        }

        /// <summary>
        /// Resolves a bare command name through PATH.
        /// </summary>
        public static string ResolveExecutablePath(string command)
        {
            if (string.IsNullOrWhiteSpace(command))
                return null;
            if (Path.IsPathRooted(command))
                return command;

            var path = Environment.GetEnvironmentVariable("PATH") ?? string.Empty;
            foreach (var dir in path.Split(':', StringSplitOptions.RemoveEmptyEntries))
            {
                var candidate = Path.Combine(dir, command);
                if (File.Exists(candidate))
                    return candidate;
            }
            return command;
        }

        private static IEnumerable<string> GetThemeSearchOrder()
        {
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var theme in new[] { GetKdeIconTheme(), GetGtkIconTheme(), "breeze", "breeze-dark", "Adwaita", "Papirus", "hicolor" })
            {
                if (!string.IsNullOrEmpty(theme) && seen.Add(theme))
                    yield return theme;
            }
        }

        private static string GetKdeIconTheme()
        {
            var config = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".config", "kdeglobals");
            return ReadIniValue(config, "[Icons]", "Theme");
        }

        private static string GetGtkIconTheme()
        {
            var config = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".config", "gtk-3.0", "settings.ini");
            return ReadIniValue(config, "[Settings]", "gtk-icon-theme-name");
        }

        private static string ReadIniValue(string file, string group, string key)
        {
            try
            {
                if (!File.Exists(file))
                    return null;

                var inGroup = false;
                foreach (var rawLine in File.ReadLines(file))
                {
                    var line = rawLine.Trim();
                    if (line.StartsWith("["))
                    {
                        inGroup = line.Equals(group, StringComparison.Ordinal);
                        continue;
                    }
                    if (inGroup && line.StartsWith(key + "=", StringComparison.Ordinal))
                        return line.Substring(key.Length + 1).Trim();
                }
            }
            catch { }
            return null;
        }

        private static string FindInTheme(string themeDir, string name, int preferredSize)
        {
            var sizes = preferredSizes.OrderBy(x => Math.Abs(x - preferredSize)).ToArray();
            foreach (var size in sizes)
            {
                foreach (var sizeDir in new[] { $"{size}x{size}", $"{size}x{size}@2x" })
                {
                    foreach (var context in contexts)
                    {
                        var png = Path.Combine(themeDir, sizeDir, context, name + ".png");
                        if (File.Exists(png))
                            return png;
                        var svg = Path.Combine(themeDir, sizeDir, context, name + ".svg");
                        if (File.Exists(svg))
                            return svg;
                    }
                }
            }
            foreach (var context in contexts)
            {
                var svg = Path.Combine(themeDir, "scalable", context, name + ".svg");
                if (File.Exists(svg))
                    return svg;
                var symbolic = Path.Combine(themeDir, "symbolic", context, name + "-symbolic.svg");
                if (File.Exists(symbolic))
                    return symbolic;
            }
            return null;
        }

        private static string ToRasterIcon(string iconPath, int size)
        {
            var ext = Path.GetExtension(iconPath).ToLowerInvariant();
            if (ext == ".png" || ext == ".jpg" || ext == ".jpeg")
                return iconPath;

            try
            {
                Directory.CreateDirectory(cacheDir);
                var hash = Convert.ToHexString(SHA1.HashData(Encoding.UTF8.GetBytes(iconPath + size)));
                var cached = Path.Combine(cacheDir, hash + ".png");
                if (File.Exists(cached))
                    return cached;

                var settings = new MagickReadSettings { BackgroundColor = MagickColors.Transparent, Width = (uint)size, Height = (uint)size };
                using var image = new MagickImage(iconPath, settings);
                image.Format = MagickFormat.Png;
                image.Write(cached);
                return cached;
            }
            catch
            {
                return null;
            }
        }

        private static string QueryMimeType(string filePath)
        {
            try
            {
                using var process = Process.Start(new ProcessStartInfo
                {
                    FileName = "xdg-mime",
                    ArgumentList = { "query", "filetype", filePath },
                    RedirectStandardOutput = true,
                    RedirectStandardError = true,
                    UseShellExecute = false,
                });
                var output = process.StandardOutput.ReadToEnd().Trim();
                process.WaitForExit(5000);
                return process.ExitCode == 0 ? output : null;
            }
            catch
            {
                return null;
            }
        }
    }
}
