using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.VisualTree;
using System;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Tile grid of the library. <see cref="SelectionModeName"/> mirrors the view model's "Single"/"None" selection mode:
    /// with "None" the tiles never select and every click is reported through <see cref="ItemClicked"/> instead.
    /// </summary>
    public class WallpaperGrid : ListBox
    {
        public static readonly StyledProperty<string> SelectionModeNameProperty =
            AvaloniaProperty.Register<WallpaperGrid, string>(nameof(SelectionModeName), "Single");

        /// <summary>
        /// Raised with the clicked item's data when a tile is clicked with the left button (buttons on the tile excluded).
        /// </summary>
        public event EventHandler<object> ItemClicked;

        public string SelectionModeName
        {
            get => GetValue(SelectionModeNameProperty);
            set => SetValue(SelectionModeNameProperty, value);
        }

        public bool IsSelectionEnabled => !string.Equals(SelectionModeName, "None", StringComparison.OrdinalIgnoreCase);

        protected override Type StyleKeyOverride => typeof(ListBox);

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);
            if (change.Property == SelectionModeNameProperty && !IsSelectionEnabled)
                SelectedItem = null;
        }

        protected override void OnPointerPressed(PointerPressedEventArgs e)
        {
            // The base class selects the pressed tile; with selection disabled the click only raises ItemClicked.
            if (IsSelectionEnabled)
                base.OnPointerPressed(e);
        }

        protected override void OnPointerReleased(PointerReleasedEventArgs e)
        {
            base.OnPointerReleased(e);
            if (e.InitialPressMouseButton != MouseButton.Left || e.Source is not Visual source)
                return;

            var button = source.FindAncestorOfType<Button>(true);
            if (button != null && this.IsVisualAncestorOf(button))
                return;

            var container = source.FindAncestorOfType<ListBoxItem>(true);
            if (container != null && this.IsVisualAncestorOf(container))
                ItemClicked?.Invoke(this, container.DataContext);
        }
    }
}
