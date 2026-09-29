<p align="center">
  <img alt="Deadly Wallpaper logo" src="assets/logo.png" width="300" />
  <h2 align="center">Deadly Wallpaper</h2>
</p>

A spitefully ported "re-imagining" of the Windows desktop application "Lively Wallpaper", but better and built for Linux, Mac, and Windows.

## Commands

```
# dev
make build
make install
make test
make check-all

# application
deadlywp daemon                  run the daemon in the foreground
deadlywp status                  daemon state and what plays where
deadlywp list                    library wallpapers
deadlywp displays                connected displays
deadlywp set <target> [-d N]     apply a library id, file, folder, URL, `random`, or `reload`
deadlywp close [-d N]            stop one display, or all
deadlywp layout per|span|duplicate
deadlywp align image|<display> [--x N --y N --scale S --rotate D] [--reset]
deadlywp volume 40 | +10 | -10
deadlywp play | pause
deadlywp seek 50 | +10           media wallpapers
deadlywp prop name=value [-d N]  change a wallpaper property (++n / --n for relative)
deadlywp screenshot out.png      capture a running wallpaper
deadlywp import <source>         add a file, a folder of wallpapers, a Lively .zip, or a URL
deadlywp export <id> out.zip     write a Lively package
deadlywp delete <id>
deadlywp quit
```

