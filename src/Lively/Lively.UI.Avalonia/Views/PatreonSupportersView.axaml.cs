using Avalonia.Controls;
using Lively.UI.Avalonia.Services;
using Lively.UI.Shared.ViewModels;
using Microsoft.Extensions.DependencyInjection;
using System;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Views
{
    /// <summary>
    /// Supporters page shown as text; there is no web view on Linux.
    /// </summary>
    public partial class PatreonSupportersView : UserControl
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly PatreonSupportersViewModel viewModel;
        private readonly CancellationTokenSource fetchCts = new CancellationTokenSource();

        public PatreonSupportersView()
        {
            InitializeComponent();
            viewModel = App.Services.GetRequiredService<PatreonSupportersViewModel>();
            DataContext = viewModel;

            _ = LoadSupportersAsync();
        }

        private async Task LoadSupportersAsync()
        {
            if (!viewModel.IsWebView2Available)
            {
                SupportersProgress.IsVisible = false;
                return;
            }

            try
            {
                var fetcher = App.Services.GetRequiredService<WebPageTextFetcher>();
                SupportersText.Text = await fetcher.FetchTextAsync(new Uri(viewModel.SupportersPageUrl), fetchCts.Token);
            }
            catch (OperationCanceledException)
            {
                // Dialog closed while loading.
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
                viewModel.SupportersFetchError = $"Exception: {ex.GetType().Name}\nMessage: {ex.Message}";
            }
            finally
            {
                SupportersProgress.IsVisible = false;
            }
        }

        public void OnClose()
        {
            fetchCts.Cancel();
        }
    }
}
