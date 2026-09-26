using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using System;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Overlay that presents one <see cref="ContentDialog"/> at a time; requests are queued so dialogs never overlap.
    /// </summary>
    public partial class DialogHost : UserControl
    {
        private static DialogHost current;
        private readonly SemaphoreSlim gate = new SemaphoreSlim(1, 1);
        private ContentDialog activeDialog;

        public DialogHost()
        {
            InitializeComponent();
            AddHandler(KeyDownEvent, Host_KeyDown, RoutingStrategies.Tunnel);
        }

        /// <summary>
        /// Host of the active top-level window, registered by the window that owns it.
        /// </summary>
        public static DialogHost Current => current ?? throw new InvalidOperationException("No DialogHost is registered; the main window must be created before dialogs are shown.");

        public static void Register(DialogHost host) => current = host;

        /// <summary>
        /// True while a dialog is displayed or queued.
        /// </summary>
        public bool IsBusy => gate.CurrentCount == 0;

        public async Task<ContentDialogResult> ShowAsync(ContentDialog dialog)
        {
            if (dialog is null)
                throw new ArgumentNullException(nameof(dialog));

            await gate.WaitAsync();
            try
            {
                var closed = dialog.BeginShow();
                activeDialog = dialog;
                Presenter.Content = dialog;
                IsVisible = true;
                Focus();
                dialog.NotifyOpened();

                var result = await closed;

                Presenter.Content = null;
                activeDialog = null;
                IsVisible = false;
                dialog.NotifyClosed(result);
                return result;
            }
            finally
            {
                gate.Release();
            }
        }

        private void Host_KeyDown(object sender, KeyEventArgs e)
        {
            if (activeDialog is null)
                return;

            if (e.Key == Key.Escape)
            {
                activeDialog.RequestClose(ContentDialogResult.None);
                e.Handled = true;
            }
            else if (e.Key == Key.Enter && e.Source is not TextBox)
            {
                activeDialog.InvokeDefaultButton();
                e.Handled = true;
            }
        }

        private void Backdrop_PointerPressed(object sender, PointerPressedEventArgs e)
        {
            // Clicks on the dimmed backdrop stay inside the host so the page below is not interacted with.
            e.Handled = true;
        }
    }
}
