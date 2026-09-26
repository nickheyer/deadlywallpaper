using Avalonia.Data.Converters;
using Lively.Models.Enums;
using Lively.UI.Avalonia.Localization;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    public sealed class WallpaperTypeEnumToStringConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            return value is WallpaperType type ? LocalizationSource.Current.GetString(type) : string.Empty;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
