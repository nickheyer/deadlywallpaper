using Avalonia;
using Avalonia.Controls;
using Avalonia.Data.Converters;
using Avalonia.Media;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// Severity name to the icon geometry shown next to the update status.
    /// </summary>
    public class StringToInfoBarSeverityIconConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            var key = (value as string) switch
            {
                "Informational" => "IconInfo",
                "Success" => "IconSuccess",
                "Warning" => "IconWarning",
                "Error" => "IconError",
                _ => "IconError",
            };
            return Application.Current.FindResource(key) as Geometry;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
