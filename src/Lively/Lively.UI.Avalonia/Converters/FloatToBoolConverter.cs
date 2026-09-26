using Avalonia.Data.Converters;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// True while a download progress value is still zero (progress ring shows indeterminate).
    /// </summary>
    public class FloatToBoolConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            return value is float val && val == 0;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
