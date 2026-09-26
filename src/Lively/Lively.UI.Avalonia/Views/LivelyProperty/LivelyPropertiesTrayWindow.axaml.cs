using Avalonia.Controls;
using Avalonia.Platform;
using Lively.UI.Shared.ViewModels;
using System;

namespace Lively.UI.Avalonia.Views.LivelyProperty
{
    /// <summary>
    /// Stand-alone customise window opened with --trayWidget (tray menu "Customise wallpaper").
    /// </summary>
    public partial class LivelyPropertiesTrayWindow : Window
    {
        public LivelyPropertiesTrayWindow(CustomiseWallpaperViewModel viewModel)
        {
            InitializeComponent();
            Icon = new WindowIcon(AssetLoader.Open(new Uri("avares://Lively.UI.Avalonia/Assets/icon-lively-48.png")));
            ContentFrame.Content = new LivelyPropertiesView(viewModel);
        }
    }
}
