using Avalonia.Controls;
using Lively.UI.Avalonia.Services;
using Lively.UI.Avalonia.Views.LivelyProperty;
using Lively.UI.Shared.ViewModels;
using System;

namespace Lively.UI.Avalonia.Views.ControlPanel
{
    public partial class WallpaperLayoutCustomiseView : UserControl, INavigationAware
    {
        public WallpaperLayoutCustomiseView()
        {
            InitializeComponent();
        }

        public void OnNavigatedTo(object navArgs)
        {
            if (navArgs is not CustomiseWallpaperViewModel viewModel)
                throw new ArgumentException($"{nameof(WallpaperLayoutCustomiseView)} expects a {nameof(CustomiseWallpaperViewModel)} as navigation argument.", nameof(navArgs));

            ContentFrame.Content = new LivelyPropertiesView(viewModel);
        }

        public void OnNavigatedFrom()
        {
        }
    }
}
