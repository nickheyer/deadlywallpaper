using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Gallery
{
    public partial class GallerySubscriptionView : UserControl
    {
        public GallerySubscriptionView(GallerySubscriptionViewModel viewModel)
        {
            InitializeComponent();
            DataContext = viewModel;
        }
    }
}
