using CommandLine;
using GrpcDotNetNamedPipes;
using Lively.Commandline;
using Lively.Common;
using Lively.Common.Exceptions;
using Lively.Common.Factories;
using Lively.Common.Linux.Audio;
using Lively.Common.Linux.DBus;
using Lively.Common.Linux.DBus.Activities;
using Lively.Common.Linux.DBus.Notifications;
using Lively.Common.Linux.DBus.StatusNotifier;
using Lively.Common.Linux.Hardware;
using Lively.Common.Linux.Media;
using Lively.Common.Linux.NowPlaying;
using Lively.Common.Linux.Platform;
using Lively.Common.Linux.Power;
using Lively.Common.Services;
using Lively.Core.Display;
using Lively.Core.Linux.Backends;
using Lively.Core.Linux.Commandline;
using Lively.Core.Linux.Core;
using Lively.Core.Linux.Display;
using Lively.Core.Linux.Factories;
using Lively.Core.Linux.RPC;
using Lively.Core.Linux.Services;
using Lively.Core.Suspend;
using Lively.Factories;
using Lively.Grpc.Common.Proto.Commands;
using Lively.Grpc.Common.Proto.Desktop;
using Lively.Grpc.Common.Proto.Display;
using Lively.Grpc.Common.Proto.Settings;
using Lively.Grpc.Common.Proto.Update;
using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Services;
using Lively.RPC;
using Lively.Services;
using Microsoft.Extensions.DependencyInjection;
using System;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Core.Linux
{
    /// <summary>
    /// The Lively core daemon for Linux: owns wallpapers, serves the gRPC API the UI and CLI talk to,
    /// keeps the tray icon, and launches the desktop UI on demand.
    /// </summary>
    public static class Program
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private static Mutex mutex;

        public static async Task<int> Main(string[] args)
        {
            NLog.LogManager.Configuration = new NLog.Config.XmlLoggingConfiguration(Path.Combine(AppContext.BaseDirectory, "Nlog.config"));

            if (!AcquireMutex())
            {
                // Another core is running: hand it our arguments (or ask it to show the UI) and exit.
                try
                {
                    var client = new CommandsService.CommandsServiceClient(new NamedPipeChannel(".", Constants.SingleInstance.GrpcPipeServerName));
                    var request = new AutomationCommandRequest();
                    request.Args.AddRange(args.Length != 0 ? args : new[] { "--showApp", "true" });
                    await client.AutomationCommandAsync(request);
                    return 0;
                }
                catch (Exception e)
                {
                    Console.Error.WriteLine($"Lively core is already running but could not be reached: {e.Message}");
                    return 1;
                }
            }

            if (!string.Equals(Environment.GetEnvironmentVariable("XDG_SESSION_TYPE"), "wayland", StringComparison.OrdinalIgnoreCase)
                && string.IsNullOrEmpty(Environment.GetEnvironmentVariable("WAYLAND_DISPLAY")))
            {
                Console.Error.WriteLine("Lively for Linux needs a Wayland session (WAYLAND_DISPLAY is not set). X11 sessions are not supported.");
                ReleaseMutex();
                return 2;
            }

            SetupUnhandledExceptionLogging();
            Logger.Info($"Lively core (Linux) starting, base directory {AppContext.BaseDirectory}");

            NamedPipeServer grpcServer = null;
            ServiceProvider services = null;
            WaylandMonitorService monitor = null;
            PowerStateService power = null;
            KdeActivityTracker activities = null;
            var exitCode = 0;
            try
            {
                var helperLocator = new NativeHelperLocator();
                if (KdeInterfaceRegistration.IsKdeSession())
                    await new KdeInterfaceRegistration(helperLocator.Resolve("lively-wl-monitor")).EnsureAsync();

                monitor = new WaylandMonitorService(helperLocator);
                await monitor.StartAsync(TimeSpan.FromSeconds(10));

                power = new PowerStateService(TimeSpan.FromSeconds(2));
                await power.StartAsync();

                activities = new KdeActivityTracker(DBusConnections.Session);
                await activities.StartAsync();

                services = ConfigureServices(monitor, power, activities);

                var lifetime = services.GetRequiredService<LinuxAppLifetimeService>();
                var userSettings = services.GetRequiredService<IUserSettingsService>();
                services.GetRequiredService<AppInitializer>().Run();
                services.GetRequiredService<IResourceService>().SetCulture(userSettings.Settings.Language);

                var desktopCore = services.GetRequiredService<LinuxDesktopCore>();
                await desktopCore.InitializeAsync();
                Logger.Info($"Desktop backend: {desktopCore.Backend.Name}");

                grpcServer = ConfigureGrpcServer(services);

                services.GetRequiredService<IPlayback>().Start();
                var systray = services.GetRequiredService<ISystray>();
                await ((StatusNotifierSystray)systray).StartAsync();
                systray.Visibility(userSettings.Settings.SysTrayIcon);
                services.GetRequiredService<WallpaperFeedService>();

                var playback = (LinuxPlayback)services.GetRequiredService<IPlayback>();
                if (!playback.IsWindowTrackingAvailable)
                {
                    var message = "This compositor exposes no window list (wlr-foreign-toplevel-management or plasma-window-management), so pausing wallpapers under fullscreen or focused apps is unavailable here.";
                    Logger.Error(message);
                    desktopCore.ReportError(new WallpaperPluginException(message));
                }

                desktopCore.WallpaperError += (s, e) =>
                {
                    if (!services.GetRequiredService<IRunnerService>().IsVisibleUI)
                        systray.ShowBalloonNotification(4000, services.GetRequiredService<IResourceService>().GetString("TextError"), e.Message);
                };

                desktopCore.RestoreWallpaper();

                if (userSettings.Settings.IsFirstRun || args.Contains("--showApp"))
                    services.GetRequiredService<IRunnerService>().ShowUI();

                if (args.Length != 0)
                    services.GetRequiredService<ICommandHandler>().ParseArgs(args);

                var updater = services.GetRequiredService<IAppUpdaterService>();
                updater.UpdateChecked += (s, e) =>
                {
                    Logger.Info($"AppUpdate status: {e.UpdateStatus}");
                    if (e.UpdateStatus == AppUpdateStatus.available && !services.GetRequiredService<IRunnerService>().IsVisibleUI)
                        systray.ShowBalloonNotification(4000, "Lively Wallpaper", services.GetRequiredService<IResourceService>().GetString("TextUpdateAvailable"));
                };
                _ = updater.CheckUpdate(30 * 1000);
                updater.Start();

                using var sigterm = PosixSignalRegistration.Create(PosixSignal.SIGTERM, ctx => { ctx.Cancel = true; lifetime.Quit(); });
                using var sigint = PosixSignalRegistration.Create(PosixSignal.SIGINT, ctx => { ctx.Cancel = true; lifetime.Quit(); });
                using var sighup = PosixSignalRegistration.Create(PosixSignal.SIGHUP, ctx => { ctx.Cancel = true; lifetime.Quit(); });

                Logger.Info("Core running.");
                try
                {
                    await Task.Delay(Timeout.Infinite, lifetime.StoppingToken);
                }
                catch (OperationCanceledException)
                {
                }
                Logger.Info("Shutting down..");
            }
            catch (Exception ex)
            {
                Logger.Fatal(ex);
                Console.Error.WriteLine($"Lively core failed to start: {ex.Message}");
                exitCode = 1;
            }
            finally
            {
                grpcServer?.Dispose();
                services?.Dispose();
                activities?.Dispose();
                power?.Dispose();
                monitor?.Dispose();
                ReleaseMutex();
                NLog.LogManager.Shutdown();
            }
            return exitCode;
        }

        private static ServiceProvider ConfigureServices(WaylandMonitorService monitor, PowerStateService power, KdeActivityTracker activities)
        {
            var sessionBus = Connection.Session;
            var helpers = new NativeHelperLocator();
            var lifetime = new LinuxAppLifetimeService();
            var notifications = new NotificationService();
            var trayIcon = Path.Combine(AppContext.BaseDirectory, "Assets", "tray-icon.png");
            var uiCommand = Environment.GetEnvironmentVariable("LIVELY_UI_COMMAND") ?? Path.Combine(AppContext.BaseDirectory, "plugins", "UI", "Lively.UI.Avalonia");
            var coreCommand = Path.Combine(AppContext.BaseDirectory, "Lively.Core.Linux");

            var collection = new ServiceCollection()
                .AddSingleton(monitor)
                .AddSingleton(power)
                .AddSingleton(activities)
                .AddSingleton(sessionBus)
                .AddSingleton(helpers)
                .AddSingleton(lifetime)
                .AddSingleton<IAppLifetimeService>(lifetime)
                .AddSingleton(notifications)
                .AddSingleton<LinuxDisplayManager>()
                .AddSingleton<IDisplayManager>(sp => sp.GetRequiredService<LinuxDisplayManager>())
                .AddSingleton<IUserSettingsService, UserSettingsService>()
                .AddSingleton<IResourceService, LinuxResourceService>()
                .AddSingleton<IDispatcherService, InlineDispatcherService>()
                .AddSingleton<IPlatformInfo, LinuxPlatformInfo>()
                .AddSingleton<IStartupService>(new XdgAutostartService(coreCommand))
                .AddSingleton<ITaskbarThemeService, LinuxTaskbarThemeService>()
                .AddSingleton<IAppThemeService, LinuxAppThemeService>()
                .AddSingleton<IScreensaverService, LinuxScreensaverService>()
                .AddSingleton<IRunnerService, LinuxRunnerService>()
                .AddSingleton<IPlayback, LinuxPlayback>()
                .AddSingleton<HostWallpaperFactory>()
                .AddSingleton<IWallpaperBackend>(sp => CreateBackend(sp, monitor, sessionBus))
                .AddSingleton<LinuxDesktopCore>()
                .AddSingleton<IDesktopCore>(sp => sp.GetRequiredService<LinuxDesktopCore>())
                .AddSingleton<IWindowService, LinuxWindowService>()
                .AddSingleton<ISystray>(sp => CreateSystray(sp, notifications, trayIcon))
                .AddSingleton<IAppUpdaterService, LinuxGithubUpdaterService>()
                .AddSingleton<IAudioVisualizerService, PulseAudioVisualizerService>()
                .AddSingleton<IHardwareUsageService, LinuxHardwareUsageService>()
                .AddSingleton<INowPlayingService, MprisNowPlayingService>()
                .AddSingleton<WallpaperFeedService>()
                .AddSingleton<DesktopCoreServer>()
                .AddSingleton<DisplayManagerServer>()
                .AddSingleton<UserSettingsServer>()
                .AddSingleton<CommandsServer>()
                .AddSingleton<AppUpdateServer, LinuxAppUpdateServer>()
                .AddTransient<AppInitializer>()
                .AddTransient<IThumbnailService, LinuxThumbnailService>()
                .AddTransient<IWallpaperLibraryFactory, WallpaperLibraryFactory>()
                .AddTransient<IWallpaperPluginFactory, LinuxWallpaperPluginFactory>()
                .AddTransient<ILivelyPropertyFactory, LivelyPropertyFactory>()
                .AddTransient<ICommandHandler, LinuxCommandHandler>()
                .AddTransient<IDownloadService, HttpDownloadService>()
                .AddHttpClient();

            return collection.BuildServiceProvider();
        }

        /// <summary>
        /// KDE gets the Plasma plugin (the only way under the desktop icons); everything else that
        /// offers layer-shell gets the native hosts. LIVELY_BACKEND=plasma|layer-shell overrides.
        /// </summary>
        private static IWallpaperBackend CreateBackend(IServiceProvider sp, WaylandMonitorService monitor, Connection sessionBus)
        {
            var forced = Environment.GetEnvironmentVariable("LIVELY_BACKEND");
            var desktop = Environment.GetEnvironmentVariable("XDG_CURRENT_DESKTOP") ?? string.Empty;
            var isKde = desktop.Split(':').Any(d => d.Equals("KDE", StringComparison.OrdinalIgnoreCase));
            var usePlasma = forced switch
            {
                "plasma" => true,
                "layer-shell" => false,
                _ => isKde || (monitor.Capabilities.PlasmaShell && !monitor.Capabilities.LayerShell),
            };

            if (usePlasma)
                return new PlasmaBackend(sessionBus, sp.GetRequiredService<LinuxDisplayManager>(), sp.GetRequiredService<IUserSettingsService>(),
                    sp.GetRequiredService<ILivelyPropertyFactory>(), sp.GetRequiredService<HostWallpaperFactory>());

            return new LayerShellBackend(monitor, sp.GetRequiredService<IDisplayManager>(), sp.GetRequiredService<HostWallpaperFactory>(),
                sp.GetRequiredService<ILivelyPropertyFactory>(), sp.GetRequiredService<IUserSettingsService>());
        }

        private static ISystray CreateSystray(IServiceProvider sp, NotificationService notifications, string trayIcon)
        {
            var runner = sp.GetRequiredService<IRunnerService>();
            var desktopCore = sp.GetRequiredService<IDesktopCore>();
            var playback = sp.GetRequiredService<IPlayback>();
            var windowService = sp.GetRequiredService<IWindowService>();
            var lifetime = sp.GetRequiredService<IAppLifetimeService>();
            var i18n = sp.GetRequiredService<IResourceService>();
            var updater = sp.GetRequiredService<IAppUpdaterService>();
            var commands = sp.GetRequiredService<ICommandHandler>();

            var tray = new TrayCommands
            {
                OpenApp = runner.ShowUI,
                CloseWallpapers = desktopCore.CloseAllWallpapers,
                TogglePause = () => playback.WallpaperPlaybackPolicy = playback.WallpaperPlaybackPolicy == PlaybackPolicy.alwaysPaused
                    ? PlaybackPolicy.automatic : PlaybackPolicy.alwaysPaused,
                ChangeWallpaper = () => commands.ParseArgs(new[] { "setwp", "--random", "true" }),
                CustomiseWallpaper = runner.ShowCustomisWallpaperePanel,
                ShowUpdatePage = runner.ShowAppUpdatePage,
                ReportBug = windowService.ShowDiagnosticWindow,
                Exit = lifetime.Quit,
                IsPaused = () => playback.WallpaperPlaybackPolicy == PlaybackPolicy.alwaysPaused,
                CanCustomise = () => desktopCore.Wallpapers.Any(x => x.LivelyPropertyCopyPath != null),
                UpdateStatus = () => updater.Status,
                ShowUpdateItem = true,
                GetString = i18n.GetString,
            };
            var systray = new StatusNotifierSystray(tray, notifications, trayIcon, trayIcon);
            playback.PlaybackPolicyChanged += (s, e) => systray.RefreshState();
            desktopCore.WallpaperChanged += (s, e) => systray.RefreshState();
            updater.UpdateChecked += (s, e) => systray.RefreshState();
            return systray;
        }

        private static NamedPipeServer ConfigureGrpcServer(IServiceProvider services)
        {
            var server = new NamedPipeServer(Constants.SingleInstance.GrpcPipeServerName);
            DesktopService.BindService(server.ServiceBinder, services.GetRequiredService<DesktopCoreServer>());
            SettingsService.BindService(server.ServiceBinder, services.GetRequiredService<UserSettingsServer>());
            DisplayService.BindService(server.ServiceBinder, services.GetRequiredService<DisplayManagerServer>());
            CommandsService.BindService(server.ServiceBinder, services.GetRequiredService<CommandsServer>());
            UpdateService.BindService(server.ServiceBinder, services.GetRequiredService<AppUpdateServer>());
            server.Start();
            Logger.Info($"gRPC server started on pipe {Constants.SingleInstance.GrpcPipeServerName}");
            return server;
        }

        private static bool AcquireMutex()
        {
            mutex = new Mutex(true, Constants.SingleInstance.MutexName, out bool created);
            if (!created)
            {
                mutex.Dispose();
                mutex = null;
                return false;
            }
            return true;
        }

        private static void ReleaseMutex()
        {
            try { mutex?.ReleaseMutex(); } catch (ApplicationException) { }
            mutex?.Dispose();
            mutex = null;
        }

        private static void SetupUnhandledExceptionLogging()
        {
            AppDomain.CurrentDomain.UnhandledException += (s, e) => Logger.Fatal(e.ExceptionObject as Exception, "Unhandled exception");
            TaskScheduler.UnobservedTaskException += (s, e) => { Logger.Error(e.Exception, "Unobserved task exception"); e.SetObserved(); };
        }
    }
}
