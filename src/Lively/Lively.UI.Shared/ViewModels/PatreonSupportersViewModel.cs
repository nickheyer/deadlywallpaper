using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Lively.Common;
using Lively.Grpc.Client;
using Lively.UI.Shared.Services;
using System.Threading.Tasks;

namespace Lively.UI.Shared.ViewModels
{
    public partial class PatreonSupportersViewModel : ObservableObject
    {
        private readonly ICommandsClient commandsClient;
        private readonly IPlatformUiFeatures platform;

        public PatreonSupportersViewModel(ICommandsClient commandsClient, IPlatformUiFeatures platform)
        {
            this.commandsClient = commandsClient;
            this.platform = platform;
        }

        public bool IsBetaBuild => Constants.ApplicationType.IsTestBuild;

        public bool IsWebView2Available => platform.IsWebViewRuntimeAvailable;

        /// <summary>
        /// Supporters page shown in the dialog, the theme parameters are appended by the view.
        /// </summary>
        public string SupportersPageUrl => IsBetaBuild ?
            "https://www.rocksdanister.com/lively-webpage/supporters/" :
            "https://www.rocksdanister.com/lively/supporters/";

        [ObservableProperty]
        private string supportersFetchError;

        [ObservableProperty]
        private bool isWebView2Installing;

        [RelayCommand]
        private async Task InstallWebView2()
        {
            try
            {
                IsWebView2Installing = true;

                if (await platform.TryInstallWebViewRuntimeAsync())
                    _ = commandsClient.RestartUI();
                else
                    LinkUtil.OpenBrowser(platform.WebViewRuntimeDownloadUrl);
            }
            finally
            {
                IsWebView2Installing = false;
            }
        }
    }
}
