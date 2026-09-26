using Avalonia.Controls;
using Lively.Common.Services;
using System;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Hosts pages inside a <see cref="ContentControl"/> assigned to <see cref="Frame"/>.
    /// </summary>
    public abstract class NavigatorBase<TPage> : INavigator<TPage> where TPage : struct, Enum
    {
        public event EventHandler<TPage> ContentPageChanged;

        public object RootFrame { get; set; }

        public object Frame { get; set; }

        public TPage? CurrentPage { get; private set; }

        public void NavigateTo(TPage contentPage, object navArgs = null)
        {
            if (CurrentPage.HasValue && CurrentPage.Value.Equals(contentPage))
                return;

            InternalNavigateTo(contentPage, navArgs);
        }

        public void Reload()
        {
            if (CurrentPage == null)
                return;

            InternalNavigateTo(CurrentPage.Value, null);
        }

        /// <summary>
        /// Creates the view for the page. Views receive their view models from the service provider.
        /// </summary>
        protected abstract Control CreateView(TPage contentPage);

        private void InternalNavigateTo(TPage contentPage, object navArgs)
        {
            if (Frame is not ContentControl host)
                throw new InvalidOperationException($"{GetType().Name}.Frame must be a ContentControl before navigating.");

            var view = CreateView(contentPage);
            if (view is INavigationAware aware)
                aware.OnNavigatedTo(navArgs);

            if (host.Content is INavigationAware previous)
                previous.OnNavigatedFrom();

            host.Content = view;
            CurrentPage = contentPage;
            ContentPageChanged?.Invoke(this, contentPage);
        }
    }
}
