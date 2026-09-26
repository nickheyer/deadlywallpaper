using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class ReportWallpaperView : UserControl
    {
        public ReportWallpaperView(ReportWallpaperViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
