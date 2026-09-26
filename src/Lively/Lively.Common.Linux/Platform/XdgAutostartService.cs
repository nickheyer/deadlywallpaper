using Lively.Common.Services;
using System;
using System.IO;
using System.Text;
using System.Threading.Tasks;

namespace Lively.Common.Linux.Platform
{
    /// <summary>
    /// Launch-at-login through the XDG autostart specification: writes or removes
    /// <c>$XDG_CONFIG_HOME/autostart/lively-wallpaper.desktop</c> (default <c>~/.config/autostart</c>).
    /// </summary>
    public sealed class XdgAutostartService : IStartupService
    {
        public const string DesktopFileName = "lively-wallpaper.desktop";
        public const string ApplicationName = "Lively Wallpaper";

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private static readonly UTF8Encoding Utf8WithoutBom = new(encoderShouldEmitUTF8Identifier: false);

        private readonly string execCommand;

        /// <param name="execCommand">
        /// Value of the <c>Exec=</c> key: the command that starts Lively, quoted per the desktop entry specification
        /// when it contains spaces (for example <c>"/opt/lively/Lively" --silent</c>).
        /// </param>
        public XdgAutostartService(string execCommand)
            : this(execCommand, GetDefaultAutostartDirectory())
        {
        }

        public XdgAutostartService(string execCommand, string autostartDirectory)
        {
            if (string.IsNullOrWhiteSpace(execCommand))
                throw new ArgumentException("The Exec command must not be empty.", nameof(execCommand));
            if (string.IsNullOrWhiteSpace(autostartDirectory))
                throw new ArgumentException("The autostart directory must not be empty.", nameof(autostartDirectory));

            this.execCommand = execCommand;
            AutostartDirectory = autostartDirectory;
            DesktopFilePath = Path.Combine(autostartDirectory, DesktopFileName);
        }

        public string AutostartDirectory { get; }

        public string DesktopFilePath { get; }

        /// <summary>
        /// <c>$XDG_CONFIG_HOME/autostart</c>, or <c>~/.config/autostart</c> when the variable is unset or empty.
        /// </summary>
        public static string GetDefaultAutostartDirectory()
        {
            var configHome = Environment.GetEnvironmentVariable("XDG_CONFIG_HOME");
            if (string.IsNullOrWhiteSpace(configHome))
                configHome = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".config");
            return Path.Combine(configHome, "autostart");
        }

        /// <summary>
        /// The desktop entry that <see cref="TrySetStartupAsync"/> writes when startup is enabled.
        /// </summary>
        public string BuildDesktopEntry()
        {
            var builder = new StringBuilder();
            builder.Append("[Desktop Entry]\n");
            builder.Append("Type=Application\n");
            builder.Append("Name=").Append(ApplicationName).Append('\n');
            builder.Append("Comment=Animated desktop wallpaper\n");
            builder.Append("Exec=").Append(execCommand).Append('\n');
            builder.Append("Icon=lively-wallpaper\n");
            builder.Append("Terminal=false\n");
            builder.Append("Hidden=false\n");
            builder.Append("X-GNOME-Autostart-enabled=true\n");
            return builder.ToString();
        }

        /// <summary>
        /// Writes the autostart entry (<paramref name="enabled"/>) or deletes it, then reports whether the file
        /// on disk matches the request: the exact entry content when enabling, no file when disabling.
        /// Filesystem failures are logged and reported as <c>false</c>.
        /// </summary>
        public Task<bool> TrySetStartupAsync(bool enabled)
        {
            try
            {
                if (enabled)
                {
                    Directory.CreateDirectory(AutostartDirectory);
                    File.WriteAllText(DesktopFilePath, BuildDesktopEntry(), Utf8WithoutBom);
                }
                else if (File.Exists(DesktopFilePath))
                {
                    File.Delete(DesktopFilePath);
                }
                return Task.FromResult(MatchesRequest(enabled));
            }
            catch (Exception ex) when (ex is IOException || ex is UnauthorizedAccessException)
            {
                Logger.Error(ex, "Failed to {0} the autostart entry {1}", enabled ? "write" : "remove", DesktopFilePath);
                return Task.FromResult(false);
            }
        }

        private bool MatchesRequest(bool enabled)
        {
            if (!enabled)
                return !File.Exists(DesktopFilePath);
            return File.Exists(DesktopFilePath)
                && File.ReadAllText(DesktopFilePath, Utf8WithoutBom) == BuildDesktopEntry();
        }
    }
}
