using CommunityToolkit.Mvvm.ComponentModel;
using Lively.Core.Display;
using Lively.Models;
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Drawing;
using System.Linq;

namespace Lively.Core.Linux.Display
{
    /// <summary>
    /// IDisplayManager backed by the Wayland outputs reported by lively-wl-monitor.
    /// DeviceId and DeviceName are the connector name (e.g. DP-4), which is stable across sessions.
    /// </summary>
    public sealed class LinuxDisplayManager : ObservableObject, IDisplayManager
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();

        private readonly WaylandMonitorService monitor;
        private readonly object sync = new object();
        private Rectangle virtualScreenBounds = Rectangle.Empty;

        public event EventHandler DisplayUpdated;

        public ObservableCollection<DisplayMonitor> DisplayMonitors { get; } = new ObservableCollection<DisplayMonitor>();

        public Rectangle VirtualScreenBounds
        {
            get => virtualScreenBounds;
            private set => SetProperty(ref virtualScreenBounds, value);
        }

        public DisplayMonitor PrimaryDisplayMonitor => DisplayMonitors.FirstOrDefault(x => x.IsPrimary) ?? DisplayMonitors.FirstOrDefault();

        public LinuxDisplayManager(WaylandMonitorService monitor)
        {
            this.monitor = monitor;
            this.monitor.OutputsChanged += (s, e) => Refresh();
            Refresh();
        }

        public bool IsMultiScreen() => DisplayMonitors.Count > 1;

        public bool ScreenExists(DisplayMonitor display) => DisplayMonitors.Any(x => x.Equals(display));

        public DisplayMonitor GetDisplayMonitorFromPoint(Point point)
        {
            return DisplayMonitors.FirstOrDefault(x => x.Bounds.Contains(point))
                ?? DisplayMonitors.OrderBy(x => DistanceToRect(point, x.Bounds)).FirstOrDefault()
                ?? PrimaryDisplayMonitor;
        }

        public DisplayMonitor GetDisplayMonitorFromHWnd(IntPtr hWnd)
        {
            throw new PlatformNotSupportedException("Window handles do not exist on Wayland; use the toplevel output list from lively-wl-monitor instead.");
        }

        public uint OnHwndCreated(IntPtr hWnd, out bool register)
        {
            register = false;
            return 0;
        }

        /// <summary>Wayland output name for a display (identical to DeviceId).</summary>
        public static string OutputName(DisplayMonitor display) => display.DeviceId;

        public WaylandOutput GetOutput(DisplayMonitor display) => monitor.Outputs.FirstOrDefault(o => o.Name == display.DeviceId);

        /// <summary>
        /// Applies the work areas the desktop backend computed (display bounds minus the panels that
        /// reserve space), the counterpart of the Windows working area without the taskbar. Displays
        /// without an entry keep their full bounds. Raises no DisplayUpdated: nothing about the
        /// outputs changed.
        /// </summary>
        public void SetWorkingAreas(IReadOnlyDictionary<string, Rectangle> workingAreas)
        {
            lock (sync)
            {
                foreach (var display in DisplayMonitors)
                {
                    display.WorkingArea = workingAreas.TryGetValue(display.DeviceId, out var area) && !area.IsEmpty
                        ? area
                        : display.Bounds;
                }
                Logger.Info($"Work areas: {string.Join(", ", DisplayMonitors.Select(d => $"{d.DeviceId} {d.WorkingArea}"))}");
            }
        }

        private void Refresh()
        {
            lock (sync)
            {
                // Stable order: left to right, then top to bottom, so display indexes (used by the
                // CLI's --monitor) and the primary choice do not depend on the compositor's list order.
                var outputs = monitor.Outputs.OrderBy(o => o.X).ThenBy(o => o.Y).ToList();
                // Primary: the output containing the origin, else the leftmost one (where desktops put it).
                var primary = outputs.FirstOrDefault(o => o.X <= 0 && o.Y <= 0 && o.X + o.Width > 0 && o.Y + o.Height > 0)
                    ?? outputs.FirstOrDefault();

                foreach (var existing in DisplayMonitors)
                    existing.isStale = true;

                var index = 0;
                foreach (var output in outputs)
                {
                    index++;
                    var bounds = new Rectangle(output.X, output.Y, output.Width, output.Height);
                    var existing = DisplayMonitors.FirstOrDefault(x => x.DeviceId == output.Name);
                    if (existing == null)
                    {
                        DisplayMonitors.Add(new DisplayMonitor(output.Name)
                        {
                            DeviceId = output.Name,
                            DisplayName = string.IsNullOrWhiteSpace(output.Description) ? output.Name : output.Description,
                            Bounds = bounds,
                            WorkingArea = bounds,
                            HMonitor = IntPtr.Zero,
                            Index = index,
                            IsPrimary = ReferenceEquals(output, primary),
                            isStale = false,
                        });
                    }
                    else
                    {
                        existing.DisplayName = string.IsNullOrWhiteSpace(output.Description) ? output.Name : output.Description;
                        existing.Bounds = bounds;
                        existing.WorkingArea = bounds;
                        existing.Index = index;
                        existing.IsPrimary = ReferenceEquals(output, primary);
                        existing.isStale = false;
                    }
                }

                foreach (var stale in DisplayMonitors.Where(x => x.isStale).ToList())
                {
                    Logger.Info($"Output removed: {stale.DeviceId}");
                    DisplayMonitors.Remove(stale);
                }

                // Keep the collection itself in index order (callers iterate it positionally).
                for (var target = 0; target < DisplayMonitors.Count; target++)
                {
                    var current = DisplayMonitors.Select((d, i) => (d, i)).First(x => x.d.Index == target + 1).i;
                    if (current != target)
                        DisplayMonitors.Move(current, target);
                }

                VirtualScreenBounds = ComputeVirtualBounds(DisplayMonitors.Select(x => x.Bounds));
                Logger.Info($"Displays: {string.Join(", ", DisplayMonitors.Select(d => $"#{d.Index} {d.DeviceId} {d.Bounds}{(d.IsPrimary ? " primary" : "")}"))}");
            }
            DisplayUpdated?.Invoke(this, EventArgs.Empty);
        }

        private static Rectangle ComputeVirtualBounds(IEnumerable<Rectangle> rects)
        {
            var list = rects.ToList();
            if (list.Count == 0)
                return Rectangle.Empty;
            var result = list[0];
            foreach (var r in list.Skip(1))
                result = Rectangle.Union(result, r);
            return result;
        }

        private static double DistanceToRect(Point p, Rectangle r)
        {
            var dx = Math.Max(Math.Max(r.Left - p.X, 0), p.X - r.Right);
            var dy = Math.Max(Math.Max(r.Top - p.Y, 0), p.Y - r.Bottom);
            return Math.Sqrt(dx * dx + dy * dy);
        }
    }
}
