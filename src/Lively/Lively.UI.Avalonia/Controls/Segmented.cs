using Avalonia.Controls;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Horizontal single-choice selector; the counterpart of the WinUI toolkit Segmented control.
    /// </summary>
    public class Segmented : ListBox
    {
        protected override Control CreateContainerForItemOverride(object item, int index, object recycleKey) => new SegmentedItem();

        protected override bool NeedsContainerOverride(object item, int index, out object recycleKey)
        {
            if (item is SegmentedItem)
            {
                recycleKey = null;
                return false;
            }
            recycleKey = nameof(SegmentedItem);
            return true;
        }
    }

    public class SegmentedItem : ListBoxItem
    {
    }
}
