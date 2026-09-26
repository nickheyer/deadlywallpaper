# lively-mpv-host

Native Linux wallpaper renderer for the Lively Wallpaper Linux port. It plays a video, gif,
still image or stream with libmpv into a `wlr-layer-shell` surface on one Wayland output and
speaks the JSON line protocol described in `../PROTOCOL.md` (sections 1 to 4) on stdin/stdout.

Files:

| File        | Contents |
|-------------|----------|
| `main.c`    | argument parsing, signal handling, the single `poll()` loop over the Wayland fd, stdin and the mpv wakeup pipe |
| `wl.c`      | registry binding, output matching by name (`wl_output.name` / `zxdg_output_v1.name`), layer surface, EGL/GLES context, frame-callback paced rendering |
| `player.c`  | libmpv setup with the OpenGL render API, event handling, scaler and span maths, `LivelyProperties.json`, screenshots, `host_mpv_command` |
| `ipc.c`     | stdout messages (`msg_hwnd`, `msg_console`, `msg_wploaded`, `msg_screenshot`) and the stdin dispatcher for every `cmd_*`, `lp_*` and Type 100 message |
| `test.sh`   | live test used by `make test` |

## Build

Dependencies (pkg-config names): `wayland-client`, `wayland-egl`, `egl`, `glesv2`, `mpv`; plus
`wayland-scanner` and the vendored `../protocols/*.xml` and `../third_party/cJSON.{c,h}`.

```
make                 # builds build/lively-mpv-host
make BUILD_DIR=out   # different build directory
make clean
```

`PREFIX` is accepted for the top-level build and unused here.

## Usage

```
lively-mpv-host --output DP-3 [--namespace NAME] [--layer background|bottom]
                [--span X,Y,W,H,VW,VH] [--interactive] [--property FILE] [--volume N] [--verbose]
                [--hwdec auto-safe|no] [--image] [--scaler none|fill|uniform|uniformFill]
                [--ytdl-format FMT] [--config-dir DIR] [--speed F] <file-or-url>
lively-mpv-host --windowed WIDTHxHEIGHT [--title TEXT] [same options] <file-or-url>
```

`--windowed WIDTHxHEIGHT` is the preview mode used by the core's add/edit dialog: instead of a
layer surface the host creates a normal `xdg_toplevel` (app_id `lively-mpv-host`, title from
`--title`, default "Lively Wallpaper") with that initial size, honours every configure/resize,
renders through the same EGL/libmpv path, follows the scale of the outputs the window overlaps,
and exits with code 0 when the window is closed (`xdg_toplevel.close`), on `cmd_close` or on
stdin EOF. `--output` and `--span` are ignored in windowed mode; the window always has a full
input region so it behaves like any application window. Only `xdg_wm_base` is required from
the compositor in this mode.

Exit codes: 0 closed/stdin EOF, 2 bad arguments, 3 missing `zwlr_layer_shell_v1` /
`zxdg_output_manager_v1` (or no Wayland connection), 4 output not found or removed / layer
surface closed, 5 EGL or libmpv initialisation failed, 6 media could not be loaded.

Implementation points worth knowing:

* The render context uses `MPV_RENDER_PARAM_ADVANCED_CONTROL`, which is what makes mpv take
  screenshots on the GPU; the software path cannot convert hardware-decoded (nvdec/vaapi)
  frames. That mode requires the render thread never to wait for the mpv core, so every
  property write/read and command goes through the asynchronous client API and the results
  come back as mpv events. Video geometry comes from the observed `video-out-params` property.
* Span mode: mpv ignores `video-scale-x/y` and `video-pan-x/y` while `keepaspect=no` (checked
  against mpv 0.40 via `osd-dimensions`). The host therefore sets `keepaspect=yes` and
  `video-unscaled=yes`, which makes mpv's base rectangle exactly `dwidth x dheight`, and then
  `video-scale-x/y` and `video-pan-x/y` place the slice of the virtual-screen fit on this
  output.
* `cmd_screenshot` Format 3 (bmp) is produced from mpv's `screenshot-raw` result because mpv has
  no BMP encoder; jpeg/png/webp use `screenshot-to-file`.
* `host_mpv_command` routes `["set_property", name, value]` and `["get_property", name]` to
  the property API exactly like mpv's JSON IPC does; every other array is run with
  `mpv_command_node_async`.
* `cmd_reload` runs `loadfile` again, which re-applies `--property` and sends a new
  `msg_wploaded`.

## Tests run

All of the following were run on KDE Plasma 6.7 (Wayland), outputs `DP-4` (2649x1490 logical,
scale 2 buffer) and `DP-3` (1080x1920 portrait, scale 1), mpv 0.40 / libmpv 2.5.0, NVIDIA
(nvdec hardware decoding was active).

```
make clean && make                                    # no warnings with -Wall -Wextra -O2
make test                                             # generates build/test.mp4, plays it on DP-3,
                                                      # cmd_volume, cmd_screenshot png, cmd_close
python3 -c "from PIL import Image, ImageStat; im=Image.open('build/shot.png').convert('RGB'); print(im.size, ImageStat.Stat(im).mean)"
```

`make test` output: host exit 0; `msg_hwnd`, `msg_wploaded Success:true` and
`msg_screenshot Success:true` on stdout; `shot.png` 1280x720 with mean RGB (123, 128, 128).
`LIVELY_TEST_OUTPUT=NAME make test` targets a different output.

Other checks performed by hand (scripts under the session scratchpad):

* Live captures with `spectacle -b -n -f -o ...` while hosts ran. On KWin the desktop is
  covered by application windows and "Show Desktop" raises Plasma's desktop above every
  layer-shell surface, so the only window on DP-3 (Discord) was minimized for 15 s through a
  KWin script (`w.minimized = true`, restored afterwards) and the DP-3 region of the capture
  was inspected: the test pattern filled the portrait output with `--layer background` and with
  `--layer bottom`, and with `--span 2649,0,1080,1920,3729,2056` it showed the right-hand slice
  of the virtual screen.
* Span continuity across DP-4 and DP-3: two hosts with `--span 0,136,2649,1490,3729,2056`
  (DP-4, 5296x2980 buffer) and `--span 2649,0,1080,1920,3729,2056` (DP-3, 1080x1920 buffer),
  each asked over stdin for `["screenshot-to-file", path, "window"]` (the exact buffer the
  compositor displays); the two images stitched at their logical positions form one continuous
  pattern with the colour-bar edges, the circle outline and the bottom gradient meeting at
  x=2649 without an offset.
* `echo -n | lively-mpv-host --output DP-3 build/test.mp4` and closing stdin after 2 s of
  playback: exit 0 within 0.6 s and 0.2 s.
* `--image` with a generated PNG: `image-display-duration=inf`, `loop-file=no`,
  `scale=nearest` applied (image smaller than the output), screenshot ok.
* `--property` with a copy of `Assets/Plugins/Mpv/LivelyProperties.json`: all sliders, the
  checkbox and the scaler dropdown applied after `file-loaded`; runtime `lp_slider` for
  `saturation` read back as 20, `speed` 1.5 (step 0.01 → double), `lp_chekbox mute`,
  `lp_dropdown_scaler`, `lp_button IsDefault`, `cmd_volume`, `cmd_suspend`/`cmd_resume`,
  `host_mpv_command` (`set_property`, `get_property`, `set`, `seek`), screenshots in bmp, jpeg
  and webp, malformed and unknown lines ignored.
* Wrong output name → exit 4; missing file → `msg_wploaded Success:false` and exit 6; bad
  arguments (missing `--output`, bad `--layer`, `--span`, `--volume`, `--scaler`, `--speed`,
  unknown option, two positionals) → exit 2.
* `--windowed 640x360 --title "Lively preview" --verbose build/test.mp4` (with `--output` and
  `--span` given and logged as ignored): the window appeared as a normal application window
  (confirmed with spectacle), `msg_hwnd`/`msg_wploaded`/`msg_screenshot` worked identically,
  and closing the window through a KWin script (`w.closeWindow()` on resourceClass
  `lively-mpv-host`) made the host exit with code 0.
