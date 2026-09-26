using Avalonia;
using Avalonia.Controls;
using Avalonia.Data;
using Avalonia.Interactivity;
using Avalonia.Threading;
using CommunityToolkit.Mvvm.Input;
using Lively.Common;
using Lively.Common.Extensions;
using Lively.Common.Helpers.Files;
using Lively.Common.Linux.Media;
using Lively.Common.Services;
using Lively.Models.Enums;
using Lively.Models.UserControls;
using Lively.UI.Avalonia.Services;
using Microsoft.Extensions.DependencyInjection;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using System.Windows.Input;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Picker for the files inside a wallpaper sub folder (LivelyProperties "folderDropdown"): lists the folder,
    /// copies new files into it and deletes files from it.
    /// </summary>
    public partial class FolderDropdown : UserControl
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        public static readonly StyledProperty<string> FolderNameProperty =
            AvaloniaProperty.Register<FolderDropdown, string>(nameof(FolderName));

        public static readonly StyledProperty<string> FileNameProperty =
            AvaloniaProperty.Register<FolderDropdown, string>(nameof(FileName), defaultBindingMode: BindingMode.TwoWay);

        public static readonly StyledProperty<string> FilterProperty =
            AvaloniaProperty.Register<FolderDropdown, string>(nameof(Filter));

        public static readonly StyledProperty<string> ParentFolderPathProperty =
            AvaloniaProperty.Register<FolderDropdown, string>(nameof(ParentFolderPath));

        public static readonly StyledProperty<ObservableCollection<FolderDropdownUserControlModel>> FilesProperty =
            AvaloniaProperty.Register<FolderDropdown, ObservableCollection<FolderDropdownUserControlModel>>(nameof(Files));

        public static readonly StyledProperty<FolderDropdownUserControlModel> SelectedFileProperty =
            AvaloniaProperty.Register<FolderDropdown, FolderDropdownUserControlModel>(nameof(SelectedFile), defaultBindingMode: BindingMode.TwoWay);

        public static readonly StyledProperty<ICommand> CommandProperty =
            AvaloniaProperty.Register<FolderDropdown, ICommand>(nameof(Command));

        public static readonly StyledProperty<object> CommandParameterProperty =
            AvaloniaProperty.Register<FolderDropdown, object>(nameof(CommandParameter));

        private readonly string[] imageExtensions;
        private string cacheDir;
        private bool isInitialized;

        public FolderDropdown()
        {
            InitializeComponent();
            OpenFileCommand = new AsyncRelayCommand(OpenFileAsync, () => isInitialized);
            imageExtensions = FileTypes.SupportedFormats
                .Where(x => x.Type == WallpaperType.picture || x.Type == WallpaperType.gif)
                .SelectMany(x => x.Extentions)
                .ToArray();
        }

        public string FolderName
        {
            get => GetValue(FolderNameProperty);
            set => SetValue(FolderNameProperty, value);
        }

        public string FileName
        {
            get => GetValue(FileNameProperty);
            set => SetValue(FileNameProperty, value);
        }

        public string Filter
        {
            get => GetValue(FilterProperty);
            set => SetValue(FilterProperty, value);
        }

        public string ParentFolderPath
        {
            get => GetValue(ParentFolderPathProperty);
            set => SetValue(ParentFolderPathProperty, value);
        }

        public ObservableCollection<FolderDropdownUserControlModel> Files
        {
            get => GetValue(FilesProperty);
            private set => SetValue(FilesProperty, value);
        }

        public FolderDropdownUserControlModel SelectedFile
        {
            get => GetValue(SelectedFileProperty);
            set => SetValue(SelectedFileProperty, value);
        }

        public ICommand Command
        {
            get => GetValue(CommandProperty);
            set => SetValue(CommandProperty, value);
        }

        public object CommandParameter
        {
            get => GetValue(CommandParameterProperty);
            set => SetValue(CommandParameterProperty, value);
        }

        public AsyncRelayCommand OpenFileCommand { get; }

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);

            if (change.Property == FolderNameProperty || change.Property == FilterProperty || change.Property == ParentFolderPathProperty || change.Property == FileNameProperty)
            {
                // FileName can be null (nothing selected or file missing); the other three are required.
                if (FolderName != null && Filter != null && ParentFolderPath != null)
                    InitializeControl();
            }
            else if (change.Property == SelectedFileProperty && isInitialized)
            {
                FileName = SelectedFile?.FileName;
                Command?.Execute(CommandParameter);
            }
        }

        private void InitializeControl()
        {
            if (isInitialized)
                return;

            isInitialized = true;
            OpenFileCommand.NotifyCanExecuteChanged();
            var folderPath = Path.Combine(ParentFolderPath, FolderName);
            var filePath = !string.IsNullOrWhiteSpace(FileName) ? Path.Combine(ParentFolderPath, FolderName, FileName) : null;
            Directory.CreateDirectory(folderPath);

            // Thumbnail cache keyed by the wallpaper folder name (the LivelyInfo.json root), which is unique.
            var rootFolderName = Path.GetFileName(Path.GetDirectoryName(folderPath));
            cacheDir = Path.Combine(Constants.CommonPaths.TempDir, "folderDropdown", rootFolderName);
            Directory.CreateDirectory(cacheDir);

            var files = new ObservableCollection<FolderDropdownUserControlModel>();
            foreach (var item in FileUtil.GetFiles(folderPath, Filter, SearchOption.TopDirectoryOnly))
                files.Add(CreateModel(item));
            Files = files;

            // If the file is not found the next available file is selected.
            SelectedFile = File.Exists(filePath)
                ? files.FirstOrDefault(x => x.FileName == Path.GetFileName(filePath))
                : files.FirstOrDefault();
        }

        private void Delete_Button_Click(object sender, RoutedEventArgs e)
        {
            if (sender is not Button button || button.DataContext is not FolderDropdownUserControlModel obj)
                return;

            // Make the selection before deletion to avoid sending NULL.
            if (obj == SelectedFile)
            {
                var index = Files.IndexOf(obj);
                if (index >= 0 && index < Files.Count - 1)
                    SelectedFile = Files[index + 1];
                else if (index == Files.Count - 1 && Files.Count > 1)
                    SelectedFile = Files[index - 1];
            }

            try
            {
                Files.Remove(obj);
                File.Delete(obj.FilePath);
                DeleteThumbnailTempCache(obj.FilePath);
            }
            catch (Exception ex)
            {
                Logger.Error(ex);
            }
        }

        private async Task OpenFileAsync()
        {
            var fileService = App.Services.GetRequiredService<IFileService>();
            var selectedFiles = await fileService.PickFileAsync(GetPickerFilters(Filter), true);
            if (selectedFiles.Count == 0)
                return;

            var destFiles = new List<string>();
            var destFolder = Path.Combine(ParentFolderPath, FolderName);
            foreach (var srcFile in selectedFiles)
            {
                var destFile = Path.Combine(destFolder, Path.GetFileName(srcFile));
                if (File.Exists(destFile))
                    destFile = FileUtil.NextAvailableFilename(destFile);
                File.Copy(srcFile, destFile);
                destFiles.Add(destFile);
            }
            // Add copied files to the bottom of the dropdown.
            foreach (var file in destFiles.OrderBy(x => Path.GetFileName(x)))
                Files.Add(CreateModel(file));

            // Select the new file when a single file was chosen or nothing was selected before.
            if (selectedFiles.Count == 1 || SelectedFile is null)
                SelectedFile = Files[Files.Count - 1];
        }

        private static IEnumerable<(string label, string[] extensions)> GetPickerFilters(string folderDropDownFilter)
        {
            if (folderDropDownFilter == "*")
                return [("*", ["*"])];

            var extensions = folderDropDownFilter.Split('|', StringSplitOptions.RemoveEmptyEntries)
                .Select(x => x.Replace("*", string.Empty).Trim())
                .Where(x => x.Length > 0)
                .ToArray();
            return [(folderDropDownFilter, extensions)];
        }

        private FolderDropdownUserControlModel CreateModel(string file)
        {
            var isImage = imageExtensions.Contains(Path.GetExtension(file), StringComparer.OrdinalIgnoreCase);
            var model = new FolderDropdownUserControlModel
            {
                ImagePath = isImage ? file : null,
                FileName = Path.GetFileName(file),
                FilePath = file
            };
            if (!isImage)
                _ = LoadThumbnailAsync(model);
            return model;
        }

        /// <summary>
        /// Videos and gifs get a frame grab through ffmpeg (cached in the temp folder), other files their mime-type icon.
        /// </summary>
        private async Task LoadThumbnailAsync(FolderDropdownUserControlModel model)
        {
            string imagePath = null;
            var cacheFile = Path.Combine(cacheDir, model.FileName + ".jpg");
            if (File.Exists(cacheFile))
            {
                imagePath = cacheFile;
            }
            else if (FileTypes.GetFileType(model.FilePath).IsVideoWallpaper())
            {
                try
                {
                    await new LinuxThumbnailService().CreateThumbnailAsync(model.FilePath, cacheFile, 128, 128);
                    imagePath = cacheFile;
                }
                catch (Exception ex)
                {
                    Logger.Warn($"Thumbnail for {model.FilePath} failed, showing the file type icon instead: {ex.Message}");
                }
            }
            imagePath ??= await Task.Run(() => LinuxIconLookup.ResolveMimeIcon(model.FilePath, 128));

            await Dispatcher.UIThread.InvokeAsync(() => model.ImagePath = imagePath);
        }

        private void DeleteThumbnailTempCache(string filePath)
        {
            var cacheFilePath = Path.Combine(cacheDir, Path.GetFileName(filePath) + ".jpg");
            if (File.Exists(cacheFilePath))
                File.Delete(cacheFilePath);
        }
    }
}
