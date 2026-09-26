using System;
using System.Collections.Generic;

namespace Lively.Core.Linux.Wallpapers
{
    /// <summary>
    /// The per-wallpaper start options a web wallpaper can request through LivelyInfo.json
    /// "arguments" (e.g. "--system-information true --pause-event true"). The Windows player
    /// accepts them with or without the "--wallpaper-" prefix; so do we.
    /// </summary>
    public sealed class WallpaperArguments
    {
        public bool SystemInformation { get; private set; }
        public bool NowPlaying { get; private set; }
        public bool PauseEvent { get; private set; }
        public bool PauseMedia { get; private set; }
        public bool VerboseLog { get; private set; }

        public static WallpaperArguments Parse(string arguments)
        {
            var result = new WallpaperArguments();
            if (string.IsNullOrWhiteSpace(arguments))
                return result;

            var tokens = arguments.Split(' ', StringSplitOptions.RemoveEmptyEntries);
            for (var i = 0; i < tokens.Length; i++)
            {
                var token = tokens[i];
                if (!token.StartsWith("--", StringComparison.Ordinal))
                    continue;

                var name = token.Substring(2);
                if (name.StartsWith("wallpaper-", StringComparison.Ordinal))
                    name = name.Substring("wallpaper-".Length);

                var value = true;
                if (i + 1 < tokens.Length && !tokens[i + 1].StartsWith("--", StringComparison.Ordinal))
                {
                    value = !string.Equals(tokens[i + 1], "false", StringComparison.OrdinalIgnoreCase);
                    i++;
                }

                switch (name)
                {
                    case "system-information": result.SystemInformation = value; break;
                    case "system-nowplaying": result.NowPlaying = value; break;
                    case "pause-event": result.PauseEvent = value; break;
                    case "pause-media": result.PauseMedia = value; break;
                    case "verbose-log": result.VerboseLog = value; break;
                }
            }
            return result;
        }
    }
}
