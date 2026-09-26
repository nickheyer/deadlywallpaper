using System.Drawing;
using System.Globalization;

namespace Lively.Core.Linux.Wallpapers
{
    /// <summary>
    /// The "--span X,Y,W,H,VW,VH" argument for a host that shows one output's slice of a
    /// wallpaper stretched over the whole virtual screen (PROTOCOL.md section 3).
    /// </summary>
    public readonly struct SpanGeometry
    {
        public int X { get; }
        public int Y { get; }
        public int Width { get; }
        public int Height { get; }
        public int VirtualWidth { get; }
        public int VirtualHeight { get; }

        public SpanGeometry(Rectangle outputBounds, Rectangle virtualBounds)
        {
            X = outputBounds.X - virtualBounds.X;
            Y = outputBounds.Y - virtualBounds.Y;
            Width = outputBounds.Width;
            Height = outputBounds.Height;
            VirtualWidth = virtualBounds.Width;
            VirtualHeight = virtualBounds.Height;
        }

        public string ToArgument() =>
            string.Format(CultureInfo.InvariantCulture, "{0},{1},{2},{3},{4},{5}", X, Y, Width, Height, VirtualWidth, VirtualHeight);
    }
}
