using Lively.Common.Linux.Platform;
using System;
using System.IO;
using System.Threading.Tasks;
using Xunit;

namespace Lively.Common.Linux.Tests
{
    public sealed class XdgAutostartServiceTests : IDisposable
    {
        private const string ExecCommand = "\"/opt/lively wallpaper/Lively\" --silent";
        private readonly string configHome;
        private readonly string previousConfigHome;

        public XdgAutostartServiceTests()
        {
            configHome = Path.Combine(Path.GetTempPath(), "lively-autostart-tests", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(configHome);
            previousConfigHome = Environment.GetEnvironmentVariable("XDG_CONFIG_HOME");
            Environment.SetEnvironmentVariable("XDG_CONFIG_HOME", configHome);
        }

        public void Dispose()
        {
            Environment.SetEnvironmentVariable("XDG_CONFIG_HOME", previousConfigHome);
            Directory.Delete(configHome, recursive: true);
        }

        [Fact]
        public void DefaultDirectoryComesFromXdgConfigHome()
        {
            var service = new XdgAutostartService(ExecCommand);

            Assert.Equal(Path.Combine(configHome, "autostart"), service.AutostartDirectory);
            Assert.Equal(Path.Combine(configHome, "autostart", "lively-wallpaper.desktop"), service.DesktopFilePath);
        }

        [Fact]
        public void DefaultDirectoryFallsBackToHomeConfigWhenVariableIsUnset()
        {
            Environment.SetEnvironmentVariable("XDG_CONFIG_HOME", null);

            var expected = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".config", "autostart");
            Assert.Equal(expected, XdgAutostartService.GetDefaultAutostartDirectory());
        }

        [Fact]
        public async Task EnablingWritesTheDesktopEntry()
        {
            var service = new XdgAutostartService(ExecCommand);

            var result = await service.TrySetStartupAsync(true);

            Assert.True(result);
            Assert.True(File.Exists(service.DesktopFilePath));
            var lines = File.ReadAllLines(service.DesktopFilePath);
            Assert.Equal("[Desktop Entry]", lines[0]);
            Assert.Contains("Type=Application", lines);
            Assert.Contains("Name=Lively Wallpaper", lines);
            Assert.Contains("Exec=" + ExecCommand, lines);
            Assert.Contains("Hidden=false", lines);
            Assert.Contains("X-GNOME-Autostart-enabled=true", lines);
        }

        [Fact]
        public async Task EnablingTwiceKeepsASingleValidEntry()
        {
            var service = new XdgAutostartService(ExecCommand);

            Assert.True(await service.TrySetStartupAsync(true));
            Assert.True(await service.TrySetStartupAsync(true));

            Assert.Equal(service.BuildDesktopEntry(), File.ReadAllText(service.DesktopFilePath));
            Assert.Single(Directory.GetFiles(service.AutostartDirectory));
        }

        [Fact]
        public async Task EnablingReplacesAForeignEntryWithTheSameName()
        {
            var service = new XdgAutostartService(ExecCommand);
            Directory.CreateDirectory(service.AutostartDirectory);
            File.WriteAllText(service.DesktopFilePath, "[Desktop Entry]\nHidden=true\n");

            Assert.True(await service.TrySetStartupAsync(true));

            Assert.Equal(service.BuildDesktopEntry(), File.ReadAllText(service.DesktopFilePath));
        }

        [Fact]
        public async Task DisablingRemovesTheDesktopEntry()
        {
            var service = new XdgAutostartService(ExecCommand);
            Assert.True(await service.TrySetStartupAsync(true));

            var result = await service.TrySetStartupAsync(false);

            Assert.True(result);
            Assert.False(File.Exists(service.DesktopFilePath));
        }

        [Fact]
        public async Task DisablingWhenNoEntryExistsReportsMatchingState()
        {
            var service = new XdgAutostartService(ExecCommand);

            Assert.True(await service.TrySetStartupAsync(false));
            Assert.False(File.Exists(service.DesktopFilePath));
        }

        [Fact]
        public async Task EnablingReportsFalseWhenTheDirectoryCannotBeCreated()
        {
            var blocker = Path.Combine(configHome, "blocked");
            File.WriteAllText(blocker, string.Empty);
            var service = new XdgAutostartService(ExecCommand, Path.Combine(blocker, "autostart"));

            Assert.False(await service.TrySetStartupAsync(true));
        }

        [Fact]
        public void EmptyExecCommandIsRejected()
        {
            Assert.Throws<ArgumentException>(() => new XdgAutostartService(" "));
        }
    }
}
