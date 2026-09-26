using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Gallery
{
    public partial class RestoreWallpaperView : UserControl
    {
        public RestoreWallpaperView(RestoreWallpaperViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
