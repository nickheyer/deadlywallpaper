using ImageMagick;
using Lively.Common;
using Lively.Common.Factories;
using Lively.Common.Helpers.Storage;
using Lively.Common.Linux.DBus.Notifications;
using Lively.Common.Services;
using Lively.Core.Display;
using Lively.Factories;
using Lively.Models;
using Lively.Models.Enums;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// The core process has no UI toolkit. Windows are either native host processes in windowed
    /// mode (previews) or dialogs shown by the UI process on the core's behalf.
    /// </summary>
    public sealed class LinuxWindowService : IWindowService
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private const int ThumbnailWidth = 1280;
        private const int ThumbnailHeight = 720;
        private const int PreviewFrames = 60;
        private const int PreviewCaptureIntervalMs = 1000 / 30;
        private const int PreviewFrameDelayMs = 1000 / 120;

        private readonly LinuxRunnerService runner;
        private readonly IWallpaperPluginFactory wallpaperFactory;
        private readonly IWallpaperLibraryFactory libraryFactory;
        private readonly IUserSettingsService userSettings;
        private readonly IDisplayManager displayManager;
        private readonly NotificationService notifications;
        private readonly IResourceService i18n;
        private readonly List<IWallpaper> previews = new List<IWallpaper>();

        public bool IsGridOverlayVisible => false;

        public LinuxWindowService(IRunnerService runner, IWallpaperPluginFactory wallpaperFactory, IWallpaperLibraryFactory libraryFactory,
            IUserSettingsService userSettings, IDisplayManager displayManager, NotificationService notifications, IResourceService i18n)
        {
            this.runner = (LinuxRunnerService)runner;
            this.wallpaperFactory = wallpaperFactory;
            this.libraryFactory = libraryFactory;
            this.userSettings = userSettings;
            this.displayManager = displayManager;
            this.notifications = notifications;
            this.i18n = i18n;
        }

        public void ShowLogWindow()
        {
            var latest = new DirectoryInfo(Constants.CommonPaths.LogDir).GetFiles("*.txt").OrderByDescending(f => f.LastWriteTimeUtc).FirstOrDefault();
            OpenExternally(latest?.FullName ?? Constants.CommonPaths.LogDir);
        }

        public void ShowDiagnosticWindow() => OpenExternally(Constants.CommonPaths.LogDir);

        public void ShowGridOverlay(bool isVisible)
        {
            throw new PlatformNotSupportedException("The window coverage debug overlay draws over the desktop with Win32 layered windows; it is not available on Wayland.");
        }

        public void ShowWallpaperPreviewWindow(LibraryModel model)
        {
            var wallpaper = wallpaperFactory.CreateWallpaper(model, userSettings.Settings.SelectedDisplay ?? displayManager.PrimaryDisplayMonitor, WallpaperArrangement.per, isWindowed: true);
            lock (previews)
                previews.Add(wallpaper);
            wallpaper.Exited += (s, e) =>
            {
                lock (previews)
                    previews.Remove(wallpaper);
                wallpaper.Dispose();
            };
            _ = wallpaper.ShowAsync().ContinueWith(t =>
            {
                if (t.IsFaulted)
                {
                    Logger.Error($"Preview failed: {t.Exception?.GetBaseException().Message}");
                    lock (previews)
                        previews.Remove(wallpaper);
                    wallpaper.Dispose();
                }
            });
        }

        /// <summary>
        /// The add/edit wallpaper flow: the wallpaper is already running in a preview window (started
        /// by the RPC server); ask the UI for the metadata, then capture the thumbnail and preview
        /// clip from the running wallpaper and store the LivelyInfo, exactly what LibraryPreview does.
        /// </summary>
        public async Task<bool> ShowWallpaperDialogWindowAsync(object wallpaperObj)
        {
            var wallpaper = (IWallpaper)wallpaperObj;
            var model = wallpaper.Model;
            var folder = model.LivelyInfoFolderPath;
            var thumbnailPath = Path.Combine(folder, "lively_t.jpg");

            // First snapshot so the dialog can show what the user is about to save.
            await wallpaper.ScreenCapture(thumbnailPath);

            var answer = await runner.RequestWallpaperDataAsync(folder, model.LivelyInfo.Title, model.LivelyInfo.Author,
                model.LivelyInfo.Desc, model.LivelyInfo.Contact, thumbnailPath, TimeSpan.FromMinutes(30));
            if (!answer.Ok)
                return false;

            runner.SetBusyUI(true);
            try
            {
                await wallpaper.ScreenCapture(thumbnailPath);
                await ResizeThumbnailAsync(thumbnailPath);

                string previewPath = null;
                if (userSettings.Settings.GifCapture && wallpaper.Category != WallpaperType.picture)
                {
                    previewPath = Path.Combine(folder, "lively_p.gif");
                    await CapturePreviewGifAsync(wallpaper, previewPath);
                }

                model.LivelyInfo.Title = answer.Title;
                model.LivelyInfo.Author = answer.Author;
                model.LivelyInfo.Desc = answer.Desc;
                model.LivelyInfo.Contact = answer.Contact;
                model.LivelyInfo.Thumbnail = model.LivelyInfo.IsAbsolutePath ? thumbnailPath : Path.GetFileName(thumbnailPath);
                model.LivelyInfo.Preview = previewPath == null ? null : (model.LivelyInfo.IsAbsolutePath ? previewPath : Path.GetFileName(previewPath));

                await libraryFactory.ConvertAbsoluteToRelativePathAsync(model.LivelyInfo, folder);
                JsonStorage<LivelyInfoModel>.StoreData(Path.Combine(folder, "LivelyInfo.json"), model.LivelyInfo);
                return true;
            }
            finally
            {
                runner.SetBusyUI(false);
            }
        }

        private static async Task ResizeThumbnailAsync(string path)
        {
            await Task.Run(() =>
            {
                using var image = new MagickImage(path);
                image.Resize(new MagickGeometry((uint)ThumbnailWidth, (uint)ThumbnailHeight) { IgnoreAspectRatio = false });
                image.Quality = 90;
                image.Write(path, MagickFormat.Jpeg);
            });
        }

        private static async Task CapturePreviewGifAsync(IWallpaper wallpaper, string gifPath)
        {
            var frameDir = Path.Combine(Constants.CommonPaths.TempDir, "preview-" + Path.GetRandomFileName());
            Directory.CreateDirectory(frameDir);
            try
            {
                var frames = new List<string>();
                var watch = Stopwatch.StartNew();
                for (var i = 0; i < PreviewFrames; i++)
                {
                    var frame = Path.Combine(frameDir, $"{i:000}.jpg");
                    await wallpaper.ScreenCapture(frame);
                    frames.Add(frame);
                    var due = (i + 1) * PreviewCaptureIntervalMs;
                    var wait = due - (int)watch.ElapsedMilliseconds;
                    if (wait > 0)
                        await Task.Delay(wait);
                }

                await Task.Run(() =>
                {
                    using var collection = new MagickImageCollection();
                    foreach (var frame in frames)
                    {
                        var image = new MagickImage(frame);
                        image.Resize(new MagickGeometry(640, 360) { IgnoreAspectRatio = false });
                        image.AnimationDelay = (uint)Math.Max(1, PreviewFrameDelayMs / 10);
                        collection.Add(image);
                    }
                    collection.Optimize();
                    collection.Write(gifPath, MagickFormat.Gif);
                });
            }
            finally
            {
                try { Directory.Delete(frameDir, true); } catch (IOException) { }
            }
        }

        public void ShowSplashWindow()
        {
            _ = notifications.ShowAsync(i18n.GetString("TitleAppName"), i18n.GetString("PleaseWait/Text"), 5000);
        }

        public void CloseSplashWindow() { }

        public void ShowErrorMessageBox(string message, string title)
        {
            Logger.Error($"{title}: {message}");
            _ = notifications.ShowAsync(title, message, 10000);
        }

        private static void OpenExternally(string path)
        {
            Process.Start(new ProcessStartInfo { FileName = "xdg-open", ArgumentList = { path }, UseShellExecute = false });
        }
    }
}
