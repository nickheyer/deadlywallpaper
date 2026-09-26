using Lively.Models;
using Lively.Models.Enums;
using Lively.Models.Message;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace Lively.Core.Linux.Wallpapers
{
    /// <summary>
    /// One logical wallpaper made of several per-output instances. Used for the "span"
    /// arrangement: Wayland surfaces belong to a single output, so a wallpaper stretched over
    /// every display is one host per output, each showing its slice.
    /// </summary>
    public sealed class CompositeWallpaper : IWallpaper
    {
        private readonly List<IWallpaper> parts;
        private bool exitedRaised;

        public event EventHandler Exited;
        public event EventHandler Loaded;

        public IReadOnlyList<IWallpaper> Parts => parts;
        public bool IsExited => parts.Any(p => p.IsExited);
        public bool IsLoaded => parts.All(p => p.IsLoaded);
        public WallpaperType Category => Model.LivelyInfo.Type;
        public LibraryModel Model { get; }
        public IntPtr Handle => IntPtr.Zero;
        public IntPtr InputHandle => IntPtr.Zero;
        public int? Pid => null;
        public DisplayMonitor Screen { get; set; }
        public string LivelyPropertyCopyPath { get; }

        public CompositeWallpaper(LibraryModel model, DisplayMonitor screen, string livelyPropertyCopyPath, IEnumerable<IWallpaper> parts)
        {
            Model = model;
            Screen = screen;
            LivelyPropertyCopyPath = livelyPropertyCopyPath;
            this.parts = parts.ToList();
            if (this.parts.Count == 0)
                throw new ArgumentException("A composite wallpaper needs at least one part.", nameof(parts));

            foreach (var part in this.parts)
            {
                part.Exited += (s, e) =>
                {
                    if (exitedRaised)
                        return;
                    exitedRaised = true;
                    Exited?.Invoke(this, EventArgs.Empty);
                };
            }
        }

        public async Task ShowAsync()
        {
            // Start every slice; if one fails, tear the others down so nothing is left half shown.
            try
            {
                await Task.WhenAll(parts.Select(p => p.ShowAsync()));
            }
            catch
            {
                foreach (var part in parts)
                    part.Terminate();
                throw;
            }
            Loaded?.Invoke(this, EventArgs.Empty);
        }

        public void Pause() { foreach (var p in parts) p.Pause(); }
        public void Play() { foreach (var p in parts) p.Play(); }
        public void Close() { foreach (var p in parts) p.Close(); }
        public void Terminate() { foreach (var p in parts) p.Terminate(); }
        public void SendMessage(IpcMessage obj) { foreach (var p in parts) p.SendMessage(obj); }
        public void SetVolume(int volume) { foreach (var p in parts) p.SetVolume(volume); }

        public void SetMute(bool mute)
        {
            // Only the primary slice plays audio; the rest would double the sound.
            for (var i = 0; i < parts.Count; i++)
                parts[i].SetMute(mute || i != 0);
        }

        public void SetPlaybackPos(float pos, PlaybackPosType type) { foreach (var p in parts) p.SetPlaybackPos(pos, type); }

        public Task ScreenCapture(string filePath) => parts[0].ScreenCapture(filePath);

        public void Dispose() { foreach (var p in parts) p.Dispose(); }
    }
}
