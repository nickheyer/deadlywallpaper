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
                                 have Steam download an item and add it; -d applies it there
deadlywp workshop sync           download every subscription Steam has not fetched
deadlywp workshop status         Steam, Wallpaper Engine, downloads under way, downloaded items
deadlywp workshop cancel <id>    stop an item's download
deadlywp quit
```

## Wallpaper Engine

Deadly Wallpaper runs Wallpaper Engine wallpapers and browses the Steam Workshop for them.

- **Scene wallpapers** (`scene.pkg` / `scene.json`) are drawn by the built-in WebGL renderer.
- **Video** and **web** wallpapers play natively.
- **Media integration** tells wallpapers what the system is playing.

### Steam Workshop

The Workshop page in the app (and `deadlywp workshop`) searches, sorts and filters the
Workshop for Wallpaper Engine without a Steam login. "Download" tells the running Steam client
to subscribe to the item and fetch it, the way Wallpaper Engine itself would.

