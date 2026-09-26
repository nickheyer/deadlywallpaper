using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Metadata;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Interactivity;
using System;
using Avalonia.Media;
using System.Windows.Input;

namespace Lively.UI.Avalonia.Controls
{
    public enum SettingsContentAlignment
    {
        /// <summary>Content sits at the right edge of the card.</summary>
        Right,
        /// <summary>Content is stacked under the header, aligned left.</summary>
        Left,
        /// <summary>Content is stacked under the header and stretched.</summary>
        Vertical
    }

    /// <summary>
    /// A settings row with header icon, header, description and action content; optionally clickable like a button.
    /// </summary>
    [PseudoClasses(":clickable", ":pressed", ":vertical")]
    public class SettingsCard : ContentControl
    {
        public static readonly StyledProperty<string> HeaderProperty =
            AvaloniaProperty.Register<SettingsCard, string>(nameof(Header));

        public static readonly StyledProperty<object> DescriptionProperty =
            AvaloniaProperty.Register<SettingsCard, object>(nameof(Description));

        public static readonly StyledProperty<Geometry> HeaderIconProperty =
            AvaloniaProperty.Register<SettingsCard, Geometry>(nameof(HeaderIcon));

        public static readonly StyledProperty<Geometry> ActionIconProperty =
            AvaloniaProperty.Register<SettingsCard, Geometry>(nameof(ActionIcon));

        public static readonly StyledProperty<bool> IsClickEnabledProperty =
            AvaloniaProperty.Register<SettingsCard, bool>(nameof(IsClickEnabled));

        public static readonly StyledProperty<ICommand> CommandProperty =
            AvaloniaProperty.Register<SettingsCard, ICommand>(nameof(Command));

        public static readonly StyledProperty<object> CommandParameterProperty =
            AvaloniaProperty.Register<SettingsCard, object>(nameof(CommandParameter));

        public static readonly StyledProperty<SettingsContentAlignment> ContentAlignmentProperty =
            AvaloniaProperty.Register<SettingsCard, SettingsContentAlignment>(nameof(ContentAlignment), SettingsContentAlignment.Right);

        public static readonly RoutedEvent<RoutedEventArgs> ClickEvent =
            RoutedEvent.Register<SettingsCard, RoutedEventArgs>(nameof(Click), RoutingStrategies.Bubble);

        static SettingsCard()
        {
            IsClickEnabledProperty.Changed.AddClassHandler<SettingsCard>((card, _) => card.UpdatePseudoClasses());
            ContentAlignmentProperty.Changed.AddClassHandler<SettingsCard>((card, _) => card.UpdatePseudoClasses());
            FocusableProperty.OverrideDefaultValue<SettingsCard>(false);
        }

        public event EventHandler<RoutedEventArgs> Click
        {
            add => AddHandler(ClickEvent, value);
            remove => RemoveHandler(ClickEvent, value);
        }

        public string Header
        {
            get => GetValue(HeaderProperty);
            set => SetValue(HeaderProperty, value);
        }

        public object Description
        {
            get => GetValue(DescriptionProperty);
            set => SetValue(DescriptionProperty, value);
        }

        public Geometry HeaderIcon
        {
            get => GetValue(HeaderIconProperty);
            set => SetValue(HeaderIconProperty, value);
        }

        public Geometry ActionIcon
        {
            get => GetValue(ActionIconProperty);
            set => SetValue(ActionIconProperty, value);
        }

        public bool IsClickEnabled
        {
            get => GetValue(IsClickEnabledProperty);
            set => SetValue(IsClickEnabledProperty, value);
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

        public SettingsContentAlignment ContentAlignment
        {
            get => GetValue(ContentAlignmentProperty);
            set => SetValue(ContentAlignmentProperty, value);
        }

        protected override void OnApplyTemplate(TemplateAppliedEventArgs e)
        {
            base.OnApplyTemplate(e);
            UpdatePseudoClasses();
        }

        protected override void OnPointerPressed(PointerPressedEventArgs e)
        {
            base.OnPointerPressed(e);
            if (IsClickEnabled && e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
                PseudoClasses.Set(":pressed", true);
        }

        protected override void OnPointerReleased(PointerReleasedEventArgs e)
        {
            base.OnPointerReleased(e);
            var wasPressed = PseudoClasses.Contains(":pressed");
            PseudoClasses.Set(":pressed", false);
            if (wasPressed && IsClickEnabled && e.InitialPressMouseButton == MouseButton.Left)
                RaiseClick();
        }

        protected override void OnPointerCaptureLost(PointerCaptureLostEventArgs e)
        {
            base.OnPointerCaptureLost(e);
            PseudoClasses.Set(":pressed", false);
        }

        protected override void OnKeyDown(KeyEventArgs e)
        {
            base.OnKeyDown(e);
            if (IsClickEnabled && (e.Key == Key.Enter || e.Key == Key.Space))
            {
                RaiseClick();
                e.Handled = true;
            }
        }

        private void RaiseClick()
        {
            var args = new RoutedEventArgs(ClickEvent);
            RaiseEvent(args);
            var command = Command;
            if (command != null && command.CanExecute(CommandParameter))
                command.Execute(CommandParameter);
        }

        private void UpdatePseudoClasses()
        {
            PseudoClasses.Set(":clickable", IsClickEnabled);
            PseudoClasses.Set(":vertical", ContentAlignment != SettingsContentAlignment.Right);
            Focusable = IsClickEnabled;
        }
    }
}
