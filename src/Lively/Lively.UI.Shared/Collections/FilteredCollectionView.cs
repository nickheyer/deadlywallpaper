using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;
using System.Linq;
using System.Reflection;

namespace Lively.UI.Shared.Collections
{
    public enum SortDirection
    {
        Ascending,
        Descending
    }

    public sealed class SortDescription
    {
        public SortDescription(string propertyName, SortDirection direction)
        {
            PropertyName = propertyName;
            Direction = direction;
        }

        public string PropertyName { get; }
        public SortDirection Direction { get; }
    }

    /// <summary>
    /// A filtered and sorted read-only view over an <see cref="ObservableCollection{T}"/>.
    /// The view stays in sync with the source collection and, when live shaping is enabled,
    /// with property changes of the items themselves.
    /// </summary>
    public sealed class FilteredCollectionView<T> : IList<T>, IReadOnlyList<T>, IList, INotifyCollectionChanged, INotifyPropertyChanged
        where T : class
    {
        private readonly ObservableCollection<T> source;
        private readonly List<T> view = new List<T>();
        private readonly bool isLiveShaping;
        private readonly Dictionary<string, PropertyInfo> propertyCache = new Dictionary<string, PropertyInfo>(StringComparer.Ordinal);
        private Predicate<T> filter;
        private int deferCount;
        private bool refreshPending;

        public event NotifyCollectionChangedEventHandler CollectionChanged;
        public event PropertyChangedEventHandler PropertyChanged;

        public FilteredCollectionView(ObservableCollection<T> source, bool isLiveShaping)
        {
            this.source = source ?? throw new ArgumentNullException(nameof(source));
            this.isLiveShaping = isLiveShaping;

            SortDescriptions.CollectionChanged += (_, _) => Refresh();
            source.CollectionChanged += Source_CollectionChanged;
            if (isLiveShaping)
            {
                foreach (var item in source)
                    Subscribe(item);
            }
            RebuildView();
        }

        public ObservableCollection<T> Source => source;

        public ObservableCollection<SortDescription> SortDescriptions { get; } = new ObservableCollection<SortDescription>();

        public Predicate<T> Filter
        {
            get => filter;
            set
            {
                filter = value;
                Refresh();
            }
        }

        public int Count => view.Count;

        public T this[int index]
        {
            get => view[index];
            set => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        }

        /// <summary>
        /// Suspends refreshes until the returned object is disposed, then refreshes once.
        /// </summary>
        public IDisposable DeferRefresh()
        {
            deferCount++;
            return new DeferHandle(this);
        }

        public void Refresh()
        {
            if (deferCount > 0)
            {
                refreshPending = true;
                return;
            }

            refreshPending = false;
            RebuildView();
        }

        private void EndDefer()
        {
            deferCount--;
            if (deferCount == 0 && refreshPending)
                Refresh();
        }

        private void RebuildView()
        {
            view.Clear();
            var items = source.Where(PassesFilter);
            view.AddRange(SortDescriptions.Count == 0 ? items : items.OrderBy(x => x, Comparer.Instance(this)));
            OnCollectionReset();
        }

        private bool PassesFilter(T item) => filter == null || filter(item);

        private void Source_CollectionChanged(object sender, NotifyCollectionChangedEventArgs e)
        {
            if (isLiveShaping)
            {
                if (e.OldItems != null)
                    foreach (T item in e.OldItems)
                        Unsubscribe(item);
                if (e.NewItems != null)
                    foreach (T item in e.NewItems)
                        Subscribe(item);
                if (e.Action == NotifyCollectionChangedAction.Reset)
                    foreach (var item in source)
                        Subscribe(item);
            }

            if (deferCount > 0)
            {
                refreshPending = true;
                return;
            }

            switch (e.Action)
            {
                case NotifyCollectionChangedAction.Add:
                    foreach (T item in e.NewItems)
                        InsertItem(item);
                    break;
                case NotifyCollectionChangedAction.Remove:
                    foreach (T item in e.OldItems)
                        RemoveItem(item);
                    break;
                default:
                    RebuildView();
                    break;
            }
        }

        private void InsertItem(T item)
        {
            if (!PassesFilter(item) || view.Contains(item))
                return;

            var index = FindInsertIndex(item);
            view.Insert(index, item);
            OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Add, item, index));
        }

        private void RemoveItem(T item)
        {
            var index = view.IndexOf(item);
            if (index < 0)
                return;

            view.RemoveAt(index);
            OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Remove, item, index));
        }

        private int FindInsertIndex(T item)
        {
            if (SortDescriptions.Count == 0)
            {
                // Keep source order when unsorted.
                var sourceIndex = source.IndexOf(item);
                for (int i = 0; i < view.Count; i++)
                {
                    if (source.IndexOf(view[i]) > sourceIndex)
                        return i;
                }
                return view.Count;
            }

            var comparer = Comparer.Instance(this);
            int low = 0, high = view.Count;
            while (low < high)
            {
                int mid = (low + high) / 2;
                if (comparer.Compare(view[mid], item) <= 0)
                    low = mid + 1;
                else
                    high = mid;
            }
            return low;
        }

        private void Subscribe(T item)
        {
            if (item is INotifyPropertyChanged inpc)
            {
                inpc.PropertyChanged -= Item_PropertyChanged;
                inpc.PropertyChanged += Item_PropertyChanged;
            }
        }

        private void Unsubscribe(T item)
        {
            if (item is INotifyPropertyChanged inpc)
                inpc.PropertyChanged -= Item_PropertyChanged;
        }

        private void Item_PropertyChanged(object sender, PropertyChangedEventArgs e)
        {
            if (!(sender is T item) || deferCount > 0)
                return;

            var affectsSort = string.IsNullOrEmpty(e.PropertyName) || SortDescriptions.Any(x => x.PropertyName == e.PropertyName);
            var affectsFilter = filter != null;
            if (!affectsSort && !affectsFilter)
                return;

            var currentIndex = view.IndexOf(item);
            var passes = PassesFilter(item);
            if (currentIndex < 0)
            {
                if (passes)
                    InsertItem(item);
                return;
            }

            if (!passes)
            {
                RemoveItem(item);
                return;
            }

            if (!affectsSort)
                return;

            view.RemoveAt(currentIndex);
            var newIndex = FindInsertIndex(item);
            view.Insert(newIndex, item);
            if (newIndex != currentIndex)
                OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Move, item, newIndex, currentIndex));
        }

        private void OnCollectionReset()
        {
            OnCollectionChanged(new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
        }

        private void OnCollectionChanged(NotifyCollectionChangedEventArgs e)
        {
            CollectionChanged?.Invoke(this, e);
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Count)));
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs("Item[]"));
        }

        private object GetPropertyValue(T item, string propertyName)
        {
            if (!propertyCache.TryGetValue(propertyName, out var property))
            {
                property = typeof(T).GetProperty(propertyName, BindingFlags.Public | BindingFlags.Instance);
                if (property == null)
                    throw new ArgumentException($"Property '{propertyName}' not found on type '{typeof(T).Name}'.");
                propertyCache[propertyName] = property;
            }
            return property.GetValue(item);
        }

        #region IList<T> / IReadOnlyList<T>

        public int IndexOf(T item) => view.IndexOf(item);
        public bool Contains(T item) => view.Contains(item);
        public void CopyTo(T[] array, int arrayIndex) => view.CopyTo(array, arrayIndex);
        public IEnumerator<T> GetEnumerator() => view.GetEnumerator();
        IEnumerator IEnumerable.GetEnumerator() => view.GetEnumerator();
        public bool IsReadOnly => true;
        public void Add(T item) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        public void Insert(int index, T item) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        public bool Remove(T item) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        public void RemoveAt(int index) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        public void Clear() => throw new NotSupportedException("The view is read-only, modify the source collection instead.");

        #endregion

        #region IList

        bool IList.IsFixedSize => false;
        bool ICollection.IsSynchronized => false;
        object ICollection.SyncRoot => this;
        object IList.this[int index]
        {
            get => view[index];
            set => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        }
        int IList.Add(object value) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        bool IList.Contains(object value) => value is T item && view.Contains(item);
        int IList.IndexOf(object value) => value is T item ? view.IndexOf(item) : -1;
        void IList.Insert(int index, object value) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        void IList.Remove(object value) => throw new NotSupportedException("The view is read-only, modify the source collection instead.");
        void ICollection.CopyTo(Array array, int index) => ((ICollection)view).CopyTo(array, index);

        #endregion

        private sealed class DeferHandle : IDisposable
        {
            private FilteredCollectionView<T> owner;

            public DeferHandle(FilteredCollectionView<T> owner)
            {
                this.owner = owner;
            }

            public void Dispose()
            {
                owner?.EndDefer();
                owner = null;
            }
        }

        private sealed class Comparer : IComparer<T>
        {
            private readonly FilteredCollectionView<T> owner;

            private Comparer(FilteredCollectionView<T> owner)
            {
                this.owner = owner;
            }

            public static Comparer Instance(FilteredCollectionView<T> owner) => new Comparer(owner);

            public int Compare(T x, T y)
            {
                foreach (var description in owner.SortDescriptions)
                {
                    var left = owner.GetPropertyValue(x, description.PropertyName);
                    var right = owner.GetPropertyValue(y, description.PropertyName);
                    int result;
                    if (left is string ls && right is string rs)
                        result = string.Compare(ls, rs, StringComparison.CurrentCultureIgnoreCase);
                    else
                        result = System.Collections.Comparer.Default.Compare(left, right);

                    if (result != 0)
                        return description.Direction == SortDirection.Ascending ? result : -result;
                }
                return 0;
            }
        }
    }
}
