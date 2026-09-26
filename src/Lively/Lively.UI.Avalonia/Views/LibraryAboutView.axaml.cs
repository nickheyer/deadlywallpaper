using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views
{
    public partial class LibraryAboutView : UserControl
    {
        internal LibraryAboutView(LibraryAboutViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
