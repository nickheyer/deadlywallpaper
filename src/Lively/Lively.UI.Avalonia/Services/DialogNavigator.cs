using Avalonia.Controls;
using Lively.Common.Services;
using Lively.Models.Enums;
using Lively.UI.Avalonia.Views.ControlPanel;
using Microsoft.Extensions.DependencyInjection;
using System;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Navigates the pages of the control panel dialog. Registered as scoped so each dialog gets its own instance
    /// resolving view models from the same scope.
    /// </summary>
    public class DialogNavigator : NavigatorBase<DialogPageType>, IDialogNavigator
    {
        private readonly IServiceProvider services;

        public DialogNavigator(IServiceProvider services)
        {
            this.services = services;
        }

        protected override Control CreateView(DialogPageType contentPage)
        {
            return contentPage switch
            {
                DialogPageType.controlPanelWallpaper => ActivatorUtilities.CreateInstance<WallpaperLayoutView>(services),
                DialogPageType.controlPanelScreensaver => ActivatorUtilities.CreateInstance<ScreensaverLayoutView>(services),
                DialogPageType.controlPanelCustomise => ActivatorUtilities.CreateInstance<WallpaperLayoutCustomiseView>(services),
                _ => throw new ArgumentOutOfRangeException(nameof(contentPage), contentPage, "Unknown dialog page."),
            };
        }
    }
}
