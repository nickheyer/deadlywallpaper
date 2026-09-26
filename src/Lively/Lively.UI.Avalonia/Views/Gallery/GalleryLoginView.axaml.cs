using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Gallery
{
    public partial class GalleryLoginView : UserControl
    {
        public GalleryLoginView(GalleryLoginViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
