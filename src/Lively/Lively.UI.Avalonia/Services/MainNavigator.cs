using Avalonia.Controls;
using Lively.Common.Services;
using Lively.Models.Enums;
using Lively.UI.Avalonia.Views;
using Lively.UI.Avalonia.Views.Gallery;
using Lively.UI.Avalonia.Views.Settings;
using Microsoft.Extensions.DependencyInjection;
using System;

namespace Lively.UI.Avalonia.Services
{
    public class MainNavigator : NavigatorBase<ContentPageType>, IMainNavigator
    {
        private readonly IServiceProvider services;

        public MainNavigator(IServiceProvider services)
        {
            this.services = services;
        }

        protected override Control CreateView(ContentPageType contentPage)
        {
            return contentPage switch
            {
                ContentPageType.library => ActivatorUtilities.CreateInstance<LibraryView>(services),
                ContentPageType.gallery => ActivatorUtilities.CreateInstance<GalleryView>(services),
                ContentPageType.appupdate => ActivatorUtilities.CreateInstance<AppUpdateView>(services),
                ContentPageType.settingsGeneral => ActivatorUtilities.CreateInstance<SettingsGeneralView>(services),
                ContentPageType.settingsPerformance => ActivatorUtilities.CreateInstance<SettingsPerformanceView>(services),
                ContentPageType.settingsScreensaver => ActivatorUtilities.CreateInstance<SettingsScreensaverView>(services),
                ContentPageType.settingsWallpaper => ActivatorUtilities.CreateInstance<SettingsWallpaperView>(services),
                ContentPageType.settingsSystem => ActivatorUtilities.CreateInstance<SettingsSystemView>(services),
                _ => throw new ArgumentOutOfRangeException(nameof(contentPage), contentPage, "Unknown content page."),
            };
        }
    }
}
