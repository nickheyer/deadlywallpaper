using Avalonia.Controls;
using Lively.UI.Avalonia.Services;
using Lively.UI.Shared.ViewModels;
using System;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Views
{
    /// <summary>
    /// Update page. The changelog web page is shown as text; there is no web view on Linux.
    /// </summary>
    public partial class AppUpdateView : UserControl, INavigationAware
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly AppUpdateViewModel viewModel;
        private readonly WebPageTextFetcher fetcher;
        private readonly CancellationTokenSource fetchCts = new CancellationTokenSource();

        public AppUpdateView(AppUpdateViewModel viewModel, WebPageTextFetcher fetcher)
        {
            this.viewModel = viewModel;
            this.fetcher = fetcher;
            InitializeComponent();
            DataContext = viewModel;

            _ = LoadChangelogAsync();
        }

        private async Task LoadChangelogAsync()
        {
            if (!viewModel.IsWebView2Available)
            {
                ChangelogProgress.IsVisible = false;
                return;
            }

            var url = viewModel.IsBetaBuild ?
                "https://www.rocksdanister.com/lively-webpage/changelog/?source=app" :
                "https://www.rocksdanister.com/lively/changelog/?source=app";
            try
            {
                viewModel.UpdateChangelogError = null;
                ChangelogText.Text = await fetcher.FetchTextAsync(new Uri(url), fetchCts.Token);
            }
            catch (OperationCanceledException)
            {
                // Navigated away while loading.
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
                viewModel.UpdateChangelogError = $"Exception: {ex.GetType().Name}\nMessage: {ex.Message}";
            }
            finally
            {
                ChangelogProgress.IsVisible = false;
            }
        }

        public void OnNavigatedTo(object navArgs)
        {
        }

        public void OnNavigatedFrom()
        {
            fetchCts.Cancel();
        }
    }
}
