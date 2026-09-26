using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Settings
{
    public partial class FindMoreAppsView : UserControl
    {
        public FindMoreAppsView(FindMoreAppsViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
