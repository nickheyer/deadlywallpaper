using Avalonia.Controls;
using Lively.UI.Avalonia.Services;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Settings
{
    public partial class SettingsGeneralView : UserControl, INavigationAware
    {
        private readonly SettingsGeneralViewModel viewModel;

        public SettingsGeneralView(SettingsGeneralViewModel viewModel)
        {
            this.viewModel = viewModel;
            InitializeComponent();
            DataContext = viewModel;
        }

        public void OnNavigatedTo(object navArgs)
        {
        }

        public void OnNavigatedFrom()
        {
            viewModel.OnClose();
        }
    }
}
