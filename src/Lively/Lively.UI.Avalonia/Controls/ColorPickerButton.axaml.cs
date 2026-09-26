using Avalonia;
using Avalonia.Controls;
using Avalonia.Data;
using Avalonia.Media;
using CommunityToolkit.Mvvm.Input;
using Lively.UI.Avalonia.Converters;
using Lively.UI.Avalonia.Services;
using Microsoft.Extensions.DependencyInjection;
using System;
using System.Threading.Tasks;
using System.Windows.Input;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Colour swatch that opens a colour picker flyout, plus a screen eyedropper backed by the XDG desktop portal.
    /// The colour is exposed as #RRGGBB text, the format of the wallpaper property files.
    /// </summary>
    public partial class ColorPickerButton : UserControl
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        public static readonly StyledProperty<string> SelectedColorProperty =
            AvaloniaProperty.Register<ColorPickerButton, string>(nameof(SelectedColor), "#FFC0CB", defaultBindingMode: BindingMode.TwoWay);

        public static readonly StyledProperty<Color> PickerColorProperty =
            AvaloniaProperty.Register<ColorPickerButton, Color>(nameof(PickerColor), HexStringToColorConverter.FallbackColor, defaultBindingMode: BindingMode.TwoWay);

        public static readonly StyledProperty<ICommand> ColorChangedCommandProperty =
            AvaloniaProperty.Register<ColorPickerButton, ICommand>(nameof(ColorChangedCommand));

        public static readonly StyledProperty<object> CommandParameterProperty =
            AvaloniaProperty.Register<ColorPickerButton, object>(nameof(CommandParameter));

        public static readonly DirectProperty<ColorPickerButton, IBrush> SelectedBrushProperty =
            AvaloniaProperty.RegisterDirect<ColorPickerButton, IBrush>(nameof(SelectedBrush), o => o.SelectedBrush);

        private IBrush selectedBrush;
        private bool isSyncing;

        public ColorPickerButton()
        {
            InitializeComponent();
            OpenEyeDropperCommand = new AsyncRelayCommand(OpenEyeDropperAsync);
            selectedBrush = new SolidColorBrush(HexStringToColorConverter.FallbackColor);
        }

        public string SelectedColor
        {
            get => GetValue(SelectedColorProperty);
            set => SetValue(SelectedColorProperty, value);
        }

        public Color PickerColor
        {
            get => GetValue(PickerColorProperty);
            set => SetValue(PickerColorProperty, value);
        }

        public ICommand ColorChangedCommand
        {
            get => GetValue(ColorChangedCommandProperty);
            set => SetValue(ColorChangedCommandProperty, value);
        }

        public object CommandParameter
        {
            get => GetValue(CommandParameterProperty);
            set => SetValue(CommandParameterProperty, value);
        }

        public IBrush SelectedBrush
        {
            get => selectedBrush;
            private set => SetAndRaise(SelectedBrushProperty, ref selectedBrush, value);
        }

        public AsyncRelayCommand OpenEyeDropperCommand { get; }

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);

            if (change.Property == SelectedColorProperty)
            {
                var color = Color.TryParse(SelectedColor, out var parsed) ? Color.FromRgb(parsed.R, parsed.G, parsed.B) : HexStringToColorConverter.FallbackColor;
                SelectedBrush = new SolidColorBrush(color);
                Sync(() => PickerColor = color);
                ColorChangedCommand?.Execute(CommandParameter);
            }
            else if (change.Property == PickerColorProperty)
            {
                Sync(() => SelectedColor = HexStringToColorConverter.ToHex(PickerColor));
            }
        }

        private void Sync(Action update)
        {
            if (isSyncing)
                return;

            isSyncing = true;
            try
            {
                update();
            }
            finally
            {
                isSyncing = false;
            }
        }

        private async Task OpenEyeDropperAsync()
        {
            try
            {
                var color = await App.Services.GetRequiredService<LinuxScreenColorPicker>().PickAsync();
                if (color is not null)
                    SelectedColor = HexStringToColorConverter.ToHex(color.Value);
            }
            catch (Exception ex)
            {
                Logger.Error($"Screen colour picker failed: {ex}");
            }
        }
    }
}
