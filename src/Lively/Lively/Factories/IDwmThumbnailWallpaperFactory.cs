using Lively.Core;
using Lively.Models;
using System;
using System.Drawing;

namespace Lively.Factories
{
    public interface IDwmThumbnailWallpaperFactory
    {
        IWallpaper CreateDwmThumbnailWallpaper(
            LibraryModel model,
            IntPtr thumbnailSrc,
            Rectangle targetRect,
            DisplayMonitor display);
    }
}
