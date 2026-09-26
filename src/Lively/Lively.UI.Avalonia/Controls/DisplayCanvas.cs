using Avalonia.Controls;
using Avalonia.Data;
using Lively.Models;
using System;

namespace Lively.UI.Avalonia.Controls
{
    /// <summary>
    /// Items control whose containers are positioned on a <see cref="Canvas"/> from the item's normalised bounds.
    /// </summary>
    public class DisplayCanvas : ItemsControl
    {
        /// <summary>
        /// A subclass gets its own style key, and no theme exists for it; without this the control has no
        /// template and renders nothing.
        /// </summary>
        protected override Type StyleKeyOverride => typeof(ItemsControl);

        protected override void PrepareContainerForItemOverride(Control container, object item, int index)
        {
            base.PrepareContainerForItemOverride(container, item, index);
            if (item is not ScreenLayoutModel model)
                return;

            container.Bind(Canvas.LeftProperty, new Binding($"{nameof(ScreenLayoutModel.NormalizedBounds)}.Left") { Source = model });
            container.Bind(Canvas.TopProperty, new Binding($"{nameof(ScreenLayoutModel.NormalizedBounds)}.Top") { Source = model });
        }
    }
}
