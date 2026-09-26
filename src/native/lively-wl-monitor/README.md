# lively-wl-monitor

Native helper for the Lively Wallpaper Linux port. It connects to the Wayland
display and reports, as JSON lines on stdout, the outputs (from `wl_output` and
`zxdg_output_v1`) and the foreign toplevel windows (from
`zwlr_foreign_toplevel_manager_v1` on wlroots compositors or
`org_kde_plasma_window_management` on KWin). It renders nothing. The message
format is the contract in [`../PROTOCOL.md`](../PROTOCOL.md), section 6.

## Build

Requirements: a C11 compiler, `pkg-config`, `wayland-client` (with headers),
`wayland-scanner`, GNU make; `python3` for `make test`.

```
make                      # builds build/lively-wl-monitor
make BUILD_DIR=out        # build somewhere else (PREFIX is accepted and unused)
make clean
```

The protocol bindings are generated at build time with `wayland-scanner` from
`../protocols/xdg-output-unstable-v1.xml`,
`../protocols/wlr-foreign-toplevel-management-unstable-v1.xml`,
`../protocols/plasma-window-management.xml` and
`../protocols/plasma-virtual-desktop.xml` into `$(BUILD_DIR)`.
`zwlr_layer_shell_v1` and `org_kde_plasma_shell` are only detected by name in
the registry, so no bindings are generated for them. The code is compiled with
`-std=c11 -Wall -Wextra -O2` and links against `pkg-config --libs wayland-client`.

## Usage

```
lively-wl-monitor [--once]
```

Start-up: one registry roundtrip, then the globals are bound (`wl_output` v4,
`zxdg_output_manager_v1` v3, and `zwlr_foreign_toplevel_manager_v1` v3 or, when
that is absent, `org_kde_plasma_window_management` up to v21 together with
`org_kde_plasma_virtual_desktop_management` up to v2), the `capabilities` line
is printed, and further roundtrips run until every output, xdg-output, virtual
desktop and window has delivered its initial state. Then `outputs` is printed,
followed by `toplevels` when a toplevel protocol exists.

* `--once`: exit 0 right after those lines.
* Without `--once`: keep running in a `poll()` loop and re-emit `outputs`
  after every `wl_output.done` / `zxdg_output_v1.done` batch and on output
  removal, and the complete `toplevels` list after any window change, after
  the current virtual desktop changed and after the show-desktop state changed.
  Events arriving in one dispatch batch produce one line. Every line is flushed
  immediately. The process exits 0 on stdin EOF (also when fd 0 is closed),
  SIGTERM and SIGINT. Data written to stdin is discarded.

Exit codes: `0` finished, `1` runtime failure (no Wayland display, connection
lost, stdout closed, out of memory), `2` bad arguments, `3` the compositor does
not provide `zxdg_output_manager_v1` (or provides neither `wl_output` v4 nor
`zxdg_output_v1` v2, so outputs would have no names).

### Output fields

`outputs`: `name` and `description` come from `wl_output` v4 (falling back to
the `zxdg_output_v1` v2 events), `make`, `model`, `transform`,
`physical_width_mm`, `physical_height_mm` from `wl_output.geometry`,
`refresh_mhz` from the current `wl_output.mode`, `x`, `y`, `width`, `height`
from `zxdg_output_v1.logical_position` / `logical_size`, and `scale` is the
current mode's pixel width divided by the logical width (the mode is swapped
for 90/270 degree transforms), printed with up to three decimals. An output is
listed once its `done` arrived and it has a name, a current mode and a
non-empty logical size.

`toplevels`: `id` is the wl proxy id of the `zwlr_foreign_toplevel_handle_v1`
or the internal window id from `org_kde_plasma_window_management`, as a decimal
string, stable for the window's lifetime. `activated`, `fullscreen`,
`maximized`, `minimized` come from the wlr `state` array or the Plasma `state`
bitfield (`active`, `fullscreen`, `maximized`, `minimized`). `skip_taskbar` is
the Plasma `skiptaskbar` flag; wlr only lists taskbar windows, so it is `false`
there. Every window is listed, including those with `skip_taskbar` true.
`outputs` comes from `output_enter` / `output_leave` on wlr and, on Plasma, is
the set of outputs whose logical rectangle intersects the window `geometry`.
`geometry` is `[x,y,w,h]` from the Plasma `geometry` event and `null` on wlr.
`virtual_desktops` and `activities` are the ids collected from the Plasma
`virtual_desktop_entered` / `virtual_desktop_left` and `activity_entered` /
`activity_left` events (empty means "on all"); `on_current_desktop` is `true`
when the window is on all desktops or on a desktop that
`org_kde_plasma_virtual_desktop_management` currently reports as `activated`.
The `toplevels` event itself carries `show_desktop`, KWin's show-desktop mode
from `show_desktop_changed`. On wlr the lists are empty, `on_current_desktop`
is `true` and `show_desktop` is `false`, because that protocol has no such
state. A wlr handle is listed from its first `done`, a Plasma window from
`initial_state`; a window disappears on `closed` / `unmapped`. Strings are
UTF-8 with JSON escaping; malformed UTF-8 from a client is replaced by U+FFFD.

## KDE Plasma: registering the binary with KWin

KWin only exposes `org_kde_plasma_window_management` to a client whose
executable has a `.desktop` file with `Exec=` set to the binary's canonical
path and `X-KDE-Wayland-Interfaces=org_kde_plasma_window_management`
(`org.kde.plasmashell.desktop` and `org.kde.spectacle.desktop` use the same
mechanism). Any other client sees the global filtered out and gets
`"toplevel_protocol":"none"`. KWin finds the file through the ksycoca service
cache, whose file name depends on `XDG_DATA_DIRS` / `XDG_DATA_HOME` of the
session, and reloads that cache asynchronously.

```
make register-kde     # writes ~/.local/share/applications/lively-wl-monitor-<hash>.desktop
                      # for $(BUILD_DIR)/lively-wl-monitor, rebuilds the cache with the
                      # session's data dirs, waits until KWin exposes the interface
make unregister-kde   # removes that file and rebuilds the cache
```

`kde-register.sh` implements both. The file is named after the binary path
(`<hash>` is the first eight hex digits of the SHA-1 of the canonical path) and
its content is exactly what the core's `KdeInterfaceRegistration` writes for
the binary it runs, so a build directory, `build/dist` and an installed copy
are registered side by side and neither the script nor the core ever rewrites
the other's file. A binary at a path that has no such file is reported
`"toplevel_protocol":"none"` by KWin.

## Tests

`make test` runs `$(BUILD_DIR)/lively-wl-monitor --once`, validates every line
with `python3 -c 'import json,sys; [json.loads(l) for l in sys.stdin]'`, then
runs `test_once.py`, which checks the line order, the field types, that the
outputs event lists `DP-4` at `0,136 2648x1490` (scale within 0.01 of 1.45,
transform 0) and `DP-3` at `2649,0 1080x1920` (scale 1), and that the toplevels
event exists with at least one window with `"activated":true`. The expected
outputs are those of the development session (KDE Plasma 6.7.5, KWin
advertising `org_kde_plasma_window_management` v20); `zxdg_output_v1` reports
DP-4's logical width as 2648 (`kscreen-doctor` rounds the same 3840 / 1.45 up
to 2649).

Commands run on that session, in `src/native/lively-wl-monitor`:

```
$ make
$ make register-kde
registered /home/nick/.local/share/applications/lively-wl-monitor.desktop: KWin exposes org_kde_plasma_window_management to /home/nick/code/deadlywallpaper/src/native/lively-wl-monitor/build/lively-wl-monitor
$ make test
build/lively-wl-monitor --once > build/once.jsonl
python3 -c 'import json,sys; [json.loads(l) for l in sys.stdin]' < build/once.jsonl
python3 test_once.py < build/once.jsonl
OK: protocol=plasma outputs=['DP-3', 'DP-4'] toplevels=6 active=code-oss (export_test.go - nebu - Code - OSS)
```

Sample `--once` output from that session (the toplevels line is shortened to
two of the six windows):

```
{"event":"capabilities","layer_shell":true,"plasma_shell":true,"toplevel_protocol":"plasma"}
{"event":"outputs","outputs":[{"name":"DP-3","description":"Acer Technologies Acer XF270H","make":"Acer Technologies","model":"Acer XF270H","x":2649,"y":0,"width":1080,"height":1920,"scale":1,"transform":1,"physical_width_mm":598,"physical_height_mm":336,"refresh_mhz":144001},{"name":"DP-4","description":"Microstep DP-4-MPG321CX OLED","make":"Microstep","model":"DP-4-MPG321CX OLED","x":0,"y":136,"width":2648,"height":1490,"scale":1.45,"transform":0,"physical_width_mm":699,"physical_height_mm":395,"refresh_mhz":239998}]}
{"event":"toplevels","show_desktop":false,"toplevels":[{"id":"5","app_id":"code-oss","title":"export_test.go - nebu - Code - OSS","activated":true,"fullscreen":false,"maximized":true,"minimized":false,"skip_taskbar":false,"outputs":["DP-4"],"geometry":[0,136,2648,1446],"virtual_desktops":["9d3a6c2e-1b0f-4c7a-9e5d-2f6b8a1c3d4e"],"activities":[],"on_current_desktop":true},{"id":"43","app_id":"org.kde.konsole","title":"~ : zsh — Konsole","activated":false,"fullscreen":false,"maximized":false,"minimized":false,"skip_taskbar":false,"outputs":["DP-4"],"geometry":[331,136,1986,1446],"virtual_desktops":["9d3a6c2e-1b0f-4c7a-9e5d-2f6b8a1c3d4e"],"activities":[],"on_current_desktop":true}]}
```

Live mode was checked by starting the binary with stdin held open for five
seconds (it stayed alive and printed the three start-up lines, followed by one
`toplevels` line per dispatch batch in which a window changed on the desktop:
eight during a run while windows were being used, none during an idle run),
closing stdin (exit code 0 within 1 ms), and separately sending SIGTERM (exit
code 0 within 1 ms). Every line of every run parsed as JSON.
`WAYLAND_DEBUG=1 build/lively-wl-monitor --once` shows the wire traffic when
something looks off.

## Files

* `main.c`: the program.
* `Makefile`: build, `test`, `register-kde`, `unregister-kde`.
* `test_once.py`: content checks for `make test`, including the desktop and
  activity fields and the `show_desktop` flag.
* `kde-register.sh`: KWin registration used by the two `*-kde` targets; writes
  the same file as the core's `KdeInterfaceRegistration`.
