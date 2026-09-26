using Avalonia;
using Avalonia.Controls;
using Avalonia.Data.Converters;
using Avalonia.Media;
using Lively.Models.Enums;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// Icon of a main navigation page; the settings pages have none, matching the WinUI menu (no glyph).
    /// </summary>
    public class PageTypeToIconConverter : IValueConverter
    {
        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            var key = value switch
            {
                ContentPageType.library => "IconLibrary",
                ContentPageType.gallery => "IconGallery",
                ContentPageType.appupdate => "IconUpdate",
                _ => null,
            };
            return key is null ? null : Application.Current.FindResource(key) as Geometry;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
