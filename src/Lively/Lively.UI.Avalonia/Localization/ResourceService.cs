using Lively.Common.Services;
using Lively.Models.Enums;
using System;
using System.Globalization;
using System.Resources;

namespace Lively.UI.Avalonia.Localization
{
    /// <summary>
    /// Serves the strings embedded from Strings/Resources*.resx (synchronised from the WinUI .resw tables) and
    /// Strings/Platform.resx (Linux-only strings) through <see cref="ResourceManager"/>s.
    /// Keys are accepted in the WinUI x:Uid form (Cancel/Content) and normalised to the resx form (Cancel.Content).
    /// </summary>
    public class ResourceService : IResourceService
    {
        public event EventHandler<string> CultureChanged;

        private readonly ResourceManager sharedResources;
        private readonly ResourceManager platformResources;
        private readonly CultureInfo systemDefaultCulture;
        private CultureInfo culture;

        public ResourceService()
        {
            sharedResources = new ResourceManager("Lively.UI.Avalonia.Strings.Resources", typeof(ResourceService).Assembly);
            platformResources = new ResourceManager("Lively.UI.Avalonia.Strings.Platform", typeof(ResourceService).Assembly);
            systemDefaultCulture = CultureInfo.CurrentUICulture;
            culture = systemDefaultCulture;
        }

        /// <summary>
        /// Culture currently used to resolve strings.
        /// </summary>
        public CultureInfo CurrentCulture => culture;

        public string GetString(string resource)
        {
            if (string.IsNullOrEmpty(resource))
                return string.Empty;

            // Compatibility with UWP .resw shared classes and WinUI x:Uid keys.
            var key = resource.Replace("/", ".").Replace("_", ".");
            return sharedResources.GetString(key, culture) ?? platformResources.GetString(key, culture) ?? resource;
        }

        public string GetString(WallpaperType type)
        {
            return type switch
            {
                WallpaperType.app => GetString("TextApplication"),
                WallpaperType.unity => "Unity",
                WallpaperType.godot => "Godot",
                WallpaperType.unityaudio => "Unity",
                WallpaperType.bizhawk => "Bizhawk",
                WallpaperType.web => GetString("Website/Header"),
                WallpaperType.webaudio => GetString("AudioGroup/Header"),
                WallpaperType.url => GetString("Website/Header"),
                WallpaperType.video => GetString("TextVideo"),
                WallpaperType.gif => "Gif",
                WallpaperType.videostream => GetString("TextWebStream"),
                WallpaperType.picture => GetString("TextPicture"),
                (WallpaperType)(100) => "Lively Wallpaper",
                _ => GetString("TextError"),
            };
        }

        public void SetCulture(string name)
        {
            CultureInfo newCulture;
            try
            {
                newCulture = string.IsNullOrEmpty(name) ? systemDefaultCulture : new CultureInfo(name);
            }
            catch (CultureNotFoundException)
            {
                // Unknown culture code in the settings file, keep using the current one.
                return;
            }

            if (string.Equals(culture.Name, newCulture.Name, StringComparison.OrdinalIgnoreCase) && CultureInfo.DefaultThreadCurrentUICulture != null)
                return;

            culture = newCulture;
            CultureInfo.DefaultThreadCurrentCulture = newCulture;
            CultureInfo.DefaultThreadCurrentUICulture = newCulture;
            CultureInfo.CurrentCulture = newCulture;
            CultureInfo.CurrentUICulture = newCulture;

            CultureChanged?.Invoke(this, newCulture.Name);
        }

        public void SetSystemDefaultCulture()
        {
            SetCulture(string.Empty);
        }
    }
}
