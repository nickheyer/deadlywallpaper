using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using System.ComponentModel;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Shared.Collections
{
    /// <summary>
    /// Supplies pages of items to an <see cref="IncrementalCollection{TSource, T}"/>.
    /// </summary>
    public interface IIncrementalSource<T>
    {
        Task<IEnumerable<T>> GetPagedItemsAsync(int pageIndex, int pageSize, CancellationToken cancellationToken = default);
    }

    /// <summary>
    /// An observable collection that loads its items page by page from an <see cref="IIncrementalSource{T}"/>.
    /// The first page is requested when the collection is created; further pages are requested
    /// by the view through <see cref="LoadMoreItemsAsync"/> while <see cref="HasMoreItems"/> is true.
    /// All members must be used from the thread that created the collection.
    /// </summary>
    public class IncrementalCollection<TSource, T> : ObservableCollection<T> where TSource : IIncrementalSource<T>
    {
        private readonly Action onStartLoading;
        private readonly Action onEndLoading;
        private readonly Action<Exception> onError;
        private CancellationTokenSource refreshCts;
        private Task<int> loadingTask;
        private bool hasMoreItems = true;
        private bool isLoading;

        public IncrementalCollection(TSource source, int itemsPerPage = 20, Action onStartLoading = null, Action onEndLoading = null, Action<Exception> onError = null)
        {
            Source = source ?? throw new ArgumentNullException(nameof(source));
            ItemsPerPage = itemsPerPage;
            this.onStartLoading = onStartLoading;
            this.onEndLoading = onEndLoading;
            this.onError = onError;

            _ = LoadMoreItemsAsync(itemsPerPage);
        }

        public TSource Source { get; }

        public int ItemsPerPage { get; }

        public int CurrentPageIndex { get; private set; }

        public bool HasMoreItems
        {
            get => hasMoreItems;
            private set
            {
                if (hasMoreItems == value)
                    return;
                hasMoreItems = value;
                OnPropertyChanged(new PropertyChangedEventArgs(nameof(HasMoreItems)));
            }
        }

        public bool IsLoading
        {
            get => isLoading;
            private set
            {
                if (isLoading == value)
                    return;
                isLoading = value;
                OnPropertyChanged(new PropertyChangedEventArgs(nameof(IsLoading)));
            }
        }

        /// <summary>
        /// Loads the next page. Returns the number of items added.
        /// Concurrent calls await the same in-flight request.
        /// </summary>
        public Task<int> LoadMoreItemsAsync(int count, CancellationToken cancellationToken = default)
        {
            if (loadingTask != null && !loadingTask.IsCompleted)
                return loadingTask;

            loadingTask = LoadMoreItemsInternalAsync(count, cancellationToken);
            return loadingTask;
        }

        /// <summary>
        /// Clears the collection and loads the first page again.
        /// </summary>
        public async Task RefreshAsync()
        {
            refreshCts?.Cancel();
            refreshCts = new CancellationTokenSource();
            var token = refreshCts.Token;

            if (loadingTask != null && !loadingTask.IsCompleted)
            {
                try
                {
                    await loadingTask;
                }
                catch (Exception)
                {
                    // Error already reported by the load that raised it.
                }
            }

            if (token.IsCancellationRequested)
                return;

            Clear();
            CurrentPageIndex = 0;
            HasMoreItems = true;
            await LoadMoreItemsAsync(ItemsPerPage, token);
        }

        private async Task<int> LoadMoreItemsInternalAsync(int count, CancellationToken cancellationToken)
        {
            if (!HasMoreItems)
                return 0;

            var pageSize = Math.Max(count, ItemsPerPage);
            var added = 0;
            try
            {
                IsLoading = true;
                onStartLoading?.Invoke();

                var items = await Source.GetPagedItemsAsync(CurrentPageIndex, pageSize, cancellationToken);
                if (cancellationToken.IsCancellationRequested)
                    return 0;

                if (items != null)
                {
                    foreach (var item in items)
                    {
                        Add(item);
                        added++;
                    }
                }

                if (added > 0)
                    CurrentPageIndex++;
                else
                    HasMoreItems = false;
            }
            catch (OperationCanceledException)
            {
                // Refresh or shutdown cancelled the page request.
            }
            catch (Exception ex)
            {
                HasMoreItems = false;
                onError?.Invoke(ex);
            }
            finally
            {
                IsLoading = false;
                onEndLoading?.Invoke();
            }
            return added;
        }
    }
}
