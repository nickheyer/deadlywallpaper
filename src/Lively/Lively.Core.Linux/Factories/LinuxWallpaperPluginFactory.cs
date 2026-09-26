using Lively.Core.Linux.Backends;
using Lively.Factories;
using Lively.Models;
using Lively.Models.Enums;

namespace Lively.Core.Linux.Factories
{
    /// <summary>
    /// Hands wallpaper creation to the active compositor backend.
    /// </summary>
    public sealed class LinuxWallpaperPluginFactory : IWallpaperPluginFactory
    {
        private readonly IWallpaperBackend backend;

        public LinuxWallpaperPluginFactory(IWallpaperBackend backend)
        {
            this.backend = backend;
        }

        public IWallpaper CreateWallpaper(LibraryModel model, DisplayMonitor display, WallpaperArrangement arrangement, bool isWindowed = false)
        {
            return backend.CreateWallpaper(model, display, arrangement, isWindowed);
        }
    }
}
