using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Generators;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.VisualTree;
using System;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.Linq;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Wrap panel for equally sized tiles that realises only the rows inside the scroll viewport plus one
    /// row above and below it, the counterpart of the virtualising GridView the Windows UI uses for the
    /// library. Containers scrolled out of view are recycled for the rows scrolling in.
    /// </summary>
    public class VirtualizingUniformWrapPanel : VirtualizingPanel
    {
        public static readonly StyledProperty<double> ItemWidthProperty =
            AvaloniaProperty.Register<VirtualizingUniformWrapPanel, double>(nameof(ItemWidth), 100);

        public static readonly StyledProperty<double> ItemHeightProperty =
            AvaloniaProperty.Register<VirtualizingUniformWrapPanel, double>(nameof(ItemHeight), 100);

        private readonly Dictionary<int, Control> realized = new Dictionary<int, Control>();
        private readonly Dictionary<Control, object> recycleKeys = new Dictionary<Control, object>();
        private readonly Dictionary<object, Stack<Control>> recyclePool = new Dictionary<object, Stack<Control>>();
        private Rect viewport;
        private bool hasViewport;
        private int columns = 1;

        static VirtualizingUniformWrapPanel()
        {
            AffectsMeasure<VirtualizingUniformWrapPanel>(ItemWidthProperty, ItemHeightProperty);
        }

        /// <summary>Slot width of one tile, margins included.</summary>
        public double ItemWidth
        {
            get => GetValue(ItemWidthProperty);
            set => SetValue(ItemWidthProperty, value);
        }

        /// <summary>Slot height of one tile, margins included.</summary>
        public double ItemHeight
        {
            get => GetValue(ItemHeightProperty);
            set => SetValue(ItemHeightProperty, value);
        }

        private double SlotWidth => Math.Max(1, ItemWidth);
        private double SlotHeight => Math.Max(1, ItemHeight);

        protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e)
        {
            base.OnAttachedToVisualTree(e);
            EffectiveViewportChanged += OnEffectiveViewportChanged;
        }

        protected override void OnDetachedFromVisualTree(VisualTreeAttachmentEventArgs e)
        {
            EffectiveViewportChanged -= OnEffectiveViewportChanged;
            base.OnDetachedFromVisualTree(e);
        }

        private void OnEffectiveViewportChanged(object sender, EffectiveViewportChangedEventArgs e)
        {
            if (hasViewport && e.EffectiveViewport == viewport)
                return;
            viewport = e.EffectiveViewport;
            hasViewport = true;
            InvalidateMeasure();
        }

        protected override Size MeasureOverride(Size availableSize)
        {
            var items = Items;
            var count = items.Count;
            var slotWidth = SlotWidth;
            var slotHeight = SlotHeight;
            var width = double.IsInfinity(availableSize.Width) ? slotWidth * Math.Max(1, count) : availableSize.Width;
            columns = Math.Max(1, (int)Math.Floor(width / slotWidth + 1e-6));
            var rows = count == 0 ? 0 : (count + columns - 1) / columns;

            int firstRow, lastRow;
            if (hasViewport)
            {
                // One row of slack on each side so a scroll step never shows an empty row.
                firstRow = (int)Math.Floor(Math.Max(0, viewport.Top) / slotHeight) - 1;
                lastRow = (int)Math.Ceiling(Math.Max(0, viewport.Bottom) / slotHeight);
            }
            else
            {
                // No viewport yet: the rows the available height can show, or two rows when it is unbounded.
                firstRow = 0;
                lastRow = double.IsInfinity(availableSize.Height) ? 1 : (int)Math.Ceiling(availableSize.Height / slotHeight);
            }
            firstRow = Math.Max(0, firstRow);
            lastRow = Math.Min(rows - 1, lastRow);

            var firstIndex = firstRow * columns;
            var lastIndex = Math.Min(count - 1, (lastRow + 1) * columns - 1);
            RealizeRange(items, firstIndex, lastIndex, new Size(slotWidth, slotHeight));

            return new Size(double.IsInfinity(availableSize.Width) ? columns * slotWidth : availableSize.Width, rows * slotHeight);
        }

        protected override Size ArrangeOverride(Size finalSize)
        {
            var slotWidth = SlotWidth;
            var slotHeight = SlotHeight;
            foreach (var (index, container) in realized)
                container.Arrange(SlotRect(index, slotWidth, slotHeight));
            return finalSize;
        }

        private Rect SlotRect(int index, double slotWidth, double slotHeight)
        {
            return new Rect((index % columns) * slotWidth, (index / columns) * slotHeight, slotWidth, slotHeight);
        }

        private void RealizeRange(IReadOnlyList<object> items, int first, int last, Size slot)
        {
            foreach (var index in realized.Keys.Where(i => i < first || i > last).ToList())
                RecycleContainer(index);

            if (last < first)
                return;

            var generator = ItemContainerGenerator;
            for (var index = first; index <= last; index++)
            {
                if (!realized.TryGetValue(index, out var container))
                {
                    container = GetOrCreateContainer(generator, items[index], index);
                    realized[index] = container;
                }
                container.Measure(slot);
            }
        }

        private Control GetOrCreateContainer(ItemContainerGenerator generator, object item, int index)
        {
            if (!generator.NeedsContainer(item, index, out var recycleKey))
            {
                // The item is a control and is its own container.
                var own = (Control)item;
                generator.PrepareItemContainer(own, item, index);
                AddInternalChild(own);
                generator.ItemContainerPrepared(own, item, index);
                recycleKeys[own] = null;
                return own;
            }

            if (recyclePool.TryGetValue(recycleKey, out var pool) && pool.Count > 0)
            {
                var recycled = pool.Pop();
                recycled.IsVisible = true;
                generator.PrepareItemContainer(recycled, item, index);
                generator.ItemContainerPrepared(recycled, item, index);
                recycleKeys[recycled] = recycleKey;
                return recycled;
            }

            var container = generator.CreateContainer(item, index, recycleKey);
            generator.PrepareItemContainer(container, item, index);
            AddInternalChild(container);
            generator.ItemContainerPrepared(container, item, index);
            recycleKeys[container] = recycleKey;
            return container;
        }

        private void RecycleContainer(int index)
        {
            if (!realized.Remove(index, out var container))
                return;
            RecycleContainer(container);
        }

        private void RecycleContainer(Control container)
        {
            recycleKeys.Remove(container, out var recycleKey);
            var generator = ItemContainerGenerator;
            if (recycleKey == null || generator == null)
            {
                // Items that are their own container leave the panel instead of the pool; so does
                // everything once the panel is detached from its ItemsControl (no generator any more).
                RemoveInternalChild(container);
                return;
            }

            generator.ClearItemContainer(container);
            container.IsVisible = false;
            if (!recyclePool.TryGetValue(recycleKey, out var pool))
            {
                pool = new Stack<Control>();
                recyclePool[recycleKey] = pool;
            }
            pool.Push(container);
        }

        private void RecycleAll()
        {
            foreach (var container in realized.Values.ToList())
                RecycleContainer(container);
            realized.Clear();
        }

        private void ShiftIndices(int from, int delta)
        {
            if (delta == 0)
                return;
            var generator = ItemContainerGenerator;
            var shifted = new Dictionary<int, Control>(realized.Count);
            foreach (var (index, container) in realized)
            {
                if (index < from)
                {
                    shifted[index] = container;
                    continue;
                }
                var newIndex = index + delta;
                generator.ItemContainerIndexChanged(container, index, newIndex);
                shifted[newIndex] = container;
            }
            realized.Clear();
            foreach (var kv in shifted)
                realized[kv.Key] = kv.Value;
        }

        protected override void OnItemsChanged(IReadOnlyList<object> items, NotifyCollectionChangedEventArgs e)
        {
            base.OnItemsChanged(items, e);
            switch (e.Action)
            {
                case NotifyCollectionChangedAction.Add:
                    ShiftIndices(e.NewStartingIndex, e.NewItems.Count);
                    break;
                case NotifyCollectionChangedAction.Remove:
                    for (var i = 0; i < e.OldItems.Count; i++)
                        RecycleContainer(e.OldStartingIndex + i);
                    ShiftIndices(e.OldStartingIndex + e.OldItems.Count, -e.OldItems.Count);
                    break;
                case NotifyCollectionChangedAction.Replace:
                    for (var i = 0; i < e.OldItems.Count; i++)
                        RecycleContainer(e.OldStartingIndex + i);
                    break;
                case NotifyCollectionChangedAction.Move:
                case NotifyCollectionChangedAction.Reset:
                    RecycleAll();
                    break;
            }
            InvalidateMeasure();
        }

        protected override void OnItemsControlChanged(ItemsControl oldValue)
        {
            base.OnItemsControlChanged(oldValue);
            // Attaching to or detaching from an ItemsControl invalidates every container: drop them all
            // (the old generator is gone, so nothing can be cleared or pooled) and start from scratch.
            realized.Clear();
            recyclePool.Clear();
            recycleKeys.Clear();
            RemoveInternalChildRange(0, Children.Count);
            InvalidateMeasure();
        }

        protected override Control ContainerFromIndex(int index)
        {
            return realized.TryGetValue(index, out var container) ? container : null;
        }

        protected override int IndexFromContainer(Control container)
        {
            foreach (var (index, realizedContainer) in realized)
            {
                if (ReferenceEquals(realizedContainer, container))
                    return index;
            }
            return -1;
        }

        protected override IEnumerable<Control> GetRealizedContainers()
        {
            return realized.Values;
        }

        protected override Control ScrollIntoView(int index)
        {
            var items = Items;
            if (index < 0 || index >= items.Count)
                return null;

            var slot = SlotRect(index, SlotWidth, SlotHeight);
            if (!realized.TryGetValue(index, out var container))
            {
                container = GetOrCreateContainer(ItemContainerGenerator, items[index], index);
                realized[index] = container;
                container.Measure(slot.Size);
                container.Arrange(slot);
            }
            this.BringIntoView(slot);
            return container;
        }

        protected override IInputElement GetControl(NavigationDirection direction, IInputElement from, bool wrap)
        {
            var count = Items.Count;
            if (count == 0)
                return null;

            var fromIndex = IndexOfContainerHolding(from as Visual);
            var rowsPerPage = hasViewport ? Math.Max(1, (int)Math.Floor(viewport.Height / SlotHeight)) : 1;
            int target;
            switch (direction)
            {
                case NavigationDirection.First:
                    target = 0;
                    break;
                case NavigationDirection.Last:
                    target = count - 1;
                    break;
                case NavigationDirection.Next:
                case NavigationDirection.Right:
                    target = fromIndex < 0 ? 0 : fromIndex + 1;
                    break;
                case NavigationDirection.Previous:
                case NavigationDirection.Left:
                    target = fromIndex < 0 ? 0 : fromIndex - 1;
                    break;
                case NavigationDirection.Down:
                    target = fromIndex < 0 ? 0 : fromIndex + columns;
                    break;
                case NavigationDirection.Up:
                    target = fromIndex < 0 ? 0 : fromIndex - columns;
                    break;
                case NavigationDirection.PageDown:
                    target = fromIndex < 0 ? 0 : Math.Min(count - 1, fromIndex + columns * rowsPerPage);
                    break;
                case NavigationDirection.PageUp:
                    target = fromIndex < 0 ? 0 : Math.Max(0, fromIndex - columns * rowsPerPage);
                    break;
                default:
                    return null;
            }

            if (target < 0 || target >= count)
            {
                if (!wrap)
                    return null;
                target = target < 0 ? count - 1 : 0;
            }
            return ScrollIntoView(target);
        }

        private int IndexOfContainerHolding(Visual visual)
        {
            while (visual != null)
            {
                if (visual is Control control)
                {
                    var index = IndexFromContainer(control);
                    if (index >= 0)
                        return index;
                }
                visual = visual.GetVisualParent();
            }
            return -1;
        }
    }
}
