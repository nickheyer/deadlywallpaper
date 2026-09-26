using Lively.Common.Services;
using Lively.Models.Enums;
using System;
using System.Threading;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// The core has no UI thread; dispatched work simply runs where it is queued.
    /// </summary>
    public sealed class InlineDispatcherService : IDispatcherService
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        public bool TryEnqueue(Action action)
        {
            try
            {
                action();
                return true;
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
                return false;
            }
        }
    }

    /// <summary>
    /// Requests the process to stop; Program.cs waits on the token and does the orderly shutdown.
    /// </summary>
    public sealed class LinuxAppLifetimeService : IAppLifetimeService
    {
        private readonly CancellationTokenSource cts = new CancellationTokenSource();

        public CancellationToken StoppingToken => cts.Token;

        public void Quit()
        {
            if (!cts.IsCancellationRequested)
                cts.Cancel();
        }
    }

    /// <summary>
    /// The Windows taskbar theming (TranslucentTB integration) has no counterpart on Linux desktops.
    /// </summary>
    public sealed class LinuxTaskbarThemeService : ITaskbarThemeService
    {
        public void Apply(TaskbarTheme theme)
        {
            if (theme != TaskbarTheme.none)
                throw new PlatformNotSupportedException("Taskbar theming is a Windows feature; Linux panels are themed by the desktop environment.");
        }
    }

    /// <summary>
    /// The core's only themed element on Linux is the tray icon.
    /// </summary>
    public sealed class LinuxAppThemeService : IAppThemeService
    {
        private readonly ISystray systray;

        public LinuxAppThemeService(ISystray systray)
        {
            this.systray = systray;
        }

        public void ChangeTheme(AppTheme theme) => systray.SetTheme(theme);
    }
}
