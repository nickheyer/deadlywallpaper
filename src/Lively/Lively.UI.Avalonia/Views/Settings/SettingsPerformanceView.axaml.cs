using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Settings
{
    public partial class SettingsPerformanceView : UserControl
    {
        public SettingsPerformanceView(SettingsPerformanceViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
