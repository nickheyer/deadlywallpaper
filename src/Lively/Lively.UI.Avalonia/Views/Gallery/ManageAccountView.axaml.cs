using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;
using Microsoft.Extensions.DependencyInjection;

namespace Lively.UI.Avalonia.Views.Gallery
{
    public partial class ManageAccountView : UserControl
    {
        public ManageAccountView() : this(App.Services.GetRequiredService<ManageAccountViewModel>())
        {
        }

        public ManageAccountView(ManageAccountViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
