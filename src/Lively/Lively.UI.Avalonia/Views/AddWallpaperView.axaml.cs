using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Platform.Storage;
using Lively.Common;
using Lively.UI.Shared.ViewModels;
using System;
using System.IO;
using System.Linq;

namespace Lively.UI.Avalonia.Views
{
    public partial class AddWallpaperView : UserControl
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly AddWallpaperViewModel viewModel;

        public AddWallpaperView(AddWallpaperViewModel viewModel)
        {
            this.viewModel = viewModel;
            InitializeComponent();
            DataContext = viewModel;

            DragDrop.SetAllowDrop(this, true);
            AddHandler(DragDrop.DragOverEvent, Page_DragOver);
            AddHandler(DragDrop.DragLeaveEvent, Page_DragLeave);
            AddHandler(DragDrop.DropEvent, Page_Drop);
        }

        private void Page_Drop(object sender, DragEventArgs e)
        {
            AddPanel.IsVisible = true;
            AddPanelDrop.IsVisible = false;

            var files = e.Data.GetFiles()?.Select(x => x.TryGetLocalPath()).Where(x => !string.IsNullOrEmpty(x)).ToList() ?? [];
            if (files.Count == 0)
            {
                var text = e.Data.GetText();
                var link = text?.Split('\n', StringSplitOptions.RemoveEmptyEntries).FirstOrDefault()?.Trim();
                if (!LinkUtil.TrySanitizeUrl(link, out Uri uri))
                    return;

                Logger.Info($"Dropped string {uri}");
                viewModel.AddWallpaperLink(uri);
            }
            else if (files.Count == 1)
            {
                var item = files[0];
                Logger.Info($"Dropped file {item}");
                if (string.IsNullOrWhiteSpace(Path.GetExtension(item)))
                    return;

                viewModel.AddWallpaperFile(item);
            }
            else
            {
                viewModel.AddWallpaperFiles(files);
            }
        }

        private void Page_DragOver(object sender, DragEventArgs e)
        {
            e.DragEffects = DragDropEffects.Copy;
            AddPanel.IsVisible = false;
            AddPanelDrop.IsVisible = true;
        }

        private void Page_DragLeave(object sender, DragEventArgs e)
        {
            AddPanel.IsVisible = true;
            AddPanelDrop.IsVisible = false;
        }
    }
}
