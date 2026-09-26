using Avalonia.Controls;
using Avalonia.Threading;
using Lively.Gallery.Client;
using Lively.UI.Shared.ViewModels;
using System;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Views.Gallery
{
    /// <summary>
    /// Gallery shell: login page until the gallery client is authenticated, then the rail navigates
    /// between the collection, the subscriptions and the profile.
    /// </summary>
    public partial class GalleryView : UserControl
    {
        private readonly IServiceProvider services;
        private readonly GalleryClient galleryClient;
        private bool isAuthenticated;
        private bool isNavigating;
        private string currentPage;

        public GalleryView(IServiceProvider services, GalleryClient galleryClient)
        {
            this.services = services;
            this.galleryClient = galleryClient;
            InitializeComponent();

            if (galleryClient.IsLoggedIn)
            {
                isAuthenticated = true;
                NavigatePage("library");
            }
            else
            {
                NavigationRail.IsVisible = false;
                NavigatePage("login");
                galleryClient.LoggedIn += GalleryClient_LoggedIn;
            }
        }

        private void GalleryClient_LoggedIn(object sender, object e)
        {
            galleryClient.LoggedIn -= GalleryClient_LoggedIn;
            isAuthenticated = true;
            Dispatcher.UIThread.Post(async () =>
            {
                // Let the login page show its welcome message first.
                await Task.Delay(2000);
                NavigatePage("library");
                NavigationRail.IsVisible = true;
            });
        }

        private void Menu_SelectionChanged(object sender, SelectionChangedEventArgs e)
        {
            if (isNavigating || !isAuthenticated || sender is not ListBox list || list.SelectedItem is not ListBoxItem item)
                return;

            NavigatePage(item.Tag as string);
        }

        private void NavigatePage(string tag)
        {
            if (tag is null || tag == currentPage)
                return;

            Control page = tag switch
            {
                "login" => new GalleryLoginView(services.GetRequiredServiceFrom<GalleryLoginViewModel>()),
                "library" => new GalleryLibraryView(services.GetRequiredServiceFrom<GalleryViewModel>()),
                "subscription" => new GallerySubscriptionView(services.GetRequiredServiceFrom<GallerySubscriptionViewModel>()),
                "profile" => new ManageAccountView(services.GetRequiredServiceFrom<ManageAccountViewModel>()),
                _ => throw new ArgumentOutOfRangeException(nameof(tag), tag, "Unknown gallery page."),
            };
            currentPage = tag;
            ContentFrame.Content = page;
            SelectRailItem(tag);
        }

        private void SelectRailItem(string tag)
        {
            isNavigating = true;
            try
            {
                MenuList.SelectedItem = FindItem(MenuList, tag);
                FooterList.SelectedItem = FindItem(FooterList, tag);
            }
            finally
            {
                isNavigating = false;
            }
        }

        private static ListBoxItem FindItem(ListBox list, string tag)
        {
            foreach (var item in list.Items)
            {
                if (item is ListBoxItem listItem && Equals(listItem.Tag, tag))
                    return listItem;
            }
            return null;
        }
    }

    internal static class ServiceProviderExtensions
    {
        public static T GetRequiredServiceFrom<T>(this IServiceProvider services) where T : class
        {
            return services.GetService(typeof(T)) as T
                ?? throw new InvalidOperationException($"{typeof(T).Name} is not registered.");
        }
    }
}
