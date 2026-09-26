using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class ChooseDisplayView : UserControl
    {
        public ChooseDisplayView(ChooseDisplayViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
