using System;
using System.Net;
using System.Linq;
using System.Net.Http;
using System.Text.RegularExpressions;
using System.Threading;
using System.Threading.Tasks;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Downloads a web page and reduces it to readable text for views that have no web view.
    /// </summary>
    public sealed class WebPageTextFetcher
    {
        private static readonly Regex ScriptAndStyle = new Regex(@"<(script|style|noscript|head|nav|footer)\b[^>]*>.*?</\1>", RegexOptions.Singleline | RegexOptions.IgnoreCase | RegexOptions.Compiled);
        private static readonly Regex BlockBreak = new Regex(@"</?(p|div|br|h[1-6]|li|tr|section|article|header|ul|ol|table)\b[^>]*>", RegexOptions.IgnoreCase | RegexOptions.Compiled);
        private static readonly Regex Tags = new Regex(@"<[^>]+>", RegexOptions.Compiled);
        private static readonly Regex Spaces = new Regex(@"[ \t]+", RegexOptions.Compiled);
        private static readonly Regex BlankLines = new Regex(@"(\r?\n\s*){3,}", RegexOptions.Compiled);

        private readonly IHttpClientFactory httpClientFactory;

        public WebPageTextFetcher(IHttpClientFactory httpClientFactory)
        {
            this.httpClientFactory = httpClientFactory;
        }

        public async Task<string> FetchTextAsync(Uri uri, CancellationToken cancellationToken = default)
        {
            var client = httpClientFactory.CreateClient();
            client.Timeout = TimeSpan.FromSeconds(30);
            var html = await client.GetStringAsync(uri, cancellationToken);
            return HtmlToText(html);
        }

        public static string HtmlToText(string html)
        {
            if (string.IsNullOrEmpty(html))
                return string.Empty;

            var text = ScriptAndStyle.Replace(html, string.Empty);
            text = BlockBreak.Replace(text, "\n");
            text = Tags.Replace(text, string.Empty);
            text = WebUtility.HtmlDecode(text);
            text = Spaces.Replace(text, " ");
            text = string.Join('\n', text.Split('\n').Select(line => line.Trim()));
            text = BlankLines.Replace(text, "\n\n");
            return text.Trim();
        }
    }
}
