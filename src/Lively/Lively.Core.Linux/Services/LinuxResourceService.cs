using Lively.Common;
using Lively.Common.Services;
using Lively.Models.Enums;
using System;
using System.Globalization;
using System.Reflection;
using System.Resources;

namespace Lively.Core.Linux.Services
{
    /// <summary>
    /// Localized strings for the core, read from the resx files shared with the Windows core
    /// (linked into this assembly as Lively.Core.Linux.Properties.Resources).
    /// </summary>
    public sealed class LinuxResourceService : IResourceService
    {
        private static readonly NLog.Logger Logger = NLog.LogManager.GetCurrentClassLogger();
        private readonly ResourceManager resources = new ResourceManager("Lively.Core.Linux.Properties.Resources", Assembly.GetExecutingAssembly());

        public event EventHandler<string> CultureChanged;

        public string GetString(string resource)
        {
            // Same key normalisation as the Windows core: "Cancel/Content" -> "Cancel.Content".
            var key = resource.Replace("/", ".").Replace("_", ".");
            var value = resources.GetString(key, CultureInfo.CurrentUICulture);
            if (value != null)
                return value;
            Logger.Warn($"Missing string resource: {resource}");
            return resource;
        }

        public string GetString(WallpaperType type) => type switch
        {
            WallpaperType.app => GetString("TextApplication"),
            WallpaperType.unity => "Unity",
            WallpaperType.godot => "Godot",
            WallpaperType.video => GetString("TextVideo"),
            WallpaperType.gif => "Gif",
            WallpaperType.web => GetString("TextWebsite"),
            WallpaperType.webaudio => GetString("TitleAudio"),
            WallpaperType.url => GetString("TextWebsite"),
            WallpaperType.videostream => GetString("TextWebStream"),
            WallpaperType.bizhawk => "Bizhawk",
            WallpaperType.unityaudio => "Unity",
            WallpaperType.picture => GetString("TextPicture"),
            _ => GetString("TextError"),
        };

        public void SetCulture(string name)
        {
            try
            {
                var culture = string.IsNullOrEmpty(name) ? CultureInfo.InstalledUICulture : new CultureInfo(name);
                CultureInfo.DefaultThreadCurrentUICulture = culture;
                CultureInfo.CurrentUICulture = culture;
                CultureChanged?.Invoke(this, name);
            }
            catch (CultureNotFoundException ex)
            {
                Logger.Error($"Unknown culture '{name}': {ex.Message}");
            }
        }

        public void SetSystemDefaultCulture() => SetCulture(string.Empty);
    }
}
