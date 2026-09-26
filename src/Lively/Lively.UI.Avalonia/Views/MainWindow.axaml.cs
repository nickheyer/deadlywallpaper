using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Platform;
using Avalonia.Threading;
using Lively.Common.Services;
using Lively.Grpc.Client;
using Lively.Models;
using Lively.Models.Enums;
using Lively.UI.Avalonia.Controls;
using Lively.UI.Avalonia.Localization;
using Lively.UI.Shared.ViewModels;
using Microsoft.Extensions.DependencyInjection;
using System;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Views
{
    public partial class MainWindow : Window
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private IDesktopCoreClient desktopCore;
        private IUserSettingsClient userSettings;
        private LibraryViewModel libraryVm;
        private AppUpdateViewModel appUpdateVm;
        private IDialogService dialogService;
        private ICommandsClient commands;
        private IMainNavigator navigator;
        private IResourceService i18n;
        private bool isCloseConfirmed;

        public MainWindow()
        {
            InitializeComponent();
            Icon = new WindowIcon(AssetLoader.Open(new Uri("avares://Lively.UI.Avalonia/Assets/icon-lively-48.png")));
            Closing += MainWindow_Closing;
        }

        /// <summary>
        /// Raised when the "core unavailable" retry button is pressed.
        /// </summary>
        public event EventHandler RetryRequested;

        /// <summary>
        /// Raised with the new visibility whenever the window is shown or hidden.
        /// </summary>
        public event EventHandler<bool> VisibilityChanged;

        public DialogHost DialogHost => DialogHostControl;

        public bool IsCoreConnected => navigator != null;

        public void ShowCoreConnecting()
        {
            CoreStatePanel.IsVisible = true;
            CoreStateIcon.IsVisible = false;
            CoreStateTitle.Text = LocalizationSource.GetString("CoreConnecting/Text");
            CoreStateMessage.IsVisible = false;
            CoreStateDetails.Text = string.Empty;
            CoreStateProgress.IsVisible = true;
            RetryButton.IsEnabled = false;
        }

        public void ShowCoreUnavailable(string details)
        {
            CoreStatePanel.IsVisible = true;
            CoreStateIcon.IsVisible = true;
            CoreStateTitle.Text = LocalizationSource.GetString("CoreUnavailable/Title");
            CoreStateMessage.Text = LocalizationSource.GetString("CoreUnavailable/Message");
            CoreStateMessage.IsVisible = true;
            CoreStateDetails.Text = details;
            CoreStateProgress.IsVisible = false;
            RetryButton.IsEnabled = true;
        }

        /// <summary>
        /// Attaches the view models once the core is connected and shows the library.
        /// </summary>
        public void Initialize(IServiceProvider services)
        {
            desktopCore = services.GetRequiredService<IDesktopCoreClient>();
            userSettings = services.GetRequiredService<IUserSettingsClient>();
            libraryVm = services.GetRequiredService<LibraryViewModel>();
            appUpdateVm = services.GetRequiredService<AppUpdateViewModel>();
            dialogService = services.GetRequiredService<IDialogService>();
            commands = services.GetRequiredService<ICommandsClient>();
            i18n = services.GetRequiredService<IResourceService>();
            var mainVm = services.GetRequiredService<MainViewModel>();

            SearchBox.DataContext = libraryVm;
            SelectionBar.DataContext = libraryVm;
            DataContext = mainVm;

            navigator = services.GetRequiredService<IMainNavigator>();
            navigator.RootFrame = this;
            navigator.Frame = ContentFrame;

            desktopCore.WallpaperChanged += DesktopCore_WallpaperChanged;

            CoreStatePanel.IsVisible = false;
            ContentRoot.IsVisible = true;
            // Open the library once the frame exists, like the WinUI Root.Loaded handler.
            mainVm.OpenHomeCommand.Execute(null);
        }

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);
            if (change.Property == IsVisibleProperty)
                VisibilityChanged?.Invoke(this, IsVisible);
        }

        private void RetryButton_Click(object sender, RoutedEventArgs e)
        {
            RetryRequested?.Invoke(this, EventArgs.Empty);
        }

        private void DesktopCore_WallpaperChanged(object sender, EventArgs e)
        {
            Dispatcher.UIThread.Post(() =>
            {
                // In duplicate mode the event fires once per display; animate once.
                if (userSettings.Settings.WallpaperArrangement != WallpaperArrangement.duplicate || desktopCore.Wallpapers.Count < 2)
                    _ = PulseControlPanelIconAsync();
            });
        }

        private async Task PulseControlPanelIconAsync()
        {
            var transform = new TranslateTransform();
            ControlPanelIcon.RenderTransform = transform;
            for (var i = 0; i < 2; i++)
            {
                transform.Y = -5;
                await Task.Delay(150);
                transform.Y = 0;
                await Task.Delay(150);
            }
        }

        private async void MainWindow_Closing(object sender, WindowClosingEventArgs e)
        {
            if (isCloseConfirmed)
                return;

            // Every path below is asynchronous or hides the window, so the close is always cancelled here first.
            e.Cancel = true;
            try
            {
                if (!IsCoreConnected)
                {
                    await Task.Yield();
                    await CloseOrHideAsync();
                }
                else if (userSettings.Settings.IsFirstRun)
                {
                    await dialogService.ShowWaitDialogAsync(new TrayMenuHelpView(), 4);
                    userSettings.Settings.IsFirstRun = false;
                    userSettings.Save<SettingsModel>();
                    await CloseOrHideAsync();
                }
                else if (userSettings.Settings.IsUpdatedNotify)
                {
                    userSettings.Settings.IsUpdatedNotify = false;
                    userSettings.Save<SettingsModel>();
                    await CloseOrHideAsync();
                }
                else if (libraryVm.IsWorking || appUpdateVm.IsUpdateDownloading)
                {
                    var result = await dialogService.ShowDialogAsync(i18n.GetString("TextConfirmCancel/Text"),
                                                                i18n.GetString("TitleDownloadProgress/Text"),
                                                                i18n.GetString("TextYes"),
                                                                i18n.GetString("TextWait/Text"),
                                                                false);
                    if (result == DialogResult.primary)
                    {
                        appUpdateVm.CancelCommand.Execute(null);
                        libraryVm.CancelAllDownloads();
                        libraryVm.IsBusy = true;

                        await Task.Delay(1500);
                        await CloseOrHideAsync();
                    }
                }
                else if (dialogService.IsWorking)
                {
                    // A customise dialog is open; the user closes it and tries again.
                }
                else
                {
                    await CloseOrHideAsync();
                }
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
            }
        }

        /// <summary>
        /// Core-managed processes stay alive in the background (the core re-shows them with WM SHOW);
        /// otherwise the window position is saved and the application exits.
        /// </summary>
        private async Task CloseOrHideAsync()
        {
            if (App.StartFlags.CoreManaged)
            {
                Hide();
                return;
            }

            if (IsCoreConnected)
                await commands.SaveRectUIAsync();

            isCloseConfirmed = true;
            Close();
            App.Shutdown();
        }
    }
}
