using Lively.Models;
using Lively.Models.Enums;
using System;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Backends
{
    /// <summary>
    /// How wallpapers get onto the desktop on this compositor: native layer-shell hosts (wlroots
    /// compositors) or the Plasma wallpaper plugin (KDE).
    /// </summary>
    public interface IWallpaperBackend : IDisposable
    {
        /// <summary>Short identifier used in logs and the CLI (layer-shell, plasma).</summary>
        string Name { get; }

        /// <summary>Connects to the compositor/desktop. Throws with a precise message when the backend cannot work here.</summary>
        Task InitializeAsync();

        /// <summary>Creates a wallpaper instance for a display. Windowed instances render in a normal window for previews.</summary>
        IWallpaper CreateWallpaper(LibraryModel model, DisplayMonitor display, WallpaperArrangement arrangement, bool isWindowed);

        /// <summary>Called when displays changed so the backend can refresh what it knows about them.</summary>
        Task OnDisplaysChangedAsync();

        /// <summary>Restores whatever the desktop showed before Lively took over (Plasma: the previous wallpaper plugin).</summary>
        Task RestoreDesktopAsync();
    }
}
