using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Platform.Storage;
using Lively.Common;
using Lively.Common.Services;
using Lively.Grpc.Client;
using Lively.Models;
using Lively.UI.Shared.ViewModels;
using System;
using System.IO;
using System.Linq;

namespace Lively.UI.Avalonia.Views
{
    public partial class LibraryView : UserControl
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly IResourceService i18n;
        private readonly IUserSettingsClient userSettings;
        private readonly IDesktopCoreClient desktopCore;
        private readonly LibraryViewModel libraryVm;
        private readonly MainViewModel mainVm;
        private readonly IDialogService dialogService;

        public LibraryView(IDesktopCoreClient desktopCore,
            LibraryViewModel libraryVm,
            MainViewModel mainVm,
            IUserSettingsClient userSettings,
            IDialogService dialogService,
            IResourceService i18n)
        {
            this.desktopCore = desktopCore;
            this.libraryVm = libraryVm;
            this.mainVm = mainVm;
            this.userSettings = userSettings;
            this.dialogService = dialogService;
            this.i18n = i18n;

            InitializeComponent();
            DataContext = libraryVm;

            DragDrop.SetAllowDrop(this, true);
            AddHandler(DragDrop.DragOverEvent, Page_DragOver);
            AddHandler(DragDrop.DragLeaveEvent, Page_DragLeave);
            AddHandler(DragDrop.DropEvent, Page_Drop);
        }

        private void TileGrid_ItemClicked(object sender, object item)
        {
            if (item is LibraryModel model)
                libraryVm.LibraryClickCommand.Execute(model);
        }

        #region file drop

        private async void Page_Drop(object sender, DragEventArgs e)
        {
            AddFilePanel.IsVisible = false;

            var files = e.Data.GetFiles()?.Select(x => x.TryGetLocalPath()).Where(x => !string.IsNullOrEmpty(x)).ToList() ?? [];
            if (files.Count == 0)
            {
                var text = e.Data.GetText();
                var link = text?.Split('\n', StringSplitOptions.RemoveEmptyEntries).FirstOrDefault()?.Trim();
                if (!LinkUtil.TrySanitizeUrl(link, out Uri uri))
                    return;

                Logger.Info($"Dropped string {uri}");
                try
                {
                    await libraryVm.AddWallpaperLink(uri, true);
                }
                catch (Exception ie)
                {
                    await dialogService.ShowDialogAsync(ie.Message,
                        i18n.GetString("TextError"),
                        i18n.GetString("TextOK"));
                }
            }
            else if (files.Count == 1)
            {
                var item = files[0];
                Logger.Info($"Dropped file {item}");
                if (string.IsNullOrWhiteSpace(Path.GetExtension(item)))
                    return;

                try
                {
                    var creationType = await dialogService.ShowWallpaperCreateDialogAsync(item);
                    if (creationType is null)
                        return;

                    switch (creationType)
                    {
                        case WallpaperCreateType.none:
                            await libraryVm.AddWallpaperFile(item, true);
                            break;
                        case WallpaperCreateType.depthmap:
                            var result = await dialogService.ShowDepthWallpaperDialogAsync(item);
                            if (result is not null)
                                await desktopCore.SetWallpaper(result, userSettings.Settings.SelectedDisplay);
                            break;
                    }
                }
                catch (Exception ie)
                {
                    await dialogService.ShowDialogAsync(ie.Message,
                        i18n.GetString("TextError"),
                        i18n.GetString("TextOK"));
                }
            }
            else
            {
                await mainVm.AddWallpapers(files);
            }
        }

        private void Page_DragOver(object sender, DragEventArgs e)
        {
            e.DragEffects = DragDropEffects.Copy;
            AddFilePanel.IsVisible = true;
        }

        private void Page_DragLeave(object sender, DragEventArgs e)
        {
            AddFilePanel.IsVisible = false;
        }

        #endregion //file drop
    }
}
