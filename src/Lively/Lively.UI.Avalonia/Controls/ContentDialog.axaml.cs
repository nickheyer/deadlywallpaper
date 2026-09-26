using Avalonia;
using Avalonia.Controls;
using Avalonia.Interactivity;
using System;
using System.Threading.Tasks;
using System.Windows.Input;

namespace Lively.UI.Avalonia.Controls
{
    public enum ContentDialogResult
    {
        None,
        Primary,
        Secondary
    }

    public enum ContentDialogButton
    {
        None,
        Primary,
        Secondary,
        Close
    }

    public sealed class ContentDialogClosingEventArgs : EventArgs
    {
        public ContentDialogClosingEventArgs(ContentDialogResult result)
        {
            Result = result;
        }

        public ContentDialogResult Result { get; }

        public bool Cancel { get; set; }
    }

    /// <summary>
    /// Modal dialog surface shown inside the window's <see cref="DialogHost"/>; mirrors the WinUI ContentDialog API used by the dialog service.
    /// </summary>
    public partial class ContentDialog : UserControl
    {
        public static readonly StyledProperty<string> TitleProperty =
            AvaloniaProperty.Register<ContentDialog, string>(nameof(Title));

        public static readonly StyledProperty<object> DialogContentProperty =
            AvaloniaProperty.Register<ContentDialog, object>(nameof(DialogContent));

        public static readonly StyledProperty<string> PrimaryButtonTextProperty =
            AvaloniaProperty.Register<ContentDialog, string>(nameof(PrimaryButtonText));

        public static readonly StyledProperty<string> SecondaryButtonTextProperty =
            AvaloniaProperty.Register<ContentDialog, string>(nameof(SecondaryButtonText));

        public static readonly StyledProperty<string> CloseButtonTextProperty =
            AvaloniaProperty.Register<ContentDialog, string>(nameof(CloseButtonText));

        public static readonly StyledProperty<ContentDialogButton> DefaultButtonProperty =
            AvaloniaProperty.Register<ContentDialog, ContentDialogButton>(nameof(DefaultButton), ContentDialogButton.None);

        public static readonly StyledProperty<bool> IsPrimaryButtonEnabledProperty =
            AvaloniaProperty.Register<ContentDialog, bool>(nameof(IsPrimaryButtonEnabled), true);

        public static readonly StyledProperty<bool> IsSecondaryButtonEnabledProperty =
            AvaloniaProperty.Register<ContentDialog, bool>(nameof(IsSecondaryButtonEnabled), true);

        public static readonly StyledProperty<ICommand> PrimaryButtonCommandProperty =
            AvaloniaProperty.Register<ContentDialog, ICommand>(nameof(PrimaryButtonCommand));

        public static readonly StyledProperty<ICommand> SecondaryButtonCommandProperty =
            AvaloniaProperty.Register<ContentDialog, ICommand>(nameof(SecondaryButtonCommand));

        public static readonly StyledProperty<double> MaxDialogWidthProperty =
            AvaloniaProperty.Register<ContentDialog, double>(nameof(MaxDialogWidth), 760);

        public static readonly StyledProperty<double> MaxDialogHeightProperty =
            AvaloniaProperty.Register<ContentDialog, double>(nameof(MaxDialogHeight), 820);

        public static readonly DirectProperty<ContentDialog, bool> HasButtonsProperty =
            AvaloniaProperty.RegisterDirect<ContentDialog, bool>(nameof(HasButtons), o => o.HasButtons);

        private TaskCompletionSource<ContentDialogResult> completion;

        public ContentDialog()
        {
            InitializeComponent();
        }

        public event EventHandler Opened;
        public event EventHandler<ContentDialogClosingEventArgs> Closing;
        public event EventHandler<ContentDialogResult> Closed;

        public string Title
        {
            get => GetValue(TitleProperty);
            set => SetValue(TitleProperty, value);
        }

        public object DialogContent
        {
            get => GetValue(DialogContentProperty);
            set => SetValue(DialogContentProperty, value);
        }

        public string PrimaryButtonText
        {
            get => GetValue(PrimaryButtonTextProperty);
            set => SetValue(PrimaryButtonTextProperty, value);
        }

        public string SecondaryButtonText
        {
            get => GetValue(SecondaryButtonTextProperty);
            set => SetValue(SecondaryButtonTextProperty, value);
        }

        public string CloseButtonText
        {
            get => GetValue(CloseButtonTextProperty);
            set => SetValue(CloseButtonTextProperty, value);
        }

        public ContentDialogButton DefaultButton
        {
            get => GetValue(DefaultButtonProperty);
            set => SetValue(DefaultButtonProperty, value);
        }

        public bool IsPrimaryButtonEnabled
        {
            get => GetValue(IsPrimaryButtonEnabledProperty);
            set => SetValue(IsPrimaryButtonEnabledProperty, value);
        }

        public bool IsSecondaryButtonEnabled
        {
            get => GetValue(IsSecondaryButtonEnabledProperty);
            set => SetValue(IsSecondaryButtonEnabledProperty, value);
        }

        public ICommand PrimaryButtonCommand
        {
            get => GetValue(PrimaryButtonCommandProperty);
            set => SetValue(PrimaryButtonCommandProperty, value);
        }

        public ICommand SecondaryButtonCommand
        {
            get => GetValue(SecondaryButtonCommandProperty);
            set => SetValue(SecondaryButtonCommandProperty, value);
        }

        public double MaxDialogWidth
        {
            get => GetValue(MaxDialogWidthProperty);
            set => SetValue(MaxDialogWidthProperty, value);
        }

        public double MaxDialogHeight
        {
            get => GetValue(MaxDialogHeightProperty);
            set => SetValue(MaxDialogHeightProperty, value);
        }

        public bool HasButtons => !string.IsNullOrEmpty(PrimaryButtonText) || !string.IsNullOrEmpty(SecondaryButtonText) || !string.IsNullOrEmpty(CloseButtonText);

        /// <summary>
        /// True while the dialog is displayed by a host.
        /// </summary>
        public bool IsOpen => completion != null && !completion.Task.IsCompleted;

        /// <summary>
        /// Shows the dialog in the window's dialog host and completes when it is closed.
        /// </summary>
        public Task<ContentDialogResult> ShowAsync() => DialogHost.Current.ShowAsync(this);

        /// <summary>
        /// Closes the dialog with <see cref="ContentDialogResult.None"/> unless a Closing handler cancels.
        /// </summary>
        public void Hide() => RequestClose(ContentDialogResult.None);

        /// <summary>
        /// Closes the dialog with the given result unless a Closing handler cancels.
        /// </summary>
        public void RequestClose(ContentDialogResult result)
        {
            if (!IsOpen)
                return;

            var args = new ContentDialogClosingEventArgs(result);
            Closing?.Invoke(this, args);
            if (args.Cancel)
                return;

            completion.TrySetResult(result);
        }

        internal Task<ContentDialogResult> BeginShow()
        {
            completion = new TaskCompletionSource<ContentDialogResult>(TaskCreationOptions.RunContinuationsAsynchronously);
            return completion.Task;
        }

        internal void NotifyOpened()
        {
            UpdateDefaultButtonClasses();
            FocusDefaultButton();
            Opened?.Invoke(this, EventArgs.Empty);
        }

        internal void NotifyClosed(ContentDialogResult result)
        {
            Closed?.Invoke(this, result);
        }

        /// <summary>
        /// Activates the default button (Enter key).
        /// </summary>
        internal void InvokeDefaultButton()
        {
            switch (DefaultButton)
            {
                case ContentDialogButton.Primary when IsPrimaryButtonEnabled && !string.IsNullOrEmpty(PrimaryButtonText):
                    PrimaryButton_Click(this, new RoutedEventArgs());
                    break;
                case ContentDialogButton.Secondary when IsSecondaryButtonEnabled && !string.IsNullOrEmpty(SecondaryButtonText):
                    SecondaryButton_Click(this, new RoutedEventArgs());
                    break;
                case ContentDialogButton.Close when !string.IsNullOrEmpty(CloseButtonText):
                    CloseButton_Click(this, new RoutedEventArgs());
                    break;
            }
        }

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);
            if (change.Property == PrimaryButtonTextProperty || change.Property == SecondaryButtonTextProperty || change.Property == CloseButtonTextProperty)
                RaisePropertyChanged(HasButtonsProperty, !HasButtons, HasButtons);
            else if (change.Property == DefaultButtonProperty)
                UpdateDefaultButtonClasses();
        }

        private void UpdateDefaultButtonClasses()
        {
            PrimaryButton.Classes.Set("accent", DefaultButton == ContentDialogButton.Primary);
            SecondaryButton.Classes.Set("accent", DefaultButton == ContentDialogButton.Secondary);
            CloseButton.Classes.Set("accent", DefaultButton == ContentDialogButton.Close);
        }

        private void FocusDefaultButton()
        {
            var target = DefaultButton switch
            {
                ContentDialogButton.Primary => PrimaryButton,
                ContentDialogButton.Secondary => SecondaryButton,
                ContentDialogButton.Close => CloseButton,
                _ => null,
            };
            if (target != null && target.IsVisible)
                target.Focus();
        }

        private void PrimaryButton_Click(object sender, RoutedEventArgs e)
        {
            var command = PrimaryButtonCommand;
            if (command != null && command.CanExecute(null))
                command.Execute(null);
            RequestClose(ContentDialogResult.Primary);
        }

        private void SecondaryButton_Click(object sender, RoutedEventArgs e)
        {
            var command = SecondaryButtonCommand;
            if (command != null && command.CanExecute(null))
                command.Execute(null);
            RequestClose(ContentDialogResult.Secondary);
        }

        private void CloseButton_Click(object sender, RoutedEventArgs e)
        {
            RequestClose(ContentDialogResult.None);
        }
    }
}
