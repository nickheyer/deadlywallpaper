using System.Drawing;

namespace Lively.Common.Helpers
{
    public static class InputMath
    {
        /// <summary>
        /// Converts global mouse position to per-display localized coordinates.
        /// </summary>
        public static Point ToMouseDisplayLocal(int x, int y, Rectangle displayBounds)
        {
            x += -1 * displayBounds.X;
            y += -1 * displayBounds.Y;
            return new Point(x, y);
        }

        /// <summary>
        /// Converts global mouse position to span-local coordinates.
        /// </summary>
        public static Point ToMouseSpanLocal(int x, int y, Rectangle virtualScreenBounds)
        {
            x -= virtualScreenBounds.Location.X;
            y -= virtualScreenBounds.Location.Y;
            return new Point(x, y);
        }
    }
}
