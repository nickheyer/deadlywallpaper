using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class DepthEstimateWallpaperView : UserControl
    {
        public DepthEstimateWallpaperView(DepthEstimateWallpaperViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
