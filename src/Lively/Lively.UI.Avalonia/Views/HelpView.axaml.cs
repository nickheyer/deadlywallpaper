using Avalonia.Controls;
using Avalonia.Interactivity;
using Lively.Common;

namespace Lively.UI.Avalonia.Views
{
    public partial class HelpView : UserControl
    {
        public HelpView()
        {
            InitializeComponent();
        }

        private void WebsiteCard_Click(object sender, RoutedEventArgs e) => LinkUtil.OpenBrowser("https://www.rocksdanister.com/lively/");

        private void DocumentationCard_Click(object sender, RoutedEventArgs e) => LinkUtil.OpenBrowser("https://github.com/rocksdanister/lively/wiki");

        private void CommunityCard_Click(object sender, RoutedEventArgs e) => LinkUtil.OpenBrowser("https://www.reddit.com/r/LivelyWallpaper/");

        private void SourceCodeCard_Click(object sender, RoutedEventArgs e) => LinkUtil.OpenBrowser("https://github.com/rocksdanister/lively");

        private void ReportBugCard_Click(object sender, RoutedEventArgs e) => LinkUtil.OpenBrowser("https://github.com/rocksdanister/lively/wiki/Common-Problems");
    }
}
