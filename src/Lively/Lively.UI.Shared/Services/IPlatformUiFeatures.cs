using Lively.Models;
using Lively.Models.Enums;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace Lively.UI.Shared.Services
{
    /// <summary>
    /// Platform capabilities consumed by the shared view models. Each capability that a platform
    /// cannot provide is reported through a Supports* property so the view can hide the related control.
    /// </summary>
    public interface IPlatformUiFeatures
    {
        /// <summary>
        /// True when the UI process runs with administrative privileges (Windows UAC elevation, Linux root).
        /// </summary>
        bool IsElevated { get; }

        /// <summary>
        /// True when the application is installed through a store package (MSIX); updates are then handled by the store.
        /// </summary>
        bool IsPackaged { get; }

        /// <summary>
        /// File extensions offered by the application picker when browsing for an executable.
        /// </summary>
        string[] ApplicationFileExtensions { get; }

        /// <summary>
        /// Enumerates the applications currently running for the user that can be selected as a pause/exclusion rule.
        /// </summary>
        IEnumerable<ApplicationModel> GetRunningApplications();

        /// <summary>
        /// True when the desktop icons can be hidden and shown by the wallpaper input forwarding setting.
        /// </summary>
        bool SupportsDesktopIconToggle { get; }

        void SetDesktopIconVisibility(bool isVisible);

        /// <summary>
        /// True when the platform can run Lively as the system screensaver.
        /// </summary>
        bool SupportsScreensaver { get; }

        /// <summary>
        /// True when Lively is the screensaver currently selected in the system settings.
        /// </summary>
        bool IsScreensaverRegistered();

        /// <summary>
        /// Opens the system screensaver settings page.
        /// </summary>
        void OpenSystemScreensaverSettings();

        /// <summary>
        /// True when the platform exposes a system page for the accent colour used by the app theme.
        /// </summary>
        bool SupportsSystemColorSettings { get; }

        void OpenSystemColorSettings();

        /// <summary>
        /// True when more than one player per media kind is shipped and the settings page offers a picker for it.
        /// </summary>
        bool SupportsPlayerSelection { get; }

        /// <summary>
        /// True when the given video player plugin is installed next to the core.
        /// </summary>
        bool IsPlayerAvailable(LivelyMediaPlayer player);

        /// <summary>
        /// True when the given gif player plugin is installed next to the core.
        /// </summary>
        bool IsGifPlayerAvailable(LivelyGifPlayer player);

        /// <summary>
        /// True when the given web wallpaper host plugin is installed next to the core.
        /// </summary>
        bool IsWebBrowserAvailable(LivelyWebBrowser browser);

        /// <summary>
        /// True when the stream downloader used by video stream wallpapers is installed.
        /// </summary>
        bool IsStreamDownloaderAvailable { get; }

        /// <summary>
        /// True when the platform lets the core restyle the system taskbar.
        /// </summary>
        bool SupportsTaskbarTheme { get; }

        /// <summary>
        /// True when the core can tell that the session is viewed through a remote desktop connection,
        /// so the "pause while remote" rule has an effect.
        /// </summary>
        bool SupportsRemoteDesktopPause { get; }

        /// <summary>
        /// True when the web wallpaper runtime can be downloaded and installed by the UI.
        /// </summary>
        bool SupportsWebViewRuntimeInstall { get; }

        /// <summary>
        /// True when the runtime used by web wallpapers is present on the machine.
        /// </summary>
        bool IsWebViewRuntimeAvailable { get; }

        /// <summary>
        /// Web page that offers the runtime download when the automatic install is unavailable.
        /// </summary>
        string WebViewRuntimeDownloadUrl { get; }

        /// <summary>
        /// Downloads and installs the web wallpaper runtime. Returns false when the install could not be performed.
        /// </summary>
        Task<bool> TryInstallWebViewRuntimeAsync();
    }
}
