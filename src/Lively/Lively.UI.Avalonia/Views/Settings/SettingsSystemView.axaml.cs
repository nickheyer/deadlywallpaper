using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Settings
{
    public partial class SettingsSystemView : UserControl
    {
        public SettingsSystemView(SettingsSystemViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
