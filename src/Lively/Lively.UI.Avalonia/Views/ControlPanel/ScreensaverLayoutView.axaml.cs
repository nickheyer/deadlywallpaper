using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.ControlPanel
{
    public partial class ScreensaverLayoutView : UserControl
    {
        public ScreensaverLayoutView(ControlPanelViewModel controlPanelViewModel)
        {
            InitializeComponent();
            DataContext = controlPanelViewModel.ScreensaverVm;
        }
    }
}
