using Avalonia;
using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;
using System.ComponentModel;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Views.Settings
{
    public partial class SettingsWallpaperView : UserControl
    {
        private readonly SettingsWallpaperViewModel viewModel;

        public SettingsWallpaperView(SettingsWallpaperViewModel viewModel)
        {
            this.viewModel = viewModel;
            InitializeComponent();
            DataContext = viewModel;

            UpdatePluginNotifications();
            viewModel.PropertyChanged += ViewModel_PropertyChanged;
        }

        private async void ViewModel_PropertyChanged(object sender, PropertyChangedEventArgs e)
        {
            if (e.PropertyName == nameof(SettingsWallpaperViewModel.IsSelectedVideoPlayerAvailable)
                || e.PropertyName == nameof(SettingsWallpaperViewModel.IsSelectedWebBrowserAvailable))
            {
                UpdatePluginNotifications();
            }
            else if (e.PropertyName == nameof(SettingsWallpaperViewModel.IsMusicSettingsChanged) && viewModel.IsMusicSettingsChanged)
            {
                // Let the layout pass place the info bar before scrolling to it.
                await Task.Delay(100);
                ScrollElementIntoView(MusicWallpaperRestartNotify);
            }
        }

        /// <summary>
        /// The "plugin not found" bars only make sense where a plugin can be picked; a platform with a single
        /// player per media kind has nothing to switch to.
        /// </summary>
        private void UpdatePluginNotifications()
        {
            VideoPluginNotFound.IsVisible = viewModel.IsPlayerSelectionSupported && !viewModel.IsSelectedVideoPlayerAvailable;
            WebPluginNotFound.IsVisible = viewModel.IsPlayerSelectionSupported && !viewModel.IsSelectedWebBrowserAvailable;
        }

        private void ScrollElementIntoView(Control element)
        {
            var content = PageScrollViewer.Content as Visual;
            var position = element.TranslatePoint(new Point(0, 0), content);
            if (position is null)
                return;

            PageScrollViewer.Offset = new Vector(0, position.Value.Y);
        }
    }
}
