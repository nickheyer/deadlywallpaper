<p align="center">
  <img alt="Deadly Wallpaper logo" src="assets/logo.png" width="300" />
  <h2 align="center">Deadly Wallpaper</h2>
</p>

Live wallpapers for Linux, macOS and Windows: videos, GIFs, pictures, web pages, audio
visualizers and video streams on the desktop, behind your icons and windows. A re-imagining of
Lively Wallpaper in Rust; it plays Lively's wallpaper packages and honours their
`LivelyProperties.json` controls.

## How it presents wallpapers

| Desktop | Presenter |
| --- | --- |
| KDE Plasma (Wayland and X11) | A Plasma wallpaper plugin, installed automatically, that plasmashell renders beneath its own icons and widgets. The daemon drives it through plasmashell's scripting interface, follows your activities and virtual desktops, and hands each screen back to the wallpaper it had before when you close a live wallpaper. |
| Wayland compositors with `wlr-layer-shell` (Sway, Hyprland, River, niri, …) | A background layer surface per display, rendered by libmpv and WebKitGTK. |
| Other X11 desktops | A keep-below window per display, on every workspace, rendered by libmpv and WebKitGTK. |
| Windows | Surfaces under Explorer's WorkerW, behind the desktop icons. |
| macOS | Desktop-level windows below the Finder icons. |

Playback rules pause wallpapers under fullscreen or covering windows, on battery, while the
session is locked, or while chosen applications run. On KWin the window monitor is a KWin
script; other desktops use EWMH or the wlroots foreign-toplevel protocol.

## Requirements on Linux

- GTK 3, WebKitGTK 4.1 and libmpv (`mpv`) for the layer-shell and X11 presenters and for
  thumbnails.
- On KDE Plasma 6: Qt Multimedia with the FFmpeg backend for video (`qt6-multimedia-ffmpeg` on
  Arch) and Qt WebEngine (`qt6-webengine`) for web wallpapers.
- `yt-dlp` for video stream links.

## Build and install

```sh
make build        # release binary in build/target/release/deadlywp
make install      # ~/.local/bin/deadlywp plus a desktop entry and icon
make test
make check-all    # type-check the Windows and macOS targets as well
```

`deadlywp` (or the desktop entry) opens the control window and starts the daemon when needed.
The daemon registers itself to start at login unless you turn that off in Settings.

## Command line

```
deadlywp daemon                  run the daemon in the foreground
deadlywp status                  daemon state and what plays where
deadlywp list                    library wallpapers
deadlywp displays                connected displays
deadlywp set <target> [-d N]     apply a library id, file, folder, URL, `random`, or `reload`
deadlywp close [-d N]            stop one display, or all
deadlywp layout per|span|duplicate
deadlywp volume 40 | +10 | -10
deadlywp play | pause
deadlywp seek 50 | +10           media wallpapers
deadlywp prop name=value [-d N]  change a wallpaper property (++n / --n for relative)
deadlywp screenshot out.png      capture a running wallpaper
deadlywp import <source>         add a file, folder, Lively .zip, or URL to the library
deadlywp export <id> out.zip     write a Lively package
deadlywp delete <id>
deadlywp quit
```

Configuration lives in `~/.config/deadlywp`, the library in `~/.local/share/deadlywp/library`,
and the log in `~/.cache/deadlywp/deadlywp.log`.
