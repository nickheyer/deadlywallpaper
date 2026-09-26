using Avalonia;
using Avalonia.Controls;
using Avalonia.Data.Converters;
using Avalonia.Media;
using System;
using System.Globalization;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// Volume (0-100) to one of the five speaker icon geometries.
    /// </summary>
    public class AudioVolumeGlyphConverter : IValueConverter
    {
        private static readonly string[] audioIcons = ["IconVolume0", "IconVolume1", "IconVolume2", "IconVolume3", "IconVolume4"];

        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            double volume = value switch
            {
                double d => d,
                int i => i,
                float f => f,
                _ => 0,
            };
            var index = (int)Math.Ceiling((audioIcons.Length - 1) * Math.Clamp(volume, 0, 100) / 100);
            return Application.Current.FindResource(audioIcons[index]) as Geometry;
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();
    }
}
