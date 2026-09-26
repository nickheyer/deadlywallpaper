using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Lively.Common.Services;
using System;
using System.IO;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Converters
{
    /// <summary>
    /// Attached property that loads an image source asynchronously: local paths through <see cref="UriToBitmapConverter"/>,
    /// http(s) URLs through the <see cref="ICacheService"/> registered by the application.
    /// <code>&lt;Image conv:ImageLoader.Source="{Binding LivelyInfo.Thumbnail}" /&gt;</code>
    /// </summary>
    public static class ImageLoader
    {
        public static readonly AttachedProperty<string> SourceProperty =
            AvaloniaProperty.RegisterAttached<Image, string>("Source", typeof(ImageLoader));

        public static readonly AttachedProperty<Uri> UriSourceProperty =
            AvaloniaProperty.RegisterAttached<Image, Uri>("UriSource", typeof(ImageLoader));

        private static readonly AttachedProperty<int> RequestVersionProperty =
            AvaloniaProperty.RegisterAttached<Image, int>("RequestVersion", typeof(ImageLoader));

        /// <summary>
        /// Cache used for remote images; assigned by the application once the service container exists.
        /// </summary>
        public static ICacheService Cache { get; set; }

        static ImageLoader()
        {
            SourceProperty.Changed.AddClassHandler<Image>((image, args) => _ = LoadAsync(image, args.NewValue as string));
            UriSourceProperty.Changed.AddClassHandler<Image>((image, args) => _ = LoadAsync(image, (args.NewValue as Uri)?.AbsoluteUri));
        }

        public static string GetSource(Image image) => image.GetValue(SourceProperty);
        public static void SetSource(Image image, string value) => image.SetValue(SourceProperty, value);

        public static Uri GetUriSource(Image image) => image.GetValue(UriSourceProperty);
        public static void SetUriSource(Image image, Uri value) => image.SetValue(UriSourceProperty, value);

        private static async Task LoadAsync(Image image, string source)
        {
            var version = image.GetValue(RequestVersionProperty) + 1;
            image.SetValue(RequestVersionProperty, version);

            if (string.IsNullOrWhiteSpace(source))
            {
                image.Source = null;
                return;
            }

            Bitmap bitmap;
            try
            {
                if (source.StartsWith("http://", StringComparison.OrdinalIgnoreCase) || source.StartsWith("https://", StringComparison.OrdinalIgnoreCase))
                {
                    var cache = Cache ?? throw new InvalidOperationException("ImageLoader.Cache must be assigned before remote images can be loaded.");
                    var file = await cache.GetFileFromCacheAsync(new Uri(source));
                    bitmap = file == null ? null : await Task.Run(() => UriToBitmapConverter.Load(file));
                }
                else
                {
                    bitmap = await Task.Run(() => UriToBitmapConverter.Load(source));
                }
            }
            catch (IOException)
            {
                bitmap = null;
            }

            await Dispatcher.UIThread.InvokeAsync(() =>
            {
                if (image.GetValue(RequestVersionProperty) == version)
                    image.Source = bitmap;
            });
        }
    }
}
