using Avalonia;
using Avalonia.Controls;
using Lively.Models.UserControls;
using System;
using System.Collections.ObjectModel;
using System.IO;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Read-only tree of the first two levels of a folder, shown when confirming a wallpaper project directory.
    /// </summary>
    public partial class FolderView : UserControl
    {
        public static readonly StyledProperty<string> FolderPathProperty =
            AvaloniaProperty.Register<FolderView, string>(nameof(FolderPath));

        public static readonly StyledProperty<ObservableCollection<ExplorerItem>> DataProperty =
            AvaloniaProperty.Register<FolderView, ObservableCollection<ExplorerItem>>(nameof(Data));

        public FolderView()
        {
            InitializeComponent();
            Data = new ObservableCollection<ExplorerItem>();
        }

        public string FolderPath
        {
            get => GetValue(FolderPathProperty);
            set => SetValue(FolderPathProperty, value);
        }

        public ObservableCollection<ExplorerItem> Data
        {
            get => GetValue(DataProperty);
            private set => SetValue(DataProperty, value);
        }

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);
            if (change.Property == FolderPathProperty)
                Data = GetDataFromFolder(FolderPath);
        }

        private static ObservableCollection<ExplorerItem> GetDataFromFolder(string path, int maxDepth = 2)
        {
            var items = new ObservableCollection<ExplorerItem>();
            if (string.IsNullOrWhiteSpace(path) || !Directory.Exists(path))
                return items;

            TraverseFolder(path, items, 0, maxDepth);
            return items;
        }

        private static void TraverseFolder(string path, ObservableCollection<ExplorerItem> items, int currentDepth, int maxDepth)
        {
            if (currentDepth >= maxDepth)
                return;

            try
            {
                foreach (var dir in Directory.GetDirectories(path))
                {
                    var folderItem = new ExplorerItem
                    {
                        Name = Path.GetFileName(dir),
                        Type = ExplorerItem.ExplorerItemType.Folder
                    };
                    TraverseFolder(dir, folderItem.Children, currentDepth + 1, maxDepth);
                    items.Add(folderItem);
                }

                foreach (var file in Directory.GetFiles(path))
                {
                    items.Add(new ExplorerItem
                    {
                        Name = Path.GetFileName(file),
                        Type = ExplorerItem.ExplorerItemType.File
                    });
                }
            }
            catch (UnauthorizedAccessException)
            {
                // Entries that cannot be listed are skipped.
            }
            catch (IOException)
            {
                // Entries that cannot be listed are skipped.
            }
        }
    }
}
