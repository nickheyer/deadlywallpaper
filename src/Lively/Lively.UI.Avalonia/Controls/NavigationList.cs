using Avalonia;
using Avalonia.Controls;
using Lively.Models.UserControls;
using System;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Horizontal tab list of the main navigation. Items with a glyph are the main pages and are shown outside the
    /// settings pages; items without a glyph are the settings pages and are shown only while a settings page is open.
    /// </summary>
    public class NavigationList : ListBox
    {
        public static readonly StyledProperty<bool> IsSettingsPageProperty =
            AvaloniaProperty.Register<NavigationList, bool>(nameof(IsSettingsPage));

        public bool IsSettingsPage
        {
            get => GetValue(IsSettingsPageProperty);
            set => SetValue(IsSettingsPageProperty, value);
        }

        protected override Type StyleKeyOverride => typeof(ListBox);

        protected override void PrepareContainerForItemOverride(Control container, object item, int index)
        {
            base.PrepareContainerForItemOverride(container, item, index);
            UpdateVisibility(container, item);
        }

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);
            if (change.Property != IsSettingsPageProperty)
                return;

            foreach (var container in GetRealizedContainers())
                UpdateVisibility(container, container.DataContext);
        }

        private void UpdateVisibility(Control container, object item)
        {
            var isMainPage = item is MainNavigationItem menuItem && !string.IsNullOrEmpty(menuItem.Glyph);
            container.IsVisible = isMainPage != IsSettingsPage;
        }
    }
}
