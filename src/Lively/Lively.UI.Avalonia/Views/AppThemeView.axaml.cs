using Avalonia.Controls;
using Lively.Models.Enums;
using Lively.UI.Shared.ViewModels;
using Microsoft.Extensions.DependencyInjection;
using System.ComponentModel;

namespace Lively.UI.Avalonia.Views
{
    public partial class AppThemeView : UserControl
    {
        private readonly AppThemeViewModel viewModel;

        public AppThemeView()
        {
            InitializeComponent();
            viewModel = App.Services.GetRequiredService<AppThemeViewModel>();
            DataContext = viewModel;
            viewModel.PropertyChanged += ViewModel_PropertyChanged;
        }

        private void ViewModel_PropertyChanged(object sender, PropertyChangedEventArgs e)
        {
            // Avalonia switches theme variants at runtime, so the choice applies immediately.
            if (e.PropertyName == nameof(AppThemeViewModel.SelectedAppThemeIndex))
                App.SetAppTheme((AppTheme)viewModel.SelectedAppThemeIndex);
        }
    }
}
