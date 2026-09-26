using Avalonia.Data.Converters;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// True when the enum value's name equals the converter parameter.
    /// </summary>
    public class EnumEqualsConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            return value is Enum e && string.Equals(e.ToString(), parameter as string, StringComparison.Ordinal);
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
