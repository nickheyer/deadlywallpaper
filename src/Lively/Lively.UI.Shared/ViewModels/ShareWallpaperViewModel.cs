using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Lively.Common;
using Lively.Common.Services;
using Lively.Gallery.Client;
using Lively.Models;
using System;
using System.IO;
using System.Threading.Tasks;

namespace Lively.UI.Shared.ViewModels
{
    public partial class ShareWallpaperViewModel : ObservableObject
    {
        private readonly GalleryClient galleryClient;
        private readonly LibraryViewModel libraryVm;
        private readonly IFileService fileService;

        public ShareWallpaperViewModel(GalleryClient galleryClient,
            LibraryViewModel libraryVm,
            IFileService fileService)
        {
            this.galleryClient = galleryClient;
            this.libraryVm = libraryVm;
            this.fileService = fileService;

            if (galleryClient.IsLoggedIn)
                canUploadFile = true;

#if DEBUG != true
            canUploadFile = false;
#endif
        }

        [ObservableProperty]
        private LibraryModel model;

        private bool canExportFile = true;
        private RelayCommand _exportFileCommand;
        public RelayCommand ExportFileCommand =>
            _exportFileCommand ??= new RelayCommand(async () => await ExportFile(), () => canExportFile);

        private bool canUploadFile = false;
        private RelayCommand _galleryFileUploadCommand;
        public RelayCommand GalleryFileUploadCommand =>
            _galleryFileUploadCommand ??= new RelayCommand(async () => await UploadFile(), () => canUploadFile);

        private bool canCopyLink = false;
        private RelayCommand _copyLinkCommand;
        public RelayCommand CopyLinkCommand =>
           _copyLinkCommand ??= new RelayCommand(async () => await CopyLink(), () => canCopyLink);

        private async Task ExportFile()
        {
            try
            {
                canExportFile = false;
                ExportFileCommand.NotifyCanExecuteChanged();

                var suggestdFileName = Model.Title;
                var file = await fileService.PickSaveFileAsync(suggestdFileName, [("Compressed archive", [".zip"])]);
                if (file != null)
                {
                    await libraryVm.WallpaperExport(Model, file);
                    await fileService.OpenFolderAsync(file);
                }
            }
            catch (Exception)
            {
                //TODO
            }
            finally
            {
                canExportFile = true;
                ExportFileCommand.NotifyCanExecuteChanged();
            }
        }

        private async Task UploadFile()
        {
            var tempFile = Path.Combine(Constants.CommonPaths.TempDir, Path.GetRandomFileName() + ".zip");
            try
            {
                canUploadFile = false;
                GalleryFileUploadCommand.NotifyCanExecuteChanged();

                await libraryVm.WallpaperExport(Model, tempFile);
                using var fs = new FileStream(tempFile, FileMode.Open);
                await galleryClient.UploadWallpaperAsync(fs);
            }
            finally
            {
                canUploadFile = true;
                GalleryFileUploadCommand.NotifyCanExecuteChanged();

                try
                {
                    File.Delete(tempFile);
                }
                catch { }
            }
        }

        private async Task CopyLink()
        {
            //TODO
        }
    }
}
