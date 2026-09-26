using Newtonsoft.Json;

namespace Lively.UI.Avalonia.Services
{
    /// <summary>
    /// Payload of the core's <c>LM WALLPAPERDATA</c> request.
    /// </summary>
    public sealed class WallpaperDataRequest
    {
        [JsonProperty("infoPath")]
        public string InfoPath { get; set; }

        [JsonProperty("title")]
        public string Title { get; set; }

        [JsonProperty("author")]
        public string Author { get; set; }

        [JsonProperty("desc")]
        public string Desc { get; set; }

        [JsonProperty("contact")]
        public string Contact { get; set; }

        [JsonProperty("thumbnail")]
        public string Thumbnail { get; set; }
    }

    /// <summary>
    /// Answer printed as <c>LM WALLPAPERDATA {json}</c>; the text fields are only present when <see cref="Ok"/> is true.
    /// </summary>
    public sealed class WallpaperDataResult
    {
        [JsonProperty("ok")]
        public bool Ok { get; set; }

        [JsonProperty("title", NullValueHandling = NullValueHandling.Ignore)]
        public string Title { get; set; }

        [JsonProperty("author", NullValueHandling = NullValueHandling.Ignore)]
        public string Author { get; set; }

        [JsonProperty("desc", NullValueHandling = NullValueHandling.Ignore)]
        public string Desc { get; set; }

        [JsonProperty("contact", NullValueHandling = NullValueHandling.Ignore)]
        public string Contact { get; set; }
    }
}
