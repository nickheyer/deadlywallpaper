using Lively.Common.Services;
using System;
using System.Collections.Generic;
using System.ComponentModel;

namespace Lively.UI.Avalonia.Localization
{
    /// <summary>
    /// Process-wide access to the <see cref="IResourceService"/> for XAML markup; notifies the
    /// <see cref="LocalizedString"/> instances created by <see cref="Str"/> when the culture changes.
    /// </summary>
    public static class LocalizationSource
    {
        private static readonly List<WeakReference<LocalizedString>> subscribers = new List<WeakReference<LocalizedString>>();
        private static readonly object gate = new object();
        private static IResourceService current;

        public static IResourceService Current
        {
            get => current ?? throw new InvalidOperationException("LocalizationSource.Initialize must be called before any string is resolved.");
        }

        public static void Initialize(IResourceService resourceService)
        {
            if (current != null)
                current.CultureChanged -= OnCultureChanged;

            current = resourceService ?? throw new ArgumentNullException(nameof(resourceService));
            current.CultureChanged += OnCultureChanged;
        }

        public static string GetString(string key) => Current.GetString(key);

        internal static void Register(LocalizedString localizedString)
        {
            lock (gate)
            {
                subscribers.Add(new WeakReference<LocalizedString>(localizedString));
            }
        }

        private static void OnCultureChanged(object sender, string e)
        {
            List<LocalizedString> alive;
            lock (gate)
            {
                alive = new List<LocalizedString>(subscribers.Count);
                subscribers.RemoveAll(reference =>
                {
                    if (reference.TryGetTarget(out var target))
                    {
                        alive.Add(target);
                        return false;
                    }
                    return true;
                });
            }

            foreach (var item in alive)
                item.Refresh();
        }
    }

    /// <summary>
    /// A single localized string that re-resolves itself when the culture changes.
    /// </summary>
    public sealed class LocalizedString : INotifyPropertyChanged
    {
        public event PropertyChangedEventHandler PropertyChanged;

        public LocalizedString(string key)
        {
            Key = key;
            LocalizationSource.Register(this);
        }

        public string Key { get; }

        public string Value => LocalizationSource.GetString(Key);

        internal void Refresh() => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Value)));
    }
}
