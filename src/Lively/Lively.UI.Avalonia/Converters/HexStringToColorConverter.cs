using Avalonia.Data.Converters;
using Avalonia.Media;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// #RRGGBB text to <see cref="Color"/> and back (alpha is dropped, matching the wallpaper property format).
    /// </summary>
    public class HexStringToColorConverter : IValueConverter
    {
        public static readonly Color FallbackColor = Color.Parse("#FFC0CB");

        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            return value is string hex && Color.TryParse(hex, out var color) ? Color.FromRgb(color.R, color.G, color.B) : FallbackColor;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture)
        {
            return value is Color color ? ToHex(color) : ToHex(FallbackColor);
        }

        public static string ToHex(Color color) => $"#{color.R:X2}{color.G:X2}{color.B:X2}";
    }
}
