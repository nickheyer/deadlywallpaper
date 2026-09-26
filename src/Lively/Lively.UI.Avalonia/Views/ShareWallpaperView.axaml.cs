using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class ShareWallpaperView : UserControl
    {
        public ShareWallpaperView(ShareWallpaperViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
