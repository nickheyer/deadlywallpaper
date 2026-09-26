using Avalonia.Controls;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.Gallery
{
    /// <summary>
    /// Gallery collection with incremental loading: scrolling near the bottom requests the next page.
    /// </summary>
    public partial class GalleryLibraryView : UserControl
    {
        private const double LoadMoreThreshold = 200;
        private readonly GalleryViewModel viewModel;

        public GalleryLibraryView(GalleryViewModel viewModel)
        {
            this.viewModel = viewModel;
            InitializeComponent();
            DataContext = viewModel;
        }

        private async void TileScroller_ScrollChanged(object sender, ScrollChangedEventArgs e)
        {
            var scrollableHeight = TileScroller.Extent.Height - TileScroller.Viewport.Height;
            var isAtBottom = scrollableHeight <= 0 || TileScroller.Offset.Y >= scrollableHeight - 1;

            var wallpapers = viewModel.Wallpapers;
            if (wallpapers != null && wallpapers.HasMoreItems && !wallpapers.IsLoading && TileScroller.Offset.Y >= scrollableHeight - LoadMoreThreshold)
                await wallpapers.LoadMoreItemsAsync(wallpapers.ItemsPerPage);

            MoreMessage.IsVisible = isAtBottom && wallpapers != null && !wallpapers.HasMoreItems;
        }
    }
}
