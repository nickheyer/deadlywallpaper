using Avalonia;
using Avalonia.Collections;
using Avalonia.Controls;
using Avalonia.Media;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// A settings card whose <see cref="Items"/> are revealed below it when expanded.
    /// <see cref="ContentControl.Content"/> is the action control shown on the header row.
    /// </summary>
    public class SettingsExpander : ContentControl
    {
        public static readonly StyledProperty<string> HeaderProperty =
            AvaloniaProperty.Register<SettingsExpander, string>(nameof(Header));

        public static readonly StyledProperty<object> DescriptionProperty =
            AvaloniaProperty.Register<SettingsExpander, object>(nameof(Description));

        public static readonly StyledProperty<Geometry> HeaderIconProperty =
            AvaloniaProperty.Register<SettingsExpander, Geometry>(nameof(HeaderIcon));

        public static readonly StyledProperty<bool> IsExpandedProperty =
            AvaloniaProperty.Register<SettingsExpander, bool>(nameof(IsExpanded), defaultBindingMode: global::Avalonia.Data.BindingMode.TwoWay);

        public static readonly DirectProperty<SettingsExpander, AvaloniaList<Control>> ItemsProperty =
            AvaloniaProperty.RegisterDirect<SettingsExpander, AvaloniaList<Control>>(nameof(Items), o => o.Items);

        public SettingsExpander()
        {
            Items = new AvaloniaList<Control>();
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

        public bool IsExpanded
        {
            get => GetValue(IsExpandedProperty);
            set => SetValue(IsExpandedProperty, value);
        }

        /// <summary>
        /// Rows shown inside the expanded area (usually <see cref="SettingsCard"/>s).
        /// </summary>
        public AvaloniaList<Control> Items { get; }
    }
}
