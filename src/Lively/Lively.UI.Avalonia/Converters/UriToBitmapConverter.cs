using Avalonia.Data.Converters;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// Loads local images for bindings: file paths, file:// and avares:// URIs, and the WinUI ms-appx:///Assets/ form
    /// which maps onto this project's embedded assets. Remote (http) images are loaded by <see cref="ImageLoader"/>.
    /// </summary>
    public class UriToBitmapConverter : IValueConverter
    {
        private const string AppxPrefix = "ms-appx:///";
        private const string AssetsBase = "avares://Lively.UI.Avalonia/";
        private static readonly Dictionary<string, WeakReference<Bitmap>> cache = new Dictionary<string, WeakReference<Bitmap>>(StringComparer.Ordinal);

        public object Convert(object value, Type targetType, object parameter, CultureInfo culture)
        {
            var source = value switch
            {
                Uri uri => uri.IsAbsoluteUri ? uri.AbsoluteUri : uri.OriginalString,
                string s => s,
                _ => null,
            };
            return Load(source);
        }

        public object ConvertBack(object value, Type targetType, object parameter, CultureInfo culture) => throw new NotSupportedException();

        public static Bitmap Load(string source)
        {
            if (string.IsNullOrWhiteSpace(source))
                return null;

            lock (cache)
            {
                if (cache.TryGetValue(source, out var reference) && reference.TryGetTarget(out var cached))
                    return cached;
            }

            var bitmap = LoadUncached(source);
            if (bitmap != null)
            {
                lock (cache)
                {
                    cache[source] = new WeakReference<Bitmap>(bitmap);
                }
            }
            return bitmap;
        }

        private static Bitmap LoadUncached(string source)
        {
            try
            {
                if (source.StartsWith(AppxPrefix, StringComparison.OrdinalIgnoreCase))
                    source = AssetsBase + source.Substring(AppxPrefix.Length);

                if (source.StartsWith("avares://", StringComparison.OrdinalIgnoreCase))
                {
                    using var stream = AssetLoader.Open(new Uri(source));
                    return new Bitmap(stream);
                }

                if (source.StartsWith("file://", StringComparison.OrdinalIgnoreCase))
                    source = new Uri(source).LocalPath;

                if (source.StartsWith("http://", StringComparison.OrdinalIgnoreCase) || source.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                    throw new NotSupportedException($"Remote image '{source}' must be loaded through ImageLoader.");

                if (!File.Exists(source))
                    return null;

                using var file = File.OpenRead(source);
                return new Bitmap(file);
            }
            catch (NotSupportedException)
            {
                throw;
            }
            catch (Exception)
            {
                // Unreadable or unsupported image file; the tile shows without a picture.
                return null;
            }
        }
    }
}
