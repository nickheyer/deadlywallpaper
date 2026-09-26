using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.ControlPanel
{
    public partial class WallpaperLayoutView : UserControl
    {
        public WallpaperLayoutView(ControlPanelViewModel controlPanelViewModel)
        {
            InitializeComponent();
            DataContext = controlPanelViewModel.WallpaperVm;
        }
    }
}
