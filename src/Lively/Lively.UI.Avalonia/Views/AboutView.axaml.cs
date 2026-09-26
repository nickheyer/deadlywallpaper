using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;
using Microsoft.Extensions.DependencyInjection;

namespace Lively.UI.Avalonia.Views
{
    public partial class AboutView : UserControl
    {
        public AboutView()
        {
            InitializeComponent();
            DataContext = App.Services.GetRequiredService<AboutViewModel>();
        }
    }
}
