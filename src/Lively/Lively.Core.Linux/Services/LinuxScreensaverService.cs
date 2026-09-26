using Lively.Common.Services;
using System;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// Lively's screensaver mode relies on Windows' .scr mechanism and DWM thumbnails; Linux
    /// sessions lock through the compositor instead. Every entry point reports that clearly.
    /// </summary>
    public sealed class LinuxScreensaverService : IScreensaverService
    {
        private const string Reason = "Screensaver mode is not available on Linux. Lock screens are provided by your desktop environment.";

        public ScreensaverApplyMode Mode => ScreensaverApplyMode.process;
        public bool IsRunning => false;

        public event EventHandler Stopped;

        public void CreatePreview(IntPtr hwnd) => throw new PlatformNotSupportedException(Reason);

        public Task StartAsync(bool isFadeIn) => throw new PlatformNotSupportedException(Reason);

        public Task StopAsync()
        {
            Stopped?.Invoke(this, EventArgs.Empty);
            return Task.CompletedTask;
        }

        public void StartIdleTimer(uint idleTime) => throw new PlatformNotSupportedException(Reason);

        public void StopIdleTimer() { }
    }
}
