using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Settings
{
    public partial class SettingsScreensaverView : UserControl
    {
        public SettingsScreensaverView(SettingsScreensaverViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
