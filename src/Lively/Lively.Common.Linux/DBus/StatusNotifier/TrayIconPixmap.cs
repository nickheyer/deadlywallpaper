using ImageMagick;
using System;
using System.IO;

namespace Lively.Common.Linux.DBus.StatusNotifier
{
    /// <summary>
    /// Decodes tray icon images into the StatusNotifierItem pixmap format:
    /// <c>(width, height, pixels)</c> with each pixel as ARGB32 in network byte order (A, R, G, B).
    /// </summary>
    public static class TrayIconPixmap
    {
        private const int BytesPerPixel = 4;

        public static (int width, int height, byte[] argb) Load(string imagePath)
        {
            if (string.IsNullOrWhiteSpace(imagePath))
                throw new ArgumentException("The icon path must not be empty.", nameof(imagePath));
            if (!File.Exists(imagePath))
                throw new FileNotFoundException("Tray icon image not found.", imagePath);

            using var image = new MagickImage(imagePath);
            image.Alpha(AlphaOption.Set);
            var width = checked((int)image.Width);
            var height = checked((int)image.Height);
            using var pixels = image.GetPixels();
            var argb = pixels.ToByteArray(PixelMapping.ARGB);
            var expected = checked(width * height * BytesPerPixel);
            if (argb is null || argb.Length != expected)
                throw new InvalidDataException($"Decoding {imagePath} produced {argb?.Length ?? 0} bytes; expected {expected} for {width}x{height} ARGB32.");
            return (width, height, argb);
        }
    }
}
