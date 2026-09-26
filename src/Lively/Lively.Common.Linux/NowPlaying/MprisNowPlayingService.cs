using ImageMagick;
using Lively.Common.Services;
using Lively.Models.Enums;
using Lively.Models.Services;
using NLog;
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using Tmds.DBus;

namespace Lively.Common.Linux.NowPlaying
{
    /// <summary>
    /// Reports the current track of the MPRIS player on the session bus, preferring the one that is playing.
    /// Album art is delivered as a base64 PNG bounded to <see cref="ArtMaxDimension"/> pixels per side.
    /// </summary>
    public sealed class MprisNowPlayingService : INowPlayingService, IDisposable
    {
        private static readonly Logger logger = LogManager.GetCurrentClassLogger();
        private const int ArtMaxDimension = 300;
        private const int ArtCacheCapacity = 64;
        private static readonly HashSet<string> AudioExtensions = new(StringComparer.OrdinalIgnoreCase)
        {
            ".mp3", ".flac", ".ogg", ".oga", ".opus", ".m4a", ".m4b", ".aac", ".wav", ".wma", ".ape", ".alac",
            ".aiff", ".aif", ".wv", ".mka", ".mpc", ".dsf", ".dff", ".ac3", ".dts", ".mid", ".mod",
        };

        public event EventHandler<NowPlayingEventArgs> NowPlayingTrackChanged;

        public NowPlayingEventArgs CurrentTrack => Volatile.Read(ref model);

        private readonly object sync = new();
        private readonly SemaphoreSlim refreshGate = new(1, 1);
        private readonly HttpClient http = new() { Timeout = TimeSpan.FromSeconds(15) };
        private readonly Dictionary<string, string> artCache = new(StringComparer.Ordinal);
        private readonly Queue<string> artCacheOrder = new();
        private readonly Dictionary<string, TrackedPlayer> players = new(StringComparer.Ordinal);
        private Connection connection;
        private IDisposable ownerWatch;
        private NowPlayingEventArgs model;

        private sealed class TrackedPlayer
        {
            public string BusName { get; init; }
            public IMprisPlayer Player { get; init; }
            public IMprisMediaPlayer2 Root { get; init; }
            public IDisposable PropertiesWatch { get; set; }
        }

        /// <exception cref="InvalidOperationException">No session bus address is configured.</exception>
        public void Start()
        {
            var address = Address.Session
                ?? throw new InvalidOperationException("No D-Bus session bus address is configured (DBUS_SESSION_BUS_ADDRESS).");
            Connection conn;
            lock (sync)
            {
                if (connection != null)
                    return;
                conn = connection = new Connection(address);
            }
            try
            {
                Task.Run(() => StartAsync(conn)).GetAwaiter().GetResult();
            }
            catch
            {
                Stop();
                throw;
            }
        }

        public void Stop()
        {
            Connection conn;
            IDisposable watch;
            TrackedPlayer[] tracked;
            lock (sync)
            {
                conn = connection;
                connection = null;
                watch = ownerWatch;
                ownerWatch = null;
                tracked = players.Values.ToArray();
                players.Clear();
            }
            watch?.Dispose();
            foreach (var player in tracked)
                player.PropertiesWatch?.Dispose();
            conn?.Dispose();
        }

        public void Dispose()
        {
            Stop();
            http.Dispose();
        }

        private async Task StartAsync(Connection conn)
        {
            await conn.ConnectAsync().ConfigureAwait(false);
            var watch = await conn.ResolveServiceOwnerAsync(Mpris.BusNamePrefix + "*", OnOwnerChanged, OnOwnerWatchError).ConfigureAwait(false);
            lock (sync)
            {
                if (!ReferenceEquals(connection, conn))
                {
                    watch.Dispose();
                    return;
                }
                ownerWatch = watch;
            }

            var names = await conn.ListServicesAsync().ConfigureAwait(false);
            foreach (var name in names.Where(n => n.StartsWith(Mpris.BusNamePrefix, StringComparison.Ordinal)))
                await TrackPlayerAsync(conn, name).ConfigureAwait(false);
            await RefreshAsync(conn).ConfigureAwait(false);
        }

        private void OnOwnerChanged(ServiceOwnerChangedEventArgs e)
        {
            Connection conn;
            lock (sync)
                conn = connection;
            if (conn is null)
                return;

            if (string.IsNullOrEmpty(e.NewOwner))
            {
                TrackedPlayer removed;
                lock (sync)
                {
                    if (players.Remove(e.ServiceName, out removed))
                        logger.Info("MPRIS player {0} left the bus.", e.ServiceName);
                }
                removed?.PropertiesWatch?.Dispose();
                ScheduleRefresh(conn);
                return;
            }

            _ = Task.Run(async () =>
            {
                try
                {
                    await TrackPlayerAsync(conn, e.ServiceName).ConfigureAwait(false);
                    await RefreshAsync(conn).ConfigureAwait(false);
                }
                catch (Exception ex)
                {
                    LogRefreshFailure(conn, ex);
                }
            });
        }

        private void OnOwnerWatchError(Exception ex)
        {
            Connection conn;
            lock (sync)
                conn = connection;
            if (conn is null)
                logger.Debug(ex, "MPRIS owner watch ended after Stop().");
            else
                logger.Error(ex, "MPRIS owner watch failed; player appearances are no longer tracked until Start() is called again.");
        }

        private async Task TrackPlayerAsync(Connection conn, string busName)
        {
            TrackedPlayer tracked;
            lock (sync)
            {
                if (!ReferenceEquals(connection, conn) || players.ContainsKey(busName))
                    return;
                tracked = new TrackedPlayer
                {
                    BusName = busName,
                    Player = conn.CreateProxy<IMprisPlayer>(busName, Mpris.ObjectPath),
                    Root = conn.CreateProxy<IMprisMediaPlayer2>(busName, Mpris.ObjectPath),
                };
                players[busName] = tracked;
            }

            try
            {
                var watch = await tracked.Player.WatchPropertiesAsync(changes => OnPlayerPropertiesChanged(conn, busName, changes)).ConfigureAwait(false);
                var keep = false;
                lock (sync)
                {
                    if (ReferenceEquals(connection, conn) && players.TryGetValue(busName, out var current) && ReferenceEquals(current, tracked))
                    {
                        tracked.PropertiesWatch = watch;
                        keep = true;
                    }
                }
                if (!keep)
                {
                    watch.Dispose();
                    return;
                }

                string identity = null;
                try
                {
                    identity = await tracked.Root.GetAsync("Identity").ConfigureAwait(false) as string;
                }
                catch (DBusException ex)
                {
                    logger.Debug(ex, "{0} exposes no Identity.", busName);
                }
                logger.Info("Tracking MPRIS player {0} ({1}).", busName, identity ?? "unnamed");
            }
            catch (DBusException ex)
            {
                lock (sync)
                {
                    if (players.TryGetValue(busName, out var current) && ReferenceEquals(current, tracked))
                        players.Remove(busName);
                }
                logger.Warn(ex, "Could not subscribe to property changes of {0}; it probably left the bus.", busName);
            }
        }

        private void OnPlayerPropertiesChanged(Connection conn, string busName, PropertyChanges changes)
        {
            var relevant = changes.Changed.Any(c => c.Key is "Metadata" or "PlaybackStatus")
                || changes.Invalidated.Any(k => k is "Metadata" or "PlaybackStatus");
            if (!relevant)
                return;
            logger.Debug("{0} changed {1}.", busName, string.Join(", ", changes.Changed.Select(c => c.Key).Concat(changes.Invalidated)));
            ScheduleRefresh(conn);
        }

        private void ScheduleRefresh(Connection conn)
        {
            _ = Task.Run(async () =>
            {
                try
                {
                    await RefreshAsync(conn).ConfigureAwait(false);
                }
                catch (Exception ex)
                {
                    LogRefreshFailure(conn, ex);
                }
            });
        }

        private void LogRefreshFailure(Connection conn, Exception ex)
        {
            bool stale;
            lock (sync)
                stale = !ReferenceEquals(connection, conn);
            if (stale)
                logger.Debug(ex, "MPRIS refresh aborted because the service was stopped.");
            else
                logger.Error(ex, "MPRIS refresh failed.");
        }

        private async Task RefreshAsync(Connection conn)
        {
            await refreshGate.WaitAsync().ConfigureAwait(false);
            try
            {
                TrackedPlayer[] snapshot;
                lock (sync)
                {
                    if (!ReferenceEquals(connection, conn))
                        return;
                    snapshot = players.Values.OrderBy(p => p.BusName, StringComparer.Ordinal).ToArray();
                }

                var statuses = new List<(TrackedPlayer player, string status)>();
                foreach (var player in snapshot)
                {
                    try
                    {
                        var status = await player.Player.GetAsync("PlaybackStatus").ConfigureAwait(false) as string;
                        statuses.Add((player, status ?? string.Empty));
                    }
                    catch (DBusException ex)
                    {
                        logger.Debug(ex, "{0} did not answer PlaybackStatus; it is probably leaving the bus.", player.BusName);
                    }
                }

                if (statuses.Count == 0)
                {
                    SetModel(null);
                    return;
                }

                TrackedPlayer chosen = null;
                foreach (var wanted in new[] { "Playing", "Paused" })
                {
                    chosen = statuses.FirstOrDefault(s => s.status == wanted).player;
                    if (chosen != null)
                        break;
                }
                if (chosen is null)
                    return; // every player is stopped: keep the last known track, as the Windows service does.

                IDictionary<string, object> metadata;
                try
                {
                    metadata = await chosen.Player.GetAsync("Metadata").ConfigureAwait(false) as IDictionary<string, object>;
                }
                catch (DBusException ex)
                {
                    logger.Debug(ex, "{0} did not answer Metadata; it is probably leaving the bus.", chosen.BusName);
                    return;
                }
                metadata ??= new Dictionary<string, object>();

                var title = GetString(metadata, "xesam:title");
                if (string.IsNullOrEmpty(title))
                    return; // ignore if title is missing, as the Windows service does.

                var artist = JoinStrings(metadata, "xesam:artist");
                var thumbnail = await LoadArtAsync(GetString(metadata, "mpris:artUrl")).ConfigureAwait(false);

                var current = Volatile.Read(ref model);
                if (current is null
                    || title != current.Title || artist != current.Artist
                    || current.Thumbnail is null && thumbnail != null
                    || current.Thumbnail != null && thumbnail != null && !thumbnail.Equals(current.Thumbnail))
                {
                    var next = new NowPlayingEventArgs
                    {
                        AlbumArtist = JoinStrings(metadata, "xesam:albumArtist"),
                        AlbumTitle = GetString(metadata, "xesam:album"),
                        AlbumTrackCount = 0,
                        Artist = artist,
                        Genres = GetStrings(metadata, "xesam:genre")?.ToList(),
                        PlaybackType = PlaybackTypeFromUrl(GetString(metadata, "xesam:url")),
                        Subtitle = null,
                        Thumbnail = thumbnail,
                        Title = title,
                        TrackNumber = GetInt(metadata, "xesam:trackNumber"),
                    };
                    logger.Info("Now playing on {0}: {1} - {2}", chosen.BusName, artist, title);
                    SetModel(next);
                }
            }
            finally
            {
                refreshGate.Release();
            }
        }

        private void SetModel(NowPlayingEventArgs next)
        {
            lock (sync)
            {
                if (next is null && model is null)
                    return;
                model = next;
            }
            try
            {
                NowPlayingTrackChanged?.Invoke(this, next);
            }
            catch (Exception ex)
            {
                logger.Error(ex, "A NowPlayingTrackChanged handler threw.");
            }
        }

        #region metadata helpers

        private static string GetString(IDictionary<string, object> metadata, string key)
        {
            if (!metadata.TryGetValue(key, out var value) || value is null)
                return null;
            return value switch
            {
                string s => s,
                ObjectPath path => path.ToString(),
                IEnumerable<string> list => list.FirstOrDefault(),
                _ => value.ToString(),
            };
        }

        private static IEnumerable<string> GetStrings(IDictionary<string, object> metadata, string key)
        {
            if (!metadata.TryGetValue(key, out var value) || value is null)
                return null;
            return value switch
            {
                string s => new[] { s },
                IEnumerable<string> list => list,
                _ => new[] { value.ToString() },
            };
        }

        private static string JoinStrings(IDictionary<string, object> metadata, string key)
        {
            var values = GetStrings(metadata, key)?.Where(v => !string.IsNullOrEmpty(v)).ToArray();
            return values is null || values.Length == 0 ? null : string.Join(", ", values);
        }

        private static int GetInt(IDictionary<string, object> metadata, string key)
        {
            if (!metadata.TryGetValue(key, out var value) || value is null)
                return 0;
            return value switch
            {
                int i => i,
                long l => (int)Math.Clamp(l, int.MinValue, int.MaxValue),
                uint u => (int)Math.Min(u, int.MaxValue),
                short s => s,
                ushort us => us,
                byte b => b,
                string s when int.TryParse(s, out var parsed) => parsed,
                _ => 0,
            };
        }

        /// <summary>
        /// Mirrors the Windows PlaybackType strings ("Music", "Video", "Image", "Unknown") using the track URL's extension;
        /// MPRIS carries no media class of its own.
        /// </summary>
        public static string PlaybackTypeFromUrl(string url)
        {
            if (string.IsNullOrEmpty(url))
                return "Unknown";
            if (url.StartsWith("spotify:", StringComparison.OrdinalIgnoreCase))
                return "Music";

            string path;
            if (Uri.TryCreate(url, UriKind.Absolute, out var uri))
            {
                if (uri.Host.EndsWith("spotify.com", StringComparison.OrdinalIgnoreCase))
                    return "Music";
                path = uri.IsFile ? uri.LocalPath : uri.AbsolutePath;
            }
            else
            {
                path = url;
            }

            var extension = Path.GetExtension(path);
            if (extension.Length == 0)
                return "Unknown";
            if (AudioExtensions.Contains(extension))
                return "Music";
            return FileTypes.GetFileType(path) switch
            {
                WallpaperType.video => "Video",
                WallpaperType.picture => "Image",
                WallpaperType.gif => "Image",
                _ => "Unknown",
            };
        }

        #endregion

        #region album art

        private async Task<string> LoadArtAsync(string artUrl)
        {
            if (string.IsNullOrEmpty(artUrl))
                return null;
            try
            {
                var uri = new Uri(artUrl);
                switch (uri.Scheme)
                {
                    case "file":
                        return EncodePng(await File.ReadAllBytesAsync(uri.LocalPath).ConfigureAwait(false));
                    case "http":
                    case "https":
                        {
                            lock (artCache)
                            {
                                if (artCache.TryGetValue(artUrl, out var cached))
                                    return cached;
                            }
                            var encoded = EncodePng(await http.GetByteArrayAsync(uri).ConfigureAwait(false));
                            lock (artCache)
                            {
                                if (artCache.TryAdd(artUrl, encoded))
                                {
                                    artCacheOrder.Enqueue(artUrl);
                                    while (artCacheOrder.Count > ArtCacheCapacity)
                                        artCache.Remove(artCacheOrder.Dequeue());
                                }
                            }
                            return encoded;
                        }
                    case "data":
                        return EncodePng(DecodeDataUri(artUrl));
                    default:
                        logger.Warn("Album art URL scheme '{0}' is not supported: {1}", uri.Scheme, artUrl);
                        return null;
                }
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or HttpRequestException
                                       or TaskCanceledException or UriFormatException or FormatException or MagickException)
            {
                logger.Warn(ex, "Album art could not be loaded from {0}.", artUrl);
                return null;
            }
        }

        private static byte[] DecodeDataUri(string dataUri)
        {
            var comma = dataUri.IndexOf(',');
            if (comma < 0)
                throw new FormatException("data: URI has no payload separator.");
            var header = dataUri[..comma];
            if (!header.EndsWith(";base64", StringComparison.OrdinalIgnoreCase))
                throw new FormatException("Only base64 data: URIs are supported for album art.");
            return Convert.FromBase64String(dataUri[(comma + 1)..]);
        }

        private static string EncodePng(byte[] imageBytes)
        {
            using var image = new MagickImage(imageBytes);
            image.AutoOrient();
            if (image.Width > ArtMaxDimension || image.Height > ArtMaxDimension)
                image.Resize(new MagickGeometry(ArtMaxDimension, ArtMaxDimension));
            return Convert.ToBase64String(image.ToByteArray(MagickFormat.Png));
        }

        #endregion
    }
}
