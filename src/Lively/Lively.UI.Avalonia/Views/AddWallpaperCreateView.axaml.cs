using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class AddWallpaperCreateView : UserControl
    {
        public AddWallpaperCreateView(AddWallpaperCreateViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
