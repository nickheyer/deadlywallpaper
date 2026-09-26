using Avalonia.Data.Converters;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// Formats the value with the converter parameter as a composite format string, e.g. parameter "({0})".
    /// </summary>
    public class StringFormatConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            if (value is null)
                return string.Empty;

            return parameter is string format && !string.IsNullOrEmpty(format)
                ? string.Format(culture, format, value)
                : value.ToString();
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
