using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using Avalonia.Styling;
using Avalonia.Threading;
using Lively.Common;
using Lively.Common.Factories;
using Lively.Common.Helpers;
using Lively.Common.Linux.DBus.SecretService;
using Lively.Common.Linux.Media;
using Lively.Common.Services;
using Lively.Gallery.Client;
using Lively.Grpc.Client;
using Lively.ML.DepthEstimate;
using Lively.Models;
using Lively.Models.Enums;
using Lively.UI.Avalonia.Controls;
using Lively.UI.Avalonia.Converters;
using Lively.UI.Avalonia.Localization;
using Lively.UI.Avalonia.Services;
using Lively.UI.Avalonia.Views;
using Lively.UI.Avalonia.Views.LivelyProperty;
using Lively.UI.Shared.Factories;
using Lively.UI.Shared.Services;
using Lively.UI.Shared.ViewModels;
using Microsoft.Extensions.DependencyInjection;
using Newtonsoft.Json;
using System;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia
{
    public partial class App : Application
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private const string WallpaperDataPrefix = "LM WALLPAPERDATA";

        private ServiceProvider services;
        private ResourceService resourceService;
        private LinuxAccentColorService accentColors;
        private MainWindow mainWindow;
        private StdioCommandChannel stdio;
        private LivelyPropertiesTrayWindow trayWindow;
        private bool isConnecting;
        private bool isBusyRequested;
        private bool isCleanedUp;

        /// <summary>
        /// Service container, available once the wallpaper core has been reached.
        /// </summary>
        public static IServiceProvider Services =>
            ((App)Current).services ?? throw new InvalidOperationException("The wallpaper core is not connected; services are unavailable.");

        public static bool IsCoreConnected => ((App)Current).services != null;

        public static StartArgs StartFlags { get; set; } = new();

        public static SingleInstanceGuard SingleInstance { get; set; }

        /// <summary>
        /// Visual whose top level owns file pickers: the tray widget while it is shown, otherwise the main window.
        /// </summary>
        public static Visual CurrentTopLevel
        {
            get
            {
                var app = (App)Current;
                return app.trayWindow is { IsVisible: true } tray ? tray : app.mainWindow;
            }
        }

        public override void Initialize()
        {
            AvaloniaXamlLoader.Load(this);
        }

        public override void OnFrameworkInitializationCompleted()
        {
            if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
            {
                // Closing the window hides it in core-managed mode and the tray widget has no main window, so the
                // lifetime never ends on its own; Shutdown() is called explicitly.
                desktop.ShutdownMode = ShutdownMode.OnExplicitShutdown;
                desktop.Exit += (_, _) => Cleanup();
                SetupUnhandledExceptionLogging();

                resourceService = new ResourceService();
                LocalizationSource.Initialize(resourceService);

                // The desktop accent applies to every page, including the "core unavailable" one.
                accentColors = new LinuxAccentColorService();
                _ = accentColors.StartAsync();

                mainWindow = new MainWindow();
                mainWindow.RetryRequested += (_, _) => _ = ConnectCoreAsync(TimeSpan.FromSeconds(3));
                mainWindow.VisibilityChanged += (_, visible) => ReportVisibility(visible);
                desktop.MainWindow = mainWindow;
                DialogHost.Register(mainWindow.DialogHost);

                stdio = new StdioCommandChannel();
                stdio.CommandReceived += (_, line) => HandleCommand(line);
                stdio.InputClosed += (_, _) => OnInputClosed();
                if (SingleInstance != null)
                    SingleInstance.CommandReceived += (_, line) => Dispatcher.UIThread.Post(() => HandleCommand(line));
                stdio.Start();
                ReportVisibility(false);

                _ = StartAsync();
            }

            base.OnFrameworkInitializationCompleted();
        }

        private async Task StartAsync()
        {
            // The core launches this process right after its gRPC server starts; give it time to accept connections.
            var connected = await ConnectCoreAsync(StartFlags.CoreManaged ? TimeSpan.FromSeconds(20) : TimeSpan.FromSeconds(2));
            if (!connected)
            {
                ShowMainWindow();
                return;
            }

            if (StartFlags.TrayWidget)
            {
                ShowTrayWidget();
            }
            else
            {
                ShowMainWindow();
                if (StartFlags.AppUpdate)
                    services.GetRequiredService<IMainNavigator>().NavigateTo(ContentPageType.appupdate);
            }
        }

        /// <summary>
        /// Waits for the core, builds the service container and initialises the main window. Returns false and leaves the
        /// window in its "core unavailable" state when the core cannot be reached within <paramref name="timeout"/>.
        /// </summary>
        private async Task<bool> ConnectCoreAsync(TimeSpan timeout)
        {
            if (services != null)
                return true;
            if (isConnecting)
                return false;

            isConnecting = true;
            try
            {
                mainWindow.ShowCoreConnecting();
                var status = await WaitForCoreAsync(timeout);
                if (!status.IsAvailable)
                {
                    Logger.Warn($"Wallpaper core unavailable: {status.Details}");
                    mainWindow.ShowCoreUnavailable(status.Details);
                    return false;
                }

                try
                {
                    services = ConfigureServices();
                    InitializeUi();
                }
                catch (Exception ex)
                {
                    Logger.Error(ex);
                    services?.Dispose();
                    services = null;
                    mainWindow.ShowCoreUnavailable(ex.Message);
                    return false;
                }
                return true;
            }
            finally
            {
                isConnecting = false;
            }
        }

        private static async Task<CoreConnectionStatus> WaitForCoreAsync(TimeSpan timeout)
        {
            var deadline = DateTime.UtcNow + timeout;
            CoreConnectionStatus status;
            do
            {
                status = AppLifeCycleUtil.IsAppMutexRunning(Constants.SingleInstance.MutexName)
                    ? await CoreConnectionProbe.ProbeAsync(TimeSpan.FromSeconds(2))
                    : new CoreConnectionStatus(false, $"No process owns the core mutex {Constants.SingleInstance.MutexName}.");
                if (status.IsAvailable)
                    return status;

                await Task.Delay(500);
            } while (DateTime.UtcNow < deadline);
            return status;
        }

        private void InitializeUi()
        {
            var userSettings = services.GetRequiredService<IUserSettingsClient>();
            SetAppTheme(userSettings.Settings.ApplicationTheme);
            ImageLoader.Cache = services.GetRequiredService<ICacheService>();
            mainWindow.Initialize(services);
            if (isBusyRequested)
                services.GetRequiredService<LibraryViewModel>().IsBusy = true;
        }

        private ServiceProvider ConfigureServices()
        {
            return new ServiceCollection()
                // Singleton
                .AddSingleton<IDesktopCoreClient, WinDesktopCoreClient>()
                .AddSingleton<IUserSettingsClient, UserSettingsClient>()
                .AddSingleton<IDisplayManagerClient, DisplayManagerClient>()
                .AddSingleton<ICommandsClient, CommandsClient>()
                .AddSingleton<IAppUpdaterClient, AppUpdaterClient>()
                .AddSingleton<IDialogService, DialogService>()
                .AddSingleton<IDispatcherService, DispatcherService>()
                .AddSingleton<IResourceService>(resourceService)
                .AddSingleton<IMainNavigator, MainNavigator>()
                .AddSingleton(mainWindow)
                .AddSingleton<MainViewModel>()
                .AddSingleton(e => new GalleryClient(e.GetRequiredService<IHttpClientFactory>(), "http://api.livelywallpaper.net/api/",
                    "https://accounts.google.com/o/oauth2/auth/oauthchooseaccount?client_id=923081992071-qg27j4uhasb3r4lasb9cb19nbhvgbb34.apps.googleusercontent.com&redirect_uri=http://127.0.0.1:43821/signin-oidc&scope=email%20openid%20profile&response_type=code&state=asdafwswdwefwsdg&flowName=GeneralOAuthFlow",
                    "https://github.com/login/oauth/authorize?client_id=bbfd46fbb54895ecee74&redirect_uri=http://127.0.0.1:43821/signin-oidc-github&scope=user:email",
                    new JsonTokenStore(new SecretServiceTokenProtector())))
                .AddSingleton<LibraryViewModel>() //Storing and tracking library items.
                .AddSingleton<GalleryViewModel>()
                .AddSingleton<GallerySubscriptionViewModel>()
                .AddSingleton<AppUpdateViewModel>()
                .AddSingleton<ICacheService, DiskCacheService>(e => new DiskCacheService(e.GetRequiredService<IHttpClientFactory>(), Path.Combine(Path.GetTempPath(), "Lively Wallpaper", "gallery")))
                .AddSingleton<IDepthEstimate, MiDaS>()
                .AddSingleton<WebPageTextFetcher>()
                .AddSingleton<LinuxScreenColorPicker>()
                // Scoped: every control panel dialog runs in its own scope and shares one view model with its pages.
                .AddScoped<IDialogNavigator, DialogNavigator>()
                .AddScoped<ControlPanelViewModel>()
                // Transient
                .AddTransient<AboutViewModel>()
                .AddTransient<CustomiseWallpaperViewModel>()
                .AddTransient<PatreonSupportersViewModel>()
                .AddTransient<AddWallpaperViewModel>()
                .AddTransient<ScreensaverLayoutViewModel>()
                .AddTransient<WallpaperLayoutViewModel>()
                .AddTransient<ChooseDisplayViewModel>()
                .AddTransient<FindMoreAppsViewModel>()
                .AddTransient<AppThemeViewModel>()
                .AddTransient<GalleryLoginViewModel>()
                .AddTransient<ManageAccountViewModel>()
                .AddTransient<RestoreWallpaperViewModel>()
                .AddTransient<AddWallpaperCreateViewModel>()
                .AddTransient<DepthEstimateWallpaperViewModel>()
                .AddTransient<SettingsGeneralViewModel>()
                .AddTransient<SettingsPerformanceViewModel>()
                .AddTransient<SettingsWallpaperViewModel>()
                .AddTransient<SettingsScreensaverViewModel>()
                .AddTransient<SettingsSystemViewModel>()
                .AddTransient<ShareWallpaperViewModel>()
                .AddTransient<AddWallpaperDataViewModel>()
                .AddTransient<IFileService, FileService>()
                .AddTransient<IApplicationsFactory, LinuxApplicationsFactory>()
                .AddTransient<IApplicationsRulesFactory, ApplicationsRulesFactory>()
                .AddTransient<IWallpaperLibraryFactory, WallpaperLibraryFactory>()
                .AddTransient<IThumbnailService, LinuxThumbnailService>()
                .AddTransient<IAppThemeFactory, AppThemeFactory>()
                .AddTransient<IDownloadService, HttpDownloadService>()
                .AddTransient<IMediaFormatConverter, MediaFormatConverter>()
                .AddTransient<IAudioDeviceFactory, LinuxAudioDeviceFactory>()
                .AddTransient<IPlatformUiFeatures, LinuxPlatformUiFeatures>()
                //https://docs.microsoft.com/en-us/dotnet/architecture/microservices/implement-resilient-applications/use-httpclientfactory-to-implement-resilient-http-requests
                .AddHttpClient()
                .BuildServiceProvider(new ServiceProviderOptions { ValidateOnBuild = true, ValidateScopes = true });
        }

        /// <summary>
        /// Applies the theme from the settings; Auto follows the desktop (ThemeVariant.Default).
        /// </summary>
        public static void SetAppTheme(AppTheme theme)
        {
            Current.RequestedThemeVariant = theme switch
            {
                AppTheme.Light => ThemeVariant.Light,
                AppTheme.Dark => ThemeVariant.Dark,
                _ => ThemeVariant.Default,
            };
        }

        private void HandleCommand(string line)
        {
            var parts = line.Trim().Split(' ', 3, StringSplitOptions.RemoveEmptyEntries);
            if (parts.Length < 2)
            {
                Logger.Warn($"Ignoring malformed command: {line}");
                return;
            }

            var payload = parts.Length > 2 ? parts[2] : string.Empty;
            switch (parts[0].ToUpperInvariant(), parts[1].ToUpperInvariant())
            {
                case ("WM", "SHOW"):
                    ShowMainWindow();
                    break;
                case ("WM", "HIDE"):
                    mainWindow.Hide();
                    break;
                case ("WM", "QUIT"):
                    Shutdown();
                    break;
                case ("LM", "SHOWBUSY"):
                    SetBusy(true);
                    break;
                case ("LM", "HIDEBUSY"):
                    SetBusy(false);
                    break;
                case ("LM", "SHOWCUSTOMISEPANEL"):
                    ShowMainWindow();
                    if (services != null)
                        _ = services.GetRequiredService<IDialogService>().ShowControlPanelDialogAsync();
                    else
                        Logger.Warn("SHOWCUSTOMISEPANEL received while the wallpaper core is unavailable.");
                    break;
                case ("LM", "SHOWAPPUPDATEPAGE"):
                    ShowMainWindow();
                    if (services != null)
                        services.GetRequiredService<IMainNavigator>().NavigateTo(ContentPageType.appupdate);
                    else
                        Logger.Warn("SHOWAPPUPDATEPAGE received while the wallpaper core is unavailable.");
                    break;
                case ("LM", "WALLPAPERDATA"):
                    _ = ShowWallpaperDataDialogAsync(payload);
                    break;
                default:
                    Logger.Warn($"Unknown command: {line}");
                    break;
            }
        }

        private void SetBusy(bool isBusy)
        {
            isBusyRequested = isBusy;
            if (services != null)
                services.GetRequiredService<LibraryViewModel>().IsBusy = isBusy;
        }

        /// <summary>
        /// Shows the wallpaper metadata dialog requested by the core and answers on stdout with a single
        /// <c>LM WALLPAPERDATA {json}</c> line. The core owns the wallpaper folder, so the dialog only collects text.
        /// </summary>
        private async Task ShowWallpaperDataDialogAsync(string json)
        {
            WallpaperDataRequest request;
            try
            {
                request = JsonConvert.DeserializeObject<WallpaperDataRequest>(json)
                    ?? throw new JsonException("The request payload is empty.");
            }
            catch (JsonException ex)
            {
                Logger.Error($"Invalid wallpaper data request '{json}': {ex.Message}");
                AnswerWallpaperData(new WallpaperDataResult { Ok = false });
                return;
            }

            ShowMainWindow();

            var model = new LibraryModel
            {
                LivelyInfoFolderPath = request.InfoPath,
                ImagePath = string.IsNullOrEmpty(request.Thumbnail) ? null : request.Thumbnail,
                ThumbnailPath = string.IsNullOrEmpty(request.Thumbnail) ? null : request.Thumbnail,
                LivelyInfo = new LivelyInfoModel
                {
                    Title = request.Title,
                    Author = request.Author,
                    Desc = request.Desc,
                    Contact = request.Contact,
                },
            };
            model.Title = request.Title;
            model.Author = request.Author;
            model.Desc = request.Desc;

            // The core deletes or keeps the folder itself, so the view model gets the library only when the core is connected.
            var viewModel = new AddWallpaperDataViewModel(services?.GetRequiredService<LibraryViewModel>()) { Model = model };
            var dialog = new ContentDialog
            {
                Title = resourceService.GetString("AddWallpaper/Label"),
                DialogContent = new AddWallpaperDataView(viewModel),
                PrimaryButtonText = resourceService.GetString("TextOK"),
                SecondaryButtonText = resourceService.GetString("Cancel/Content"),
                DefaultButton = ContentDialogButton.Primary,
            };
            var result = await dialog.ShowAsync();

            AnswerWallpaperData(result == ContentDialogResult.Primary
                ? new WallpaperDataResult
                {
                    Ok = true,
                    Title = viewModel.Title ?? string.Empty,
                    Author = viewModel.Author ?? string.Empty,
                    Desc = viewModel.Desc ?? string.Empty,
                    Contact = viewModel.Url ?? string.Empty,
                }
                : new WallpaperDataResult { Ok = false });
        }

        private void AnswerWallpaperData(WallpaperDataResult result)
        {
            stdio.WriteLine($"{WallpaperDataPrefix} {JsonConvert.SerializeObject(result)}");
        }

        /// <summary>
        /// --trayWidget: customise panel of the running wallpaper on the primary display, without the main window.
        /// </summary>
        private void ShowTrayWidget()
        {
            var desktopCore = services.GetRequiredService<IDesktopCoreClient>();
            var items = desktopCore.Wallpapers.Where(x => x.LivelyPropertyCopyPath != null).ToList();
            var selection = items.FirstOrDefault(x => x.Display.IsPrimary) ?? items.FirstOrDefault();
            var libraryVm = services.GetRequiredService<LibraryViewModel>();
            var model = selection is null ? null : libraryVm.LibraryItems.FirstOrDefault(x => selection.LivelyInfoFolderPath == x.LivelyInfoFolderPath);
            if (model is null)
            {
                Logger.Info("No running wallpaper with customisable properties; nothing to show for --trayWidget.");
                if (!mainWindow.IsVisible)
                    Shutdown();
                return;
            }

            var viewModel = services.GetRequiredService<CustomiseWallpaperViewModel>();
            trayWindow = new LivelyPropertiesTrayWindow(viewModel) { Title = model.Title };
            trayWindow.Closed += (_, _) =>
            {
                viewModel.OnClose();
                trayWindow = null;
                if (!mainWindow.IsVisible)
                    Shutdown();
            };
            viewModel.Load(model);
            trayWindow.Show();
        }

        private void ShowMainWindow()
        {
            mainWindow.Show();
            mainWindow.Activate();
        }

        private void ReportVisibility(bool isVisible)
        {
            stdio.WriteLine($"LM UIVISIBLE {(isVisible ? "true" : "false")}");
        }

        private void OnInputClosed()
        {
            if (StartFlags.CoreManaged)
            {
                Logger.Info("The core closed the command pipe, exiting.");
                Shutdown();
            }
            else
            {
                Logger.Info("stdin closed; further commands arrive through the single-instance socket only.");
            }
        }

        public static void Shutdown()
        {
            var app = (App)Current;
            app.Cleanup();
            (app.ApplicationLifetime as IClassicDesktopStyleApplicationLifetime)?.Shutdown();
        }

        private void Cleanup()
        {
            if (isCleanedUp)
                return;
            isCleanedUp = true;

            try
            {
                services?.Dispose();
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
            }
            services = null;
            accentColors?.Dispose();
            SingleInstance?.Dispose();
        }

        private void SetupUnhandledExceptionLogging()
        {
            AppDomain.CurrentDomain.UnhandledException += (_, e) => Logger.Error(e.ExceptionObject as Exception);
            TaskScheduler.UnobservedTaskException += (_, e) => Logger.Error(e.Exception);
            Dispatcher.UIThread.UnhandledException += (_, e) => Logger.Error(e.Exception);
        }
    }
}
