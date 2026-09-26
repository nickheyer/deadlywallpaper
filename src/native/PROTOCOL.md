# Lively Linux host protocol

This document is the contract between the Linux core (`Lively.Core.Linux`, C#) and the
processes that actually draw wallpapers on Linux. Every host and the core implement it
exactly; the Windows players (`Lively.Player.WebView2`, `mpv.exe`) are the reference
behaviour and this protocol mirrors them where Linux allows it.

There are three native helpers, all written in C, all built by the top-level `Makefile`:

| Binary              | Role |
|---------------------|------|
| `lively-wl-monitor` | Enumerates Wayland outputs (via `xdg-output`) and reports foreign toplevel window state (via `wlr-foreign-toplevel-management` on wlroots compositors, `org_kde_plasma_window_management` on KWin). Pure reporter, no rendering. |
| `lively-mpv-host`   | Renders video / gif / still images with libmpv into a `wlr-layer-shell` surface on the `background` layer of one output. |
| `lively-web-host`   | Renders HTML / URL wallpapers with WebKitGTK into a `wlr-layer-shell` surface (through `gtk-layer-shell`) on one output. |

On KDE Plasma the core does not use the two render hosts; it drives the Plasma wallpaper
plugin in `src/plasma/com.lively.wallpaper` instead (see "Plasma transport" below). The
monitor helper is used on every compositor.

## 1. Process lifecycle (all hosts)

* One host process per (wallpaper, output). The core spawns it with stdin and stdout
  redirected. stderr is inherited and used only for diagnostics.
* The host **must exit** when stdin reaches EOF. That is how orphaned wallpapers are
  prevented if the core crashes; do not rely on any other parent-death mechanism.
* The host must also exit (code 0) on `cmd_close`.
* Exit codes: `0` success/closed, `2` bad arguments, `3` compositor does not provide a
  required protocol (`zwlr_layer_shell_v1`, `zxdg_output_manager_v1`), `4` requested
  output not found, `5` renderer initialisation failed (EGL, libmpv, WebKit), `6` media /
  URL could not be loaded. The core maps these to `WallpaperPluginException`,
  `ScreenNotFoundException`, `WallpaperFileException`.
* All messages on stdin and stdout are UTF-8 JSON objects, one per line, terminated by
  `\n`. Unknown message types on stdin are ignored (and logged with `--verbose`).

## 2. Message envelope

Messages are the `IpcMessage` classes from `Lively.Models/Message`, serialised by
Newtonsoft with default settings. The discriminator is the integer property `Type`,
using the `MessageType` enum order:

| Value | Name                | Direction    | Extra fields |
|------:|---------------------|--------------|--------------|
| 0  | `msg_hwnd`             | host → core  | `Hwnd` (long). Linux hosts always send `0`; the core uses it only as the "process is up" signal. |
| 1  | `msg_console`          | host → core  | `Message` (string), `Category` (0 log, 1 error, 2 console) |
| 2  | `msg_wploaded`         | host → core  | `Success` (bool) |
| 3  | `msg_screenshot`       | host → core  | `FileName` (string, basename only), `Success` (bool) |
| 4  | `cmd_reload`           | core → host  | – |
| 5  | `cmd_close`            | core → host  | – |
| 6  | `cmd_screenshot`       | core → host  | `Format` (0 jpeg, 1 png, 2 webp, 3 bmp), `FilePath` (string), `Delay` (uint, ignored) |
| 7  | `cmd_suspend`          | core → host  | – |
| 8  | `cmd_resume`           | core → host  | – |
| 9  | `cmd_volume`           | core → host  | `Volume` (int 0–100) |
| 10 | `lsp_perfcntr`         | core → host  | `Info` (object, `HardwareUsageEventArgs`) |
| 11 | `lsp_nowplaying`       | core → host  | `Info` (object, `NowPlayingEventArgs`) |
| 12 | `lp_slider`            | core → host  | `Name` (string), `Value` (double), `Step` (double) |
| 13 | `lp_textbox`           | core → host  | `Name`, `Value` (string) |
| 14 | `lp_dropdown`          | core → host  | `Name`, `Value` (int) |
| 15 | `lp_fdropdown`         | core → host  | `Name`, `Value` (string relative path or null) |
| 16 | `lp_button`            | core → host  | `Name`, `IsDefault` (bool) |
| 17 | `lp_cpicker`           | core → host  | `Name`, `Value` (string "#rrggbb") |
| 18 | `lp_chekbox`           | core → host  | `Name`, `Value` (bool) |
| 19 | `lp_dropdown_scaler`   | core → host  | `Name`, `Value` (int: 0 none, 1 fill, 2 uniform, 3 uniformFill) |
| 20 | `lsp_audio`            | core → host  | `Data` (array of 128 doubles, 0..1). Linux-only addition; the core captures system audio and pushes spectrum frames at ~30 Hz to web wallpapers that asked for audio. |
| 100 | `host_mpv_command`    | core → mpv host | `Command` (JSON array, an mpv command as accepted by `mpv_command_node`, e.g. `["seek", 50, "absolute-percent"]`). Only `lively-mpv-host` understands this. |

Example lines:

```
{"Type":7}
{"Type":9,"Volume":50}
{"Type":12,"Name":"saturation","Value":20.0,"Step":1.0}
{"Type":100,"Command":["set_property","pause",true]}
{"Type":2,"Success":true}
{"Type":1,"Message":"Opening local project: /home/x/wall/index.html","Category":0}
```

## 3. Common command line (both render hosts)

```
--output NAME          Wayland output name as reported by lively-wl-monitor (required)
--namespace NAME       layer-shell namespace (default "lively-wallpaper")
--layer LAYER          background|bottom (default background)
--span X,Y,W,H,VW,VH   Span mode. This output covers logical rect (X,Y,W,H) of a virtual
                       screen VW×VH. The host renders the content as if it filled the whole
                       virtual screen and shows only its slice, so several hosts on
                       different outputs together display one continuous wallpaper.
--interactive          Give the surface a full input region so pointer events reach the
                       wallpaper. Without it the input region is empty and clicks pass
                       through to whatever is below. Keyboard interactivity is always none.
--windowed WxH         Preview mode: render into a normal resizable window of that initial
                       size (xdg_toplevel / GtkWindow) instead of a layer surface. `--output`,
                       `--span` and `--interactive` are ignored. The host exits with code 0
                       when the user closes the window.
--title TEXT           Window title in windowed mode (default "Lively Wallpaper").
--property PATH        Path to the wallpaper's LivelyProperties.json copy. Applied after
                       the content loads, and again on lp_button with IsDefault=true.
--volume N             Initial volume 0–100 (default 0).
--verbose              Log every stdin line and internal event to stderr.
```

The surface must be anchored to all four edges with exclusive zone `-1` so it always
covers the entire output including areas under panels, and must set the buffer scale
to the output's scale factor (render at physical resolution).

If the output disappears the host exits with code 4; the core restarts wallpapers on
display change.

## 4. `lively-mpv-host`

```
lively-mpv-host [common options] [--hwdec auto-safe|no] [--image] [--scaler none|fill|uniform|uniformFill]
                [--ytdl-format FMT] [--config-dir DIR] [--speed F] <file-or-url>
```

* Uses the libmpv render API (`MPV_RENDER_API_TYPE_OPENGL`) with EGL on the layer
  surface. Frames are rendered only when mpv signals `MPV_RENDER_UPDATE_FRAME`, paced by
  `wl_surface.frame` callbacks.
* Startup mpv options (matching the Windows core): `volume` from `--volume`,
  `loop-file=inf`, `keep-open=yes`, `input-default-bindings=no`, `osc=no`,
  `hwdec` from `--hwdec`, `vo=libmpv`, `audio-client-name=Lively Wallpaper`. Gif and
  still images additionally get `scale=nearest` when smaller than the output. `--image`
  sets `image-display-duration=inf` and `loop-file=no`. `--ytdl-format` enables ytdl and
  passes the format string (used for `videostream` wallpapers).
* `--scaler` and `lp_dropdown_scaler` map to mpv exactly like `VideoMpvPlayer.UpdateScaler`
  in the Windows core: none → `keepaspect=yes,video-unscaled=yes`; fill →
  `keepaspect=no,video-unscaled=no`; uniform → `keepaspect=yes,video-unscaled=no,panscan=0`;
  uniformFill → `keepaspect=yes,video-unscaled=no,panscan=1`. In span mode the host
  computes the fitted rectangle of the video within the virtual screen for the chosen
  scaler itself and sets `keepaspect=yes`, `video-unscaled=yes` (so the base rectangle is
  the video's own size) and then `video-scale-x/y`, `video-pan-x/y` so that this output
  shows the correct slice. (`keepaspect=no` makes mpv ignore scale and pan.)
* `lp_slider` and `lp_chekbox` set the mpv property named by `Name` to `Value`
  (integers when the slider step is whole, like the Windows core). `lp_button` with
  `IsDefault` re-applies `--property`. `host_mpv_command` runs the array as an mpv
  command. `cmd_suspend`/`cmd_resume` set `pause`. `cmd_volume` sets `volume`.
* `cmd_screenshot` runs `screenshot-to-file` with the requested path and format
  (`jpeg`/`png`/`webp` via `screenshot-format`), waits for the file to exist, then
  answers `msg_screenshot`.
* mpv log messages at `info` and above are forwarded as `msg_console` (`Category` 0,
  or 1 for `error`/`fatal`). `msg_wploaded` is sent when mpv reports `file-loaded`
  (or `MPV_EVENT_END_FILE` with an error → `Success:false`, then exit code 6).

## 5. `lively-web-host`

```
lively-web-host [common options] [--type local|online] [--debug PORT] [--color-scheme dark|light]
                [--pause-media] [--audio] [--sysinfo] [--nowplaying] [--pause-event]
                [--scale F] [--user-data DIR] <path-or-url>
```

Behaviour mirrors `Lively.Player.WebView2/Form1.cs` one for one:

* Local pages are loaded with `webkit_web_view_load_uri("file://…")`; the directory of
  the page must be readable (`allow-file-access-from-file-urls`,
  `allow-universal-access-from-file-urls` enabled). Online pages: Shadertoy links are
  converted to the embed page and YouTube links to the embed player exactly as
  `StreamUtil.TryParseShadertoy` / `TryParseYouTubeVideoIdFromUrl` do.
* WebKit settings: media autoplay without user gesture, WebGL on, hardware
  acceleration policy `ALWAYS`, context menu disabled unless `--debug`, developer
  extras only with `--debug`, `--user-data DIR` sets the website data directory
  (cache, local storage), `--scale F` sets the zoom level so CSS pixels match the
  output's logical pixels, `--color-scheme` sets the preferred colour scheme.
  Pop-ups (`create` signal) are opened in the default browser with `xdg-open` and
  cancelled in the view. Downloads are cancelled.
* Background: transparent until the first `load-changed FINISHED`, then white (WebView2
  does the same).
* After `load-changed FINISHED`: apply `--property` by calling
  `livelyPropertyListener(name, value)` for every control (same rules as
  `LivelyPropertyUtil.LoadProperty`: folder dropdowns resolve relative to the page
  directory and pass `null` when the file is missing), then send `msg_wploaded`.
  A load failure sends `msg_wploaded` with `Success:false` and exits with code 6.
* JavaScript bridge (all via `webkit_web_view_evaluate_javascript`, arguments JSON
  encoded): `livelyPropertyListener(name, value)`, `livelyWallpaperPlaybackChanged({"IsPaused":bool})`
  when `--pause-event`, `livelySystemInformation(info)` for `lsp_perfcntr`,
  `livelyCurrentTrack(info)` for `lsp_nowplaying`, `livelyAudioListener(array)` for
  `lsp_audio` (only when `--audio`). Every call is guarded with
  `if (typeof fn === 'function')`. Console messages from the page are forwarded as
  `msg_console` with `Category` 2.
* Pause (`cmd_suspend`): when `--pause-media` or the page is a YouTube stream, pause all
  `<video>`/`<audio>` elements; fire the playback-changed event when `--pause-event`;
  then freeze rendering **without losing the picture**: take a snapshot of the view
  (`webkit_web_view_get_snapshot`, visible region), show it in a `GtkImage` placed over
  the view inside a `GtkOverlay`, and hide the `WebKitWebView` widget so WebKit treats the
  page as hidden (timers and animation frames stop). Resume (`cmd_resume`) shows the view
  again, removes the snapshot, resumes media, fires the playback-changed event and
  re-sends the last now-playing payload when `--nowplaying`.
* `cmd_volume`: `webkit_web_view_set_is_muted(view, volume == 0)` (WebView2 also only has mute).
* `cmd_screenshot`: `webkit_web_view_get_snapshot` → `gdk_pixbuf_save` as `jpeg`/`png`/
  `webp`(falls back to png when the loader is missing)/`bmp`, reply `msg_screenshot`.
* `cmd_reload`: `webkit_web_view_reload`.

## 6. `lively-wl-monitor`

```
lively-wl-monitor [--once]
```

Connects to the Wayland display and prints JSON lines to stdout. Without `--once` it
keeps running and re-emits the affected list whenever something changes; the core
kills it on shutdown. Lines:

```
{"event":"capabilities","layer_shell":true,"plasma_shell":true,"toplevel_protocol":"plasma"}
{"event":"outputs","outputs":[
  {"name":"DP-4","description":"Dell U2723QE","make":"DEL","model":"U2723QE",
   "x":0,"y":136,"width":2649,"height":1490,"scale":1.45,"transform":0,
   "physical_width_mm":600,"physical_height_mm":340,"refresh_mhz":60000}]}
{"event":"toplevels","show_desktop":false,"toplevels":[
  {"id":"4","app_id":"org.kde.konsole","title":"~ : zsh","activated":true,"fullscreen":false,
   "maximized":false,"minimized":false,"skip_taskbar":false,"outputs":["DP-4"],
   "geometry":[100,200,800,600],"virtual_desktops":["c3f1…"],"activities":[],
   "on_current_desktop":true}]}
```

* `capabilities`: `layer_shell` is whether `zwlr_layer_shell_v1` is advertised,
  `plasma_shell` whether `org_kde_plasma_shell` is advertised, `toplevel_protocol` is
  `"wlr"`, `"plasma"` or `"none"`.
* `outputs`: logical geometry from `zxdg_output_v1` (`logical_position`,
  `logical_size`, `name`, `description`), physical data and `transform` from
  `wl_output`, `scale` as a real number computed as `physical_width_px / logical_width`.
  Emitted on start and after every `wl_output.done` / output removal.
* `toplevels`: the complete current list, emitted whenever any handle changes, the
  current virtual desktop changes or the show-desktop state changes.
  `show_desktop` is KWin's "show desktop" mode (`org_kde_plasma_window_management.
  show_desktop_changed`): while it is `true` the desktop is in front of every window
  and the core plays the wallpapers as if no window were open; always `false` on wlr.
  `outputs` lists the output names the window is on (`output_enter/leave` on wlr;
  on Plasma computed from the window geometry). `geometry` is `[x,y,w,h]` in logical
  coordinates or `null` when the protocol does not provide it. `id` is stable for the
  window's lifetime. `virtual_desktops` and `activities` are the ids of the virtual
  desktops and activities the window is on (`virtual_desktop_entered/left`,
  `activity_entered/left`); an empty list means the window is on all of them.
  `on_current_desktop` is `true` when the window is on every desktop or on one of the
  desktops currently activated in `org_kde_plasma_virtual_desktop_management`; the core
  ignores windows on other desktops, exactly like the Windows core ignores cloaked
  windows on other virtual desktops. Activities have no current-activity event in the
  Wayland protocol; the core reads it from `org.kde.ActivityManager` on D-Bus and
  ignores windows whose `activities` list does not contain it. The wlr protocol
  carries neither desktop nor activity membership, so wlr windows always report empty
  lists and `on_current_desktop: true`. With `"toplevel_protocol":"none"` the
  `toplevels` event is never emitted; the core then reports that fullscreen and focus
  based pausing is unavailable on this compositor.

## 7. Plasma transport (KDE)

On KDE the core installs the wallpaper plugin package `com.lively.wallpaper` under
`~/.local/share/plasma/wallpapers/` and uses plasmashell's scripting interface
(`org.kde.PlasmaShell` → `evaluateScript`) to switch a desktop containment to it and
write its configuration. The plugin connects back to the core over a local WebSocket
(`ws://127.0.0.1:<port>/wallpaper/<instance>`; port and instance id are written to the
plugin configuration) and then speaks the same JSON messages as section 2: the core
sends `cmd_*`, `lp_*`, `lsp_*` lines and the plugin answers with `msg_wploaded`,
`msg_screenshot`, `msg_console`. Plugin configuration keys:

| Key          | Type   | Meaning |
|--------------|--------|---------|
| `Source`     | string | Absolute file path or URL of the wallpaper. |
| `Kind`       | string | `video`, `gif`, `picture`, `web`, `url`, `videostream`, or `none`. |
| `CoreSocket` | string | WebSocket URL the plugin connects to. |
| `Instance`   | string | Instance id, echoed in the WebSocket path. |
| `Scaler`     | string | `none`, `fill`, `uniform`, `uniformFill`. |
| `Volume`     | int    | Initial volume 0–100. |
| `Interactive`| bool   | Whether the web view receives pointer input. |

The plugin must behave like the render hosts for every message it receives. It renders
video/gif/picture with `QtMultimedia` (`MediaPlayer` + `VideoOutput`, looping, with
`MultiEffect` providing brightness/contrast/saturation and `playbackRate` for speed) and
web content with `QtWebEngine` (`WebEngineView`) using the same JavaScript bridge as
`lively-web-host`. Screenshots are taken with `Item.grabToImage` and saved to the
requested path.
