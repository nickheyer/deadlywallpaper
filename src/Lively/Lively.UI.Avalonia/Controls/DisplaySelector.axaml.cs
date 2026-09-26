using Avalonia;
using Avalonia.Controls;
using Avalonia.Data;
using Avalonia.Input;
using Avalonia.Interactivity;
using Lively.Models;
using Lively.Models.Enums;
using System;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.Drawing;
using System.Linq;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Draws the monitors of <see cref="Displays"/> to scale on a canvas; in per-display layout a tile can be selected.
    /// Span and duplicate layouts render a presentation of overlapping / side-by-side displays instead.
    /// </summary>
    public partial class DisplaySelector : UserControl
    {
        public static readonly StyledProperty<ObservableCollection<ScreenLayoutModel>> DisplaysProperty =
            AvaloniaProperty.Register<DisplaySelector, ObservableCollection<ScreenLayoutModel>>(nameof(Displays));

        public static readonly StyledProperty<ScreenLayoutModel> SelectedItemProperty =
            AvaloniaProperty.Register<DisplaySelector, ScreenLayoutModel>(nameof(SelectedItem), defaultBindingMode: BindingMode.TwoWay);

        public static readonly StyledProperty<bool> IsSelectionProperty =
            AvaloniaProperty.Register<DisplaySelector, bool>(nameof(IsSelection), true);

        public static readonly StyledProperty<WallpaperArrangement> LayoutProperty =
            AvaloniaProperty.Register<DisplaySelector, WallpaperArrangement>(nameof(Layout), WallpaperArrangement.per);

        public static readonly DirectProperty<DisplaySelector, bool> IsSelectableProperty =
            AvaloniaProperty.RegisterDirect<DisplaySelector, bool>(nameof(IsSelectable), o => o.IsSelectable);

        private ObservableCollection<ScreenLayoutModel> subscribedDisplays;

        public DisplaySelector()
        {
            InitializeComponent();
        }

        public ObservableCollection<ScreenLayoutModel> Displays
        {
            get => GetValue(DisplaysProperty);
            set => SetValue(DisplaysProperty, value);
        }

        public ScreenLayoutModel SelectedItem
        {
            get => GetValue(SelectedItemProperty);
            set => SetValue(SelectedItemProperty, value);
        }

        public bool IsSelection
        {
            get => GetValue(IsSelectionProperty);
            set => SetValue(IsSelectionProperty, value);
        }

        public WallpaperArrangement Layout
        {
            get => GetValue(LayoutProperty);
            set => SetValue(LayoutProperty, value);
        }

        /// <summary>
        /// True while the user can pick a display: selection is enabled and the layout is per-display.
        /// </summary>
        public bool IsSelectable => IsSelection && Layout == WallpaperArrangement.per;

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);

            if (change.Property == DisplaysProperty)
            {
                if (subscribedDisplays != null)
                    subscribedDisplays.CollectionChanged -= Displays_CollectionChanged;
                subscribedDisplays = change.GetNewValue<ObservableCollection<ScreenLayoutModel>>();
                if (subscribedDisplays != null)
                    subscribedDisplays.CollectionChanged += Displays_CollectionChanged;

                UpdateCanvas();
                UpdateDisplaySelection();
            }
            else if (change.Property == SelectedItemProperty)
            {
                UpdateDisplaySelection();
            }
            else if (change.Property == LayoutProperty)
            {
                RaisePropertyChanged(IsSelectableProperty, !IsSelectable, IsSelectable);
                UpdateCanvas();
                UpdateDisplaySelection();
            }
            else if (change.Property == IsSelectionProperty)
            {
                RaisePropertyChanged(IsSelectableProperty, !IsSelectable, IsSelectable);
            }
            else if (change.Property == IsEnabledProperty)
            {
                Opacity = IsEnabled ? 1.0 : 0.25;
            }
            else if (change.Property == BoundsProperty)
            {
                UpdateCanvas();
            }
        }

        protected override void OnLoaded(RoutedEventArgs e)
        {
            base.OnLoaded(e);
            UpdateCanvas();
        }

        protected override void OnUnloaded(RoutedEventArgs e)
        {
            base.OnUnloaded(e);
            if (subscribedDisplays != null)
                subscribedDisplays.CollectionChanged -= Displays_CollectionChanged;
            subscribedDisplays = null;
        }

        private void UpdateCanvas()
        {
            // Bounds are only available once laid out.
            var displays = Displays;
            if (displays is null || displays.Count == 0 || Bounds.Width <= 0 || Bounds.Height <= 0)
                return;

            var width = Bounds.Width;
            var height = Bounds.Height;
            switch (displays.Count > 1 ? Layout : WallpaperArrangement.per)
            {
                case WallpaperArrangement.per:
                    {
                        var totalBounds = new Rectangle();
                        foreach (var item in displays)
                            totalBounds = Rectangle.Union(totalBounds, item.Screen.Bounds);

                        // Worst case factor + margin
                        var factor = Math.Max(totalBounds.Height / height, totalBounds.Width / width) + 2;
                        foreach (var item in displays)
                        {
                            item.NormalizedBounds = new Rectangle((int)(item.Screen.Bounds.Left / factor),
                                (int)(item.Screen.Bounds.Top / factor),
                                (int)(item.Screen.Bounds.Width / factor),
                                (int)(item.Screen.Bounds.Height / factor));
                        }
                    }
                    break;
                case WallpaperArrangement.duplicate:
                    {
                        // Presentation of overlapped displays.
                        LayoutPresentation(displays, width, height, 150, 150);
                    }
                    break;
                case WallpaperArrangement.span:
                    {
                        // Presentation of displays side by side.
                        LayoutPresentation(displays, width, height, 1920 / 2, 0);
                    }
                    break;
                default:
                    throw new ArgumentOutOfRangeException(nameof(Layout), Layout, "Unknown wallpaper arrangement.");
            }

            // Bounds.Left and Right can be negative
            int minLeft = displays.Min(item => item.NormalizedBounds.Left);
            int maxRight = displays.Max(item => item.NormalizedBounds.Left + item.NormalizedBounds.Width);
            int minTop = displays.Min(item => item.NormalizedBounds.Top);
            int maxBottom = displays.Max(item => item.NormalizedBounds.Top + item.NormalizedBounds.Height);

            // Center to canvas
            double horizontalOffset = (maxRight + minLeft) / 2 - width / 2;
            double verticalOffset = (maxBottom + minTop) / 2 - height / 2;

            foreach (var item in displays)
            {
                item.NormalizedBounds = new Rectangle(
                    (int)(item.NormalizedBounds.Left - horizontalOffset),
                    (int)(item.NormalizedBounds.Top - verticalOffset),
                    item.NormalizedBounds.Width,
                    item.NormalizedBounds.Height);
            }
        }

        private static void LayoutPresentation(ObservableCollection<ScreenLayoutModel> displays, double width, double height, int offsetX, int offsetY)
        {
            const int sampleWidth = 1920;
            const int sampleHeight = 1080;
            var totalBounds = new Rectangle();
            for (int i = 0; i < displays.Count; i++)
                totalBounds = Rectangle.Union(totalBounds, new Rectangle(offsetX * i, offsetY * i, sampleWidth, sampleHeight));

            var factor = Math.Max(totalBounds.Height / height, totalBounds.Width / width) + 2;
            for (int i = 0; i < displays.Count; i++)
            {
                displays[i].NormalizedBounds = new Rectangle((int)(offsetX * i / factor),
                    (int)(offsetY * i / factor),
                    (int)(sampleWidth / factor),
                    (int)(sampleHeight / factor));
            }
        }

        private void UpdateDisplaySelection()
        {
            var displays = Displays;
            if (displays is null)
                return;

            // Only visual change
            foreach (var item in displays)
                item.IsSelected = Layout != WallpaperArrangement.per || item == SelectedItem;
        }

        private void Displays_CollectionChanged(object sender, NotifyCollectionChangedEventArgs e)
        {
            UpdateCanvas();
            UpdateDisplaySelection();
        }

        private void Tile_PointerPressed(object sender, PointerPressedEventArgs e)
        {
            if (!IsSelectable || !e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
                return;

            if (sender is Control element && element.DataContext is ScreenLayoutModel screenLayoutModel)
                SelectedItem = screenLayoutModel;
        }
    }
}
