using Avalonia.Controls;
using Avalonia.Interactivity;
using Lively.Common.Services;
using Lively.Models.Enums;
using Lively.UI.Shared.ViewModels;

namespace Lively.UI.Avalonia.Views.ControlPanel
{
    public partial class ControlPanelView : UserControl
    {
        private readonly IDialogNavigator dialogNavigator;

        public ControlPanelView(ControlPanelViewModel viewModel, IDialogNavigator dialogNavigator)
        {
            this.dialogNavigator = dialogNavigator;
            InitializeComponent();
            DataContext = viewModel;

            dialogNavigator.RootFrame = this;
            dialogNavigator.Frame = ContentFrame;
        }

        protected override void OnLoaded(RoutedEventArgs e)
        {
            base.OnLoaded(e);
            if (dialogNavigator.CurrentPage == null)
                dialogNavigator.NavigateTo(DialogPageType.controlPanelWallpaper);
        }
    }
}
