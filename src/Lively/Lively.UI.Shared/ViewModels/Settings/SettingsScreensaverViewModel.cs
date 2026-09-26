using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Lively.Common.Services;
using Lively.Grpc.Client;
using Lively.Models;
using Lively.UI.Shared.Services;

namespace Lively.UI.Shared.ViewModels
{
    public partial class SettingsScreensaverViewModel : ObservableObject
    {
        private readonly IUserSettingsClient userSettings;
        private readonly IDispatcherService dispatcher;
        private readonly IPlatformUiFeatures platform;

        public SettingsScreensaverViewModel(IUserSettingsClient userSettings, IDispatcherService dispatcher, IPlatformUiFeatures platform)
        {
            this.userSettings = userSettings;
            this.dispatcher = dispatcher;
            this.platform = platform;

            IsFadeIn = userSettings.Settings.ScreensaverFadeIn;
            IsLockOnResume = userSettings.Settings.ScreensaverLockOnResume;
            Volume = userSettings.Settings.ScreensaverGlobalVolume;
            IsScreensaverSupported = platform.SupportsScreensaver;
            IsScreensaverPluginNotify = platform.SupportsScreensaver && !platform.IsScreensaverRegistered();
        }

        /// <summary>
        /// False when the platform cannot run Lively as the system screensaver; the system settings card is hidden.
        /// </summary>
        public bool IsScreensaverSupported { get; }

        [ObservableProperty]
        private bool isScreensaverPluginNotify;

        private bool _isLockOnResume;
        public bool IsLockOnResume
        {
            get => _isLockOnResume;
            set
            {
                if (userSettings.Settings.ScreensaverLockOnResume != value)
                {
                    userSettings.Settings.ScreensaverLockOnResume = value;
                    UpdateSettingsConfigFile();
                }
                SetProperty(ref _isLockOnResume, value);
            }
        }

        private bool _isFadeIn;
        public bool IsFadeIn
        {
            get => _isFadeIn;
            set
            {
                if (userSettings.Settings.ScreensaverFadeIn != value)
                {
                    userSettings.Settings.ScreensaverFadeIn = value;
                    UpdateSettingsConfigFile();
                }
                SetProperty(ref _isFadeIn, value);
            }
        }

        private int _volume;
        public int Volume
        {
            get => _volume;
            set
            {
                if (userSettings.Settings.ScreensaverGlobalVolume != value)
                {
                    userSettings.Settings.ScreensaverGlobalVolume = value;
                    UpdateSettingsConfigFile();
                }
                SetProperty(ref _volume, value);
            }
        }

        [RelayCommand]
        private void OpenWindowsSettings()
        {
            if (!platform.SupportsScreensaver)
                return;

            platform.OpenSystemScreensaverSettings();
        }

        private void UpdateSettingsConfigFile()
        {
            dispatcher.TryEnqueue(userSettings.Save<SettingsModel>);
        }
    }
}
