using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class AddWallpaperDataView : UserControl
    {
        public AddWallpaperDataView(AddWallpaperDataViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
