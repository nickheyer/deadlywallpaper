using Avalonia.Data;
using Avalonia.Markup.Xaml;
using System;

namespace Lively.UI.Avalonia.Localization
{
    /// <summary>
    /// XAML markup extension resolving a localized string: <c>{loc:Str Cancel/Content}</c>.
    /// The key uses the WinUI x:Uid form (<c>Uid/Property</c>); the bound value updates when the culture changes.
    /// </summary>
    public sealed class Str : MarkupExtension
    {
        public Str()
        {
        }

        public Str(string key)
        {
            Key = key;
        }

        [ConstructorArgument("key")]
        public string Key { get; set; }

        public override object ProvideValue(IServiceProvider serviceProvider)
        {
            if (string.IsNullOrWhiteSpace(Key))
                throw new InvalidOperationException("Str markup extension requires a resource key.");

            return new Binding(nameof(LocalizedString.Value))
            {
                Source = new LocalizedString(Key),
                Mode = BindingMode.OneWay,
            };
        }
    }
}
