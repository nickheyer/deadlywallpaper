namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Implemented by views that receive navigation arguments from an <see cref="NavigatorBase{TPage}"/>.
    /// </summary>
    public interface INavigationAware
    {
        /// <summary>
        /// Called after the view was created and before it is placed into the frame.
        /// </summary>
        void OnNavigatedTo(object navArgs);

        /// <summary>
        /// Called when the frame navigates away from the view.
        /// </summary>
        void OnNavigatedFrom();
    }
}
