using Lively.Common.Services;
using Lively.Core;
using Lively.Core.Display;
using Lively.Extensions;
using Lively.Models;
using Lively.Views;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using System.Windows;
using System.Windows.Threading;

namespace Lively.Services
{
    public class WindowService : IWindowService
    {
        public bool IsGridOverlayVisible => isGridOverlayVisible;

        private readonly IDisplayManager displayManager;
        private readonly IRunnerService runner;
        private readonly IUserSettingsService userSettings;
        private readonly List<WindowCoverageDebugOverlay> gridOverlays = [];
        private bool isGridOverlayVisible;
        private DebugLog debugLogWindow;
        private DiagnosticMenu diagnosticWindow;
        private SplashWindow splashWindow;

        public WindowService(IRunnerService runner, IDisplayManager displayManager, IUserSettingsService userSettings)
        {
            this.runner = runner;
            this.displayManager = displayManager;
            this.userSettings = userSettings;
        }

        public void ShowLogWindow()
        {
            if (debugLogWindow != null)
                return;

            debugLogWindow = new DebugLog();
            debugLogWindow.Closed += (s, e) => debugLogWindow = null;
            debugLogWindow.Show();
        }

        public void ShowDiagnosticWindow()
        {
            if (diagnosticWindow != null)
                return;

            diagnosticWindow = new DiagnosticMenu();
            diagnosticWindow.Closed += (s, e) => diagnosticWindow = null;
            diagnosticWindow.Show();
        }

        public void ShowGridOverlay(bool isVisible)
        {
            if (isVisible)
                ShowGridOverlayInternal();
            else
                CloseGridOverlayInternal();
        }


        public async Task<bool> ShowWallpaperDialogWindowAsync(object wallpaper)
        {
            bool? success = false;
            await Application.Current.Dispatcher.InvokeAsync(() =>
            {
                var previewWindow = new LibraryPreview(wallpaper as IWallpaper)
                {
                    Topmost = true,
                    ShowActivated = true,
                    WindowStartupLocation = WindowStartupLocation.CenterScreen
                };
                previewWindow.Loaded += (s, e) =>
                {
                    if (runner.IsVisibleUI)
                        previewWindow.CenterToWindow(runner.HwndUI);
                };

                try
                {
                    success = previewWindow.ShowDialog();
                }
                catch
                {
                    previewWindow.Close();
                    throw;
                }
            });
            return success ?? false;
        }

        public void ShowWallpaperPreviewWindow(LibraryModel model)
        {
            _ = Application.Current.Dispatcher.Invoke(DispatcherPriority.Normal, new ThreadStart(delegate
            {
                var preview = new WallpaperPreview(model, userSettings.Settings.SelectedDisplay, userSettings.Settings.WallpaperArrangement) {
                    // Default incase UI not running.
                    WindowStartupLocation = WindowStartupLocation.CenterScreen,
                };
                preview.Show();
                // Center preview relative to UI.
                if (runner.IsVisibleUI)
                    preview.CenterToWindow(runner.HwndUI);
                // Re-activate incase launching wallpaper loses focus.
                preview.Activate();
            }));
        }

        public void ShowSplashWindow()
        {
            if (splashWindow != null)
                return;

            splashWindow = new SplashWindow(0, 500);
            splashWindow.Show();
        }

        public void CloseSplashWindow()
        {
            splashWindow?.Close();
            splashWindow = null;
        }

        public void ShowErrorMessageBox(string message, string title)
        {
            MessageBox.Show(message, title, MessageBoxButton.OK, MessageBoxImage.Error);
        }

        private void ShowGridOverlayInternal()
        {
            if (isGridOverlayVisible)
                return;

            isGridOverlayVisible = true;
            foreach (var display in displayManager.DisplayMonitors)
            {
                var gridOverlay = new WindowCoverageDebugOverlay(display);
                gridOverlay.Show();
                gridOverlays.Add(gridOverlay);
            }
        }

        private void CloseGridOverlayInternal()
        {
            if (!isGridOverlayVisible)
                return;

            isGridOverlayVisible = false;
            foreach (var gridOverlay in gridOverlays)
                gridOverlay.Close();

            gridOverlays.Clear();
        }
    }
}
