using Lively.Core;
using Lively.Models;
using Lively.Models.Enums;

namespace Lively.Factories
{
    public interface IWallpaperPluginFactory
    {
        IWallpaper CreateWallpaper(LibraryModel model,
            DisplayMonitor display,
            WallpaperArrangement arrangement,
            bool isWindowed = false);
    }
}
