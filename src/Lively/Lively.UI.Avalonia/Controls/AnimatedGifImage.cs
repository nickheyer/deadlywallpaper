using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.Platform;
using Avalonia.Threading;
using ImageMagick;
using System;
using System.Collections.Generic;
using System.IO;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Image that plays an animated gif: the frames are decoded with Magick.NET and cycled with a timer.
    /// </summary>
    public class AnimatedGifImage : Image
    {
        public static readonly StyledProperty<string> GifSourceProperty =
            AvaloniaProperty.Register<AnimatedGifImage, string>(nameof(GifSource));

        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly DispatcherTimer timer = new DispatcherTimer();
        private List<(Bitmap frame, TimeSpan delay)> frames;
        private int frameIndex;
        private int loadVersion;

        public AnimatedGifImage()
        {
            timer.Tick += Timer_Tick;
        }

        /// <summary>
        /// avares:// URI or file path of the gif.
        /// </summary>
        public string GifSource
        {
            get => GetValue(GifSourceProperty);
            set => SetValue(GifSourceProperty, value);
        }

        protected override Type StyleKeyOverride => typeof(Image);

        protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
        {
            base.OnPropertyChanged(change);
            if (change.Property == GifSourceProperty)
                _ = LoadAsync(GifSource);
        }

        protected override void OnDetachedFromVisualTree(VisualTreeAttachmentEventArgs e)
        {
            base.OnDetachedFromVisualTree(e);
            timer.Stop();
        }

        protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e)
        {
            base.OnAttachedToVisualTree(e);
            if (frames != null && frames.Count > 1)
                ScheduleNextFrame();
        }

        private async Task LoadAsync(string source)
        {
            var version = ++loadVersion;
            timer.Stop();
            frames = null;
            Source = null;
            if (string.IsNullOrWhiteSpace(source))
                return;

            List<(Bitmap frame, TimeSpan delay)> decoded;
            try
            {
                decoded = await Task.Run(() => Decode(source));
            }
            catch (Exception ex)
            {
                Logger.Error($"Failed to decode gif {source}: {ex}");
                return;
            }

            if (version != loadVersion)
                return;

            frames = decoded;
            frameIndex = 0;
            if (frames.Count > 0)
                Source = frames[0].frame;
            if (frames.Count > 1)
                ScheduleNextFrame();
        }

        private static List<(Bitmap frame, TimeSpan delay)> Decode(string source)
        {
            using var stream = source.StartsWith("avares://", StringComparison.OrdinalIgnoreCase)
                ? AssetLoader.Open(new Uri(source))
                : File.OpenRead(source);
            using var collection = new MagickImageCollection(stream);
            collection.Coalesce();

            var result = new List<(Bitmap, TimeSpan)>(collection.Count);
            foreach (var image in collection)
            {
                image.Format = MagickFormat.Png;
                using var buffer = new MemoryStream();
                image.Write(buffer);
                buffer.Position = 0;
                // AnimationDelay is in 1/100 s; browsers treat 0 as 100 ms.
                var delay = image.AnimationDelay == 0 ? TimeSpan.FromMilliseconds(100) : TimeSpan.FromMilliseconds(image.AnimationDelay * 10);
                result.Add((new Bitmap(buffer), delay));
            }
            return result;
        }

        private void ScheduleNextFrame()
        {
            timer.Interval = frames[frameIndex].delay;
            timer.Start();
        }

        private void Timer_Tick(object sender, EventArgs e)
        {
            timer.Stop();
            if (frames is null || frames.Count == 0)
                return;

            frameIndex = (frameIndex + 1) % frames.Count;
            Source = frames[frameIndex].frame;
            ScheduleNextFrame();
        }
    }
}
