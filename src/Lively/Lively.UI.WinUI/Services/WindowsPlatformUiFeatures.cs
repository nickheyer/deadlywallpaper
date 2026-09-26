using Lively.Common;
using Lively.Common.Factories;
using Lively.Common.Helpers;
using Lively.Common.Helpers.Shell;
using Lively.Common.Services;
using Lively.Grpc.Client;
using Lively.Models;
using Lively.Models.Enums;
using Lively.UI.Shared.Services;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Threading.Tasks;
using UAC = UACHelper.UACHelper;

namespace Lively.UI.WinUI.Services
{
    public class WindowsPlatformUiFeatures : IPlatformUiFeatures
    {
        private readonly IApplicationsFactory appFactory;
        private readonly IDownloadService downloader;
        private readonly IDesktopCoreClient desktopCore;

        public WindowsPlatformUiFeatures(IApplicationsFactory appFactory, IDownloadService downloader, IDesktopCoreClient desktopCore)
        {
            this.appFactory = appFactory;
            this.downloader = downloader;
            this.desktopCore = desktopCore;
        }

        public bool IsElevated => UAC.IsElevated;

        public bool IsPackaged => PackageUtil.IsRunningAsPackaged;

        public string[] ApplicationFileExtensions => [".exe"];

        public IEnumerable<ApplicationModel> GetRunningApplications()
        {
            foreach (var process in Process.GetProcesses())
            {
                var hwnd = process.MainWindowHandle;
                if (hwnd == IntPtr.Zero || WindowUtil.IsUWPApp(hwnd) || !WindowUtil.IsVisibleTopLevelWindows(hwnd))
                    continue;

                var app = appFactory.CreateApp(hwnd);
                if (app is not null)
                    yield return app;
            }
        }

        public bool SupportsDesktopIconToggle => true;

        public void SetDesktopIconVisibility(bool isVisible) => DesktopUtil.SetDesktopIconVisibility(isVisible);

        public bool SupportsScreensaver => true;

        public bool IsScreensaverRegistered() => ScreensaverUtil.IsScreensaverSelected("Lively");

        public void OpenSystemScreensaverSettings()
        {
            try
            {
                // Ref: https://help.ivanti.com/res/help/en_us/iwc/2021/help/Content/20030.htm
                Process.Start(new ProcessStartInfo()
                {
                    FileName = "rundll32.exe",
                    Arguments = "shell32.dll,Control_RunDLL desk.cpl,,1",
                    UseShellExecute = true
                });
            }
            catch { /* Nothing to do */ }
        }

        public bool SupportsSystemColorSettings => true;

        public void OpenSystemColorSettings() => LinkUtil.OpenBrowser("ms-settings:colors");

        public bool SupportsPlayerSelection => true;

        public bool IsPlayerAvailable(LivelyMediaPlayer player)
        {
            return player switch
            {
                LivelyMediaPlayer.libvlc => false, //depreciated
                LivelyMediaPlayer.libmpv => false, //depreciated
                LivelyMediaPlayer.wmf => IsPluginInstalled(Constants.PlayerPartialPaths.WmfPath),
                LivelyMediaPlayer.libvlcExt => false,
                LivelyMediaPlayer.libmpvExt => false,
                LivelyMediaPlayer.mpv => IsPluginInstalled(Constants.PlayerPartialPaths.MpvPath),
                LivelyMediaPlayer.vlc => IsPluginInstalled(Constants.PlayerPartialPaths.VlcPath),
                _ => false,
            };
        }

        public bool IsGifPlayerAvailable(LivelyGifPlayer player)
        {
            return player switch
            {
                LivelyGifPlayer.win10Img => false, //xaml island
                LivelyGifPlayer.libmpvExt => false,
                LivelyGifPlayer.mpv => IsPluginInstalled(Constants.PlayerPartialPaths.MpvPath),
                _ => false,
            };
        }

        public bool IsWebBrowserAvailable(LivelyWebBrowser browser)
        {
            return browser switch
            {
                LivelyWebBrowser.cef => IsPluginInstalled(Constants.PlayerPartialPaths.CefSharpPath),
                LivelyWebBrowser.webview2 => IsPluginInstalled(Constants.PlayerPartialPaths.WebView2Path),
                _ => false,
            };
        }

        public bool IsStreamDownloaderAvailable => IsPluginInstalled(Path.Combine(Constants.PlayerPartialPaths.MpvDir, "youtube-dl.exe"));

        public bool SupportsTaskbarTheme => true;

        public bool SupportsRemoteDesktopPause => true;

        private bool IsPluginInstalled(string relativePath)
        {
            try
            {
                return File.Exists(Path.Combine(desktopCore.BaseDirectory, relativePath));
            }
            catch (ArgumentException)
            {
                // BaseDirectory is empty until the core reported its stats.
                return false;
            }
        }

        public bool SupportsWebViewRuntimeInstall => !PackageUtil.IsRunningAsPackaged;

        public bool IsWebViewRuntimeAvailable => WebViewUtil.IsWebView2Available();

        public string WebViewRuntimeDownloadUrl => WebViewUtil.DownloadUrl;

        public Task<bool> TryInstallWebViewRuntimeAsync() => WebViewUtil.InstallWebView2(downloader);
    }
}
