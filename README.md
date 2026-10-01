<p align="center">
  <img alt="Deadly Wallpaper logo" src="assets/logo.png" width="300" />
  <h2 align="center">Deadly Wallpaper</h2>
</p>

A spitefully ported "re-imagining" of the Windows desktop application "Lively Wallpaper", but better and built for Linux, Mac, and Windows.

## Install

| Channel | How |
| --- | --- |
| Arch Linux (AUR) | `yay -S deadlywp-bin` |
| Cargo | `cargo install deadlywp` |
| Binaries | [Releases](https://github.com/nickheyer/deadlywallpaper/releases) |
| Source | `make build && make install` |

## Deps

It's possible you will need a library called `libmpv` in order to play video wallpapers.
On mac, `brew install mpv`.
On windows, `libmpv-2.dll` next to `deadlywp.exe` on Windows.

## Commands

```
# Build
make build
make install
make test
make check-all

# Run
deadlywp daemon                  run the daemon in the foreground
deadlywp status                  daemon state and what plays where
deadlywp list                    library wallpapers
deadlywp displays                connected displays
deadlywp set <target> [-d N]     apply a library id, file, folder, URL, `random`, or `reload`
deadlywp close [-d N]            stop one display, or all
deadlywp layout per|span|duplicate
deadlywp align image|<display> [--x N --y N --scale S --rotate D] [--reset]
deadlywp volume <0-100|+N|-N>
deadlywp <play|pause>
deadlywp seek <0-100|+N|-N>           media wallpapers
deadlywp prop name=value [-d N]  change a wallpaper property (++n / --n for relative)
deadlywp screenshot out.png      capture a running wallpaper
deadlywp import <source>         add a file, a folder of wallpapers, a Lively .zip, a URL,
                                 a Wallpaper Engine project folder, or a Steam Workshop item
deadlywp export <id> out.zip     write a Lively package
deadlywp delete <id>
deadlywp workshop search [words] [--sort trend|recent|updated|subscribers|rated]
                                 [--type scene|video|web|application] [--days N] [--mature] [--page N]
deadlywp workshop show <id|url>  details of one Workshop item
deadlywp workshop get <id|url> [-d N]
                                 fetch an item through Steam and add it; -d applies it there
deadlywp workshop sync           add every item Steam has downloaded, refresh updated ones
deadlywp workshop status         Steam, Wallpaper Engine and every downloaded item
deadlywp workshop forget <id>    stop waiting for a download
deadlywp quit
```

## Wallpaper Engine

Deadly Wallpaper runs Wallpaper Engine wallpapers and browses the Steam Workshop for them.

- **Scene wallpapers** (`scene.pkg` / `scene.json`) are drawn by the built-in WebGL renderer:
  images, effects and shaders, particles, text, sounds, puppet warp, lighting, parallax,
  bloom, audio-reactive uniforms and SceneScript. They need Wallpaper Engine's `assets`
  folder, taken from the Steam install of Wallpaper Engine or set in Settings → Wallpaper
  Engine.
- **Video** and **web** wallpapers play natively. Web wallpapers get Wallpaper Engine's page
  API: `wallpaperPropertyListener` (user and general properties, pause, directory files),
  `wallpaperRegisterAudioListener` (64 left + 64 right bands), random files for directory
  properties, `file:///` access to the files a property points at, and the media
  integration listeners.
- **Application** wallpapers run on Windows, where they are Windows programs.
- **Properties** from `project.json` (sliders, colours, combos, text, files, directories,
  texture pickers, display conditions, localisation) appear in Customize.
- **Audio** reaches Wallpaper Engine wallpapers as the 64-band left/right spectrum they
  expect, next to the 128-bin feed Lively visualizers use.
- **Media integration** tells wallpapers what the system is playing (title, artist, album
  art with derived colours, playback state, position): MPRIS players on Linux, the system
  media session on Windows, Music and Spotify on macOS. Settings → Wallpaper Engine turns
  it off.

### Steam Workshop

The Workshop page in the app (and `deadlywp workshop`) searches, sorts and filters the
Workshop for Wallpaper Engine without a Steam login. Downloads go through the Steam client:
"Get" opens the item's Steam page, you subscribe there, and the daemon adds the item to the
library as soon as Steam finishes downloading it (Wallpaper Engine has to be owned and
installed in that Steam account). Items already downloaded by Steam are added directly, and
"Sync downloads" (or the auto-import setting) brings in everything Steam has and refreshes
what Steam updated. Any item id or Workshop URL works with `import` and `set` too.

