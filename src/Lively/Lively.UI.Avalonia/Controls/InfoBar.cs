using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Metadata;
using Avalonia.Controls.Primitives;
using Avalonia.Interactivity;
using System;
using System.Windows.Input;

namespace Lively.UI.Avalonia.Controls
{
    public enum InfoBarSeverity
    {
        Informational,
        Success,
        Warning,
        Error
    }

    /// <summary>
    /// Inline notification with severity icon, title, message, an optional action button and extra content.
    /// </summary>
    [PseudoClasses(":informational", ":success", ":warning", ":error")]
    [TemplatePart("PART_CloseButton", typeof(Button))]
    public class InfoBar : ContentControl
    {
        public static readonly StyledProperty<string> TitleProperty =
            AvaloniaProperty.Register<InfoBar, string>(nameof(Title));

        public static readonly StyledProperty<string> MessageProperty =
            AvaloniaProperty.Register<InfoBar, string>(nameof(Message));

        public static readonly StyledProperty<InfoBarSeverity> SeverityProperty =
            AvaloniaProperty.Register<InfoBar, InfoBarSeverity>(nameof(Severity), InfoBarSeverity.Informational);

        public static readonly StyledProperty<bool> IsOpenProperty =
            AvaloniaProperty.Register<InfoBar, bool>(nameof(IsOpen), true, defaultBindingMode: global::Avalonia.Data.BindingMode.TwoWay);

        public static readonly StyledProperty<bool> IsClosableProperty =
            AvaloniaProperty.Register<InfoBar, bool>(nameof(IsClosable), true);

        public static readonly StyledProperty<bool> IsIconVisibleProperty =
            AvaloniaProperty.Register<InfoBar, bool>(nameof(IsIconVisible), true);

        public static readonly StyledProperty<object> ActionButtonProperty =
            AvaloniaProperty.Register<InfoBar, object>(nameof(ActionButton));

        public static readonly StyledProperty<ICommand> CloseButtonCommandProperty =
            AvaloniaProperty.Register<InfoBar, ICommand>(nameof(CloseButtonCommand));

        public static readonly RoutedEvent<RoutedEventArgs> ClosedEvent =
            RoutedEvent.Register<InfoBar, RoutedEventArgs>(nameof(Closed), RoutingStrategies.Bubble);

        private Button closeButton;

        static InfoBar()
        {
            SeverityProperty.Changed.AddClassHandler<InfoBar>((bar, _) => bar.UpdatePseudoClasses());
            IsOpenProperty.Changed.AddClassHandler<InfoBar>((bar, args) => bar.IsVisible = (bool)args.NewValue);
        }

        public InfoBar()
        {
            UpdatePseudoClasses();
        }

        public event EventHandler<RoutedEventArgs> Closed
        {
            add => AddHandler(ClosedEvent, value);
            remove => RemoveHandler(ClosedEvent, value);
        }

        public string Title
        {
            get => GetValue(TitleProperty);
            set => SetValue(TitleProperty, value);
        }

        public string Message
        {
            get => GetValue(MessageProperty);
            set => SetValue(MessageProperty, value);
        }

        public InfoBarSeverity Severity
        {
            get => GetValue(SeverityProperty);
            set => SetValue(SeverityProperty, value);
        }

        public bool IsOpen
        {
            get => GetValue(IsOpenProperty);
            set => SetValue(IsOpenProperty, value);
        }

        public bool IsClosable
        {
            get => GetValue(IsClosableProperty);
            set => SetValue(IsClosableProperty, value);
        }

        public bool IsIconVisible
        {
            get => GetValue(IsIconVisibleProperty);
            set => SetValue(IsIconVisibleProperty, value);
        }

        public object ActionButton
        {
            get => GetValue(ActionButtonProperty);
            set => SetValue(ActionButtonProperty, value);
        }

        public ICommand CloseButtonCommand
        {
            get => GetValue(CloseButtonCommandProperty);
            set => SetValue(CloseButtonCommandProperty, value);
        }

        protected override void OnApplyTemplate(TemplateAppliedEventArgs e)
        {
            base.OnApplyTemplate(e);
            if (closeButton != null)
                closeButton.Click -= CloseButton_Click;

            closeButton = e.NameScope.Find<Button>("PART_CloseButton");
            if (closeButton != null)
                closeButton.Click += CloseButton_Click;

            IsVisible = IsOpen;
        }

        private void CloseButton_Click(object sender, RoutedEventArgs e)
        {
            var command = CloseButtonCommand;
            if (command != null && command.CanExecute(null))
                command.Execute(null);

            IsOpen = false;
            RaiseEvent(new RoutedEventArgs(ClosedEvent));
        }

        private void UpdatePseudoClasses()
        {
            PseudoClasses.Set(":informational", Severity == InfoBarSeverity.Informational);
            PseudoClasses.Set(":success", Severity == InfoBarSeverity.Success);
            PseudoClasses.Set(":warning", Severity == InfoBarSeverity.Warning);
            PseudoClasses.Set(":error", Severity == InfoBarSeverity.Error);
        }
    }
}
