using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Lively.Common.Factories;
using Lively.Common.Services;
using Lively.Models;
using Lively.Models.Enums;
using Lively.UI.Shared.Collections;
using Lively.UI.Shared.Services;
using System.Collections.ObjectModel;
using System.Linq;
using System.Threading.Tasks;

namespace Lively.UI.Shared.ViewModels
{
    public partial class FindMoreAppsViewModel : ObservableObject
    {
        private readonly IApplicationsFactory appFactory;
        private readonly IPlatformUiFeatures platform;
        private readonly IFileService fileService;
        private readonly IResourceService i18n;

        [ObservableProperty]
        private ObservableCollection<ApplicationModel> applications = [];

        [ObservableProperty]
        private FilteredCollectionView<ApplicationModel> applicationsFiltered;

        [ObservableProperty]
        private ApplicationModel selectedItem;

        public FindMoreAppsViewModel(IApplicationsFactory appFactory, IPlatformUiFeatures platform, IFileService fileService, IResourceService i18n)
        {
            this.appFactory = appFactory;
            this.platform = platform;
            this.fileService = fileService;
            this.i18n = i18n;

            ApplicationsFiltered = new FilteredCollectionView<ApplicationModel>(Applications, true);
            ApplicationsFiltered.SortDescriptions.Add(new SortDescription(nameof(ApplicationModel.AppName), SortDirection.Ascending));

            using (ApplicationsFiltered.DeferRefresh())
            {
                foreach (var app in platform.GetRunningApplications())
                {
                    if (app is not null)
                        Applications.Add(app);
                }
            }
            SelectedItem = Applications.FirstOrDefault();
        }

        private RelayCommand _browseCommand;
        public RelayCommand BrowseCommand => _browseCommand ??= new RelayCommand(async() => await BrowseApp());

        private async Task BrowseApp()
        {
            var files = await fileService.PickFileAsync([(i18n.GetString(WallpaperType.app), platform.ApplicationFileExtensions)]);
            if (files.Any())
            {
                var app = appFactory.CreateApp(files[0]);
                if (app is not null)
                {
                    Applications.Add(app);
                    SelectedItem = app;
                }
            }
        }
    }
}
