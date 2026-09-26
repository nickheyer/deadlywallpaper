using Avalonia.Controls;
using Avalonia.Platform.Storage;
using Lively.Common;
using Lively.Common.Services;
using Lively.Models.Enums;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// File and folder pickers backed by the Avalonia storage provider (the XDG desktop portal on Linux).
    /// </summary>
    public class FileService : IFileService
    {
        private readonly IResourceService i18n;

        public FileService(IResourceService i18n)
        {
            this.i18n = i18n;
        }

        public async Task<IReadOnlyList<string>> PickFileAsync(IEnumerable<(string label, string[] extensions)> filters, bool multipleFile = false)
        {
            var storage = GetStorageProvider();
            var files = await storage.OpenFilePickerAsync(new FilePickerOpenOptions
            {
                AllowMultiple = multipleFile,
                FileTypeFilter = filters.Select(ToFileType).ToList(),
            });
            return files.Select(x => x.TryGetLocalPath()).Where(x => !string.IsNullOrEmpty(x)).ToList();
        }

        public async Task<IReadOnlyList<string>> PickFileAsync(WallpaperType type, bool multipleFile = false)
        {
            var filters = GetFilter(type);
            return await PickFileAsync([filters], multipleFile);
        }

        public async Task<IReadOnlyList<string>> PickWallpaperFile(bool multipleFile = false)
        {
            var filters = GetWallpaperFilters(true);
            filters.Add(("Lively Wallpaper", [".zip"]));

            return await PickFileAsync(filters, multipleFile);
        }

        public async Task<string> PickSaveFileAsync(string suggestedFileName, IEnumerable<(string label, string[] extensions)> fileTypeChoices)
        {
            var storage = GetStorageProvider();
            var choices = fileTypeChoices.ToList();
            var defaultExtension = choices.FirstOrDefault().extensions?.FirstOrDefault(x => x != "*");
            var file = await storage.SaveFilePickerAsync(new FilePickerSaveOptions
            {
                SuggestedFileName = suggestedFileName,
                DefaultExtension = defaultExtension?.TrimStart('.'),
                FileTypeChoices = choices.Select(ToFileType).ToList(),
                ShowOverwritePrompt = true,
            });
            return file?.TryGetLocalPath();
        }

        public async Task<string> PickFolderAsync(string[] filters)
        {
            var storage = GetStorageProvider();
            var folders = await storage.OpenFolderPickerAsync(new FolderPickerOpenOptions
            {
                AllowMultiple = false,
            });
            return folders.FirstOrDefault()?.TryGetLocalPath();
        }

        public Task OpenFolderAsync(string path)
        {
            var target = File.Exists(path) ? Path.GetDirectoryName(path) : path;
            if (string.IsNullOrEmpty(target) || !Directory.Exists(target))
                throw new DirectoryNotFoundException($"Folder not found: {path}");

            Process.Start(new ProcessStartInfo
            {
                FileName = "xdg-open",
                ArgumentList = { target },
                UseShellExecute = false,
                RedirectStandardOutput = true,
                RedirectStandardError = true,
            });
            return Task.CompletedTask;
        }

        private static IStorageProvider GetStorageProvider()
        {
            var topLevel = TopLevel.GetTopLevel(App.CurrentTopLevel)
                ?? throw new InvalidOperationException("A window must be open before a file picker can be shown.");
            return topLevel.StorageProvider;
        }

        private static FilePickerFileType ToFileType((string label, string[] extensions) filter)
        {
            var patterns = filter.extensions.Select(ext => ext == "*" ? "*" : $"*{ext}").ToList();
            return new FilePickerFileType(filter.label) { Patterns = patterns };
        }

        private List<(string label, string[] extensions)> GetWallpaperFilters(bool includeAllFiles = false)
        {
            var filters = new List<(string label, string[] extensions)>();
            if (includeAllFiles)
                filters.Add((i18n.GetString("TextAllFiles"), ["*"]));

            foreach (var format in FileTypes.SupportedFormats)
                filters.Add(GetFilter(format.Type));

            return filters;
        }

        private (string label, string[] extensions) GetFilter(WallpaperType wallpaperType)
        {
            var format = FileTypes.SupportedFormats.First(x => x.Type == wallpaperType);
            var label = i18n.GetString(format.Type);
            return (label, format.Extentions);
        }
    }
}
