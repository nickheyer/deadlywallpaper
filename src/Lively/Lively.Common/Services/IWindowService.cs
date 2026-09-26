using Lively.Models;
using System.Threading.Tasks;

namespace Lively.Common.Services
{
    public interface IWindowService
    {
        bool IsGridOverlayVisible { get; }
        void ShowLogWindow();
        void ShowDiagnosticWindow();
        void ShowGridOverlay(bool isVisible);
        Task<bool> ShowWallpaperDialogWindowAsync(object wallpaper);
        void ShowWallpaperPreviewWindow(LibraryModel model);
        void ShowSplashWindow();
        void CloseSplashWindow();
        void ShowErrorMessageBox(string message, string title);
    }
}
