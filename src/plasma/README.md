# Lively Wallpaper – KDE Plasma 6 wallpaper plugin

`com.lively.wallpaper` is the Plasma 6 wallpaper plugin that the Lively Linux core drives on
KDE instead of the layer-shell render hosts (see `src/native/PROTOCOL.md`, section 7). It
renders video, gif, pictures, video streams, local web pages and online URLs inside
plasmashell, connects back to the core over a local WebSocket and speaks the section 2
message set.

```
src/plasma/
├── build.sh                          compiles contents/shaders/adjust.frag -> adjust.frag.qsb (qsb)
├── com.lively.wallpaper/             the KPackage (Plasma/Wallpaper)
│   ├── metadata.json
│   └── contents/
│       ├── config/main.xml           configuration keys (kcfg)
│       ├── shaders/adjust.frag       hue/gamma fragment shader (GLSL 440) + compiled .qsb
│       └── ui/
│           ├── main.qml              WallpaperItem root, binds wallpaper.configuration.* into LivelyContent
│           ├── config.qml            settings page (read-only Source/Kind, fill color picker)
│           ├── LivelyContent.qml     framework-agnostic surface: WebSocket, dispatch, screenshots, renderer Loader
│           ├── VideoRenderer.qml     video / gif / videostream (QtMultimedia)
│           ├── ImageRenderer.qml     picture (Image)
│           ├── WebRenderer.qml       web / url (QtWebEngine) with the Lively JavaScript bridge
│           ├── AdjustLayer.qml       MultiEffect + ShaderEffect colour adjustments shared by video and picture
│           └── LivelyUtil.qml        URL helpers (StreamUtil port), scaler names, JS literal encoding
└── test/
    ├── run_tests.sh                  qmllint + generated media + end-to-end runs for video, gif, picture, web, span
    ├── fake_core.py                  scripted Lively core: RFC 6455 WebSocket server (standard library only) + assertions
    ├── span_compare.py               compares the span-mode screenshots against the full-frame screenshot
    ├── harness.qml                   hosts LivelyContent in a window without Plasma (size and --span configurable)
    └── web/index.html                test page defining every lively* bridge function
```

## Requirements

Plasma 6 (developed against 6.7.5), Qt 6 with `qt6-declarative`, `qt6-multimedia` (+ the ffmpeg
backend), `qt6-webengine`, `qt6-websockets`; `qt6-shadertools` (`qsb`) to rebuild the shader.
The tests additionally need `ffmpeg`, `python-pillow` and `/usr/lib/qt6/bin/qml`.

## Build and install

```sh
cd src/plasma
./build.sh                                              # regenerates contents/shaders/adjust.frag.qsb
kpackagetool6 --type Plasma/Wallpaper --install com.lively.wallpaper     # first install
kpackagetool6 --type Plasma/Wallpaper --upgrade com.lively.wallpaper     # later updates
kpackagetool6 --type Plasma/Wallpaper --show com.lively.wallpaper
kpackagetool6 --type Plasma/Wallpaper --remove com.lively.wallpaper
```

The package is installed to `~/.local/share/plasma/wallpapers/com.lively.wallpaper` (copying the
directory there by hand is equivalent). The compiled `adjust.frag.qsb` is part of the package
and must be present; `build.sh` fails loudly when `qsb` is missing.

Switching a desktop containment to the plugin and writing its configuration is done by the core
through plasmashell's scripting interface (`org.kde.PlasmaShell` `evaluateScript`), not by
this package.

## Configuration keys (`contents/config/main.xml`, group `General`)

| Key           | Type   | Default       | Meaning |
|---------------|--------|---------------|---------|
| `Source`      | String | `""`          | Absolute file path or URL of the wallpaper. |
| `Kind`        | String | `none`        | `video`, `gif`, `picture`, `web`, `url`, `videostream` or `none`. |
| `CoreSocket`  | String | `""`          | WebSocket URL of the core, e.g. `ws://127.0.0.1:41234`. |
| `Instance`    | String | `""`          | Instance id; the plugin connects to `CoreSocket + "/wallpaper/" + Instance`. |
| `Scaler`      | String | `uniformFill` | `none`, `fill`, `uniform`, `uniformFill`. |
| `Volume`      | Int    | `0`           | Initial volume 0–100. |
| `Interactive` | Bool   | `false`       | Whether the web view receives pointer input. Keyboard input is never given. |
| `FillColor`   | Color  | `#000000`     | Painted behind the wallpaper and on its own when `Kind` is `none`. |
| `SpanX`, `SpanY`, `SpanWidth`, `SpanHeight` | Int | `0` | Span mode: the slice of the virtual screen this desktop shows. |
| `SpanVirtualWidth`, `SpanVirtualHeight` | Int | `0` | Span mode: size of the virtual screen; both > 0 enable span mode. |

### Span mode

The six `Span*` keys are the plugin form of the render hosts' `--span X,Y,W,H,VW,VH`. When
`SpanVirtualWidth` and `SpanVirtualHeight` are both > 0 the wallpaper is one image rendered
over a virtual screen of that size and this desktop shows the slice
(`SpanX`, `SpanY`, `SpanWidth`, `SpanHeight`) of it: the renderer (video, image or web view)
is laid out at the full virtual size, positioned at (−`SpanX`, −`SpanY`) inside the clipped
wallpaper item, and scaled by `item.width / SpanWidth` × `item.height / SpanHeight` (1 when
the slice is the desktop's own size), so several desktops with complementary slices show one
continuous wallpaper. The scaler (`uniformFill` etc.) is applied against the virtual size, and
`none` centres the media's native resolution in the virtual screen. Screenshots capture the
slice. Span mode with `SpanWidth` or `SpanHeight` ≤ 0 is a configuration error: `msg_console`
error, `msg_wploaded` `Success:false`, `FillColor` shown. With both virtual sizes 0 the other
four keys are ignored.

`Kind none` shows `FillColor` and connects to the core like every other kind. An unknown
`Kind` shows `FillColor`, sends `msg_console` (error) and `msg_wploaded` `Success:false`.

The wallpaper keeps rendering from its configuration alone, so the last wallpaper is back
after login before the core has started; the core then connects by writing a fresh
`CoreSocket`/`Instance`.

## Transport

* The plugin connects as soon as `CoreSocket` and `Instance` are both set and reconnects with
  backoff (1 s, 2 s, 4 s, 8 s, 10 s, 10 s, …) whenever the connection is lost, until
  `cmd_close` arrives or the keys are cleared.
* Every WebSocket text frame is one JSON object with the integer `Type` of PROTOCOL.md
  section 2, exactly as the render hosts read and write stdin/stdout lines.
* `msg_wploaded` is sent when the wallpaper has loaded (or failed), and again on every new
  connection while it is loaded, so a core that connects after the wallpaper is already up
  learns its state immediately. `msg_console` frames produced while disconnected are queued
  (256 most recent) and flushed on connect.
* `msg_hwnd` is never sent: there is no window handle in a wallpaper item, and the core treats
  the WebSocket connection itself as the "wallpaper is up" signal.
* `cmd_close` ends the session: the renderer is unloaded (the surface shows `FillColor`), the
  socket is closed and no reconnect happens. A new session starts when `Source`, `Kind` or the
  socket URL changes, so the core must write a fresh `Instance` (or a different socket) for
  every wallpaper it starts.
* `LivelyProperties.json` is not read by the plugin. After `msg_wploaded` the core sends every
  control as `lp_*` messages, and does so again after `lp_button` with `IsDefault:true` (the
  plugin resets to defaults on that message; the core's re-send restores the file values).

### Messages

| Type | Name                 | video / gif / videostream | picture | web / url |
|-----:|----------------------|---------------------------|---------|-----------|
| 4  | `cmd_reload`         | re-opens the media, `msg_wploaded` again | re-reads the file, `msg_wploaded` again | `reload()`, `msg_wploaded` again |
| 5  | `cmd_close`          | session ends (see above) | same | same |
| 6  | `cmd_screenshot`     | `grabToImage` → `saveToFile`, `msg_screenshot` | same | same (also while suspended: the freeze frame is captured) |
| 7  | `cmd_suspend`        | `MediaPlayer.pause()` | accepted, nothing to pause | pause `<video>/<audio>`, `livelyWallpaperPlaybackChanged("{\"IsPaused\":true}")`, snapshot, hide, `LifecycleState.Frozen` |
| 8  | `cmd_resume`         | `MediaPlayer.play()` | accepted | show, `Active`, play media, `livelyWallpaperPlaybackChanged("{\"IsPaused\":false}")`, re-send last now-playing payload |
| 9  | `cmd_volume`         | `AudioOutput.volume = Volume/100` | accepted | `audioMuted = (Volume == 0)` (WebView2 also only mutes) |
| 10 | `lsp_perfcntr`       | ignored (as lively-mpv-host) | ignored | `livelySystemInformation(JSON string of Info)` |
| 11 | `lsp_nowplaying`     | ignored | ignored | `livelyCurrentTrack(JSON string of Info)`, remembered for resume |
| 12 | `lp_slider`          | `saturation`, `hue`, `brightness`, `contrast`, `gamma` (−100..100), `speed` (playbackRate); any other name → `msg_console` error | same (speed accepted, no effect) | `livelyPropertyListener(Name, Value)` |
| 13 | `lp_textbox`         | ignored | ignored | `livelyPropertyListener(Name, Value)` |
| 14 | `lp_dropdown`        | ignored | ignored | `livelyPropertyListener(Name, Value)` |
| 15 | `lp_fdropdown`       | ignored | ignored | `livelyPropertyListener(Name, Value)` when `dirname(Source)/Value` exists, else `null` (online pages always `null`) |
| 16 | `lp_button`          | `IsDefault`: speed 1, mute off, adjustments 0, scaler from config | `IsDefault`: adjustments 0, scaler from config | `IsDefault`: nothing (core re-sends); otherwise `livelyPropertyListener(Name, true)` |
| 17 | `lp_cpicker`         | ignored | ignored | `livelyPropertyListener(Name, Value)` |
| 18 | `lp_chekbox`         | `mute` → `AudioOutput.muted`; other names → error | `mute` accepted, no audio; other names → error | `livelyPropertyListener(Name, Value)` |
| 19 | `lp_dropdown_scaler` | 0 none, 1 fill, 2 uniform, 3 uniformFill | same | ignored (WebView2 has no scaler) |
| 20 | `lsp_audio`          | ignored | ignored | `livelyAudioListener(Data)` (JS array) |
| 100 | `host_mpv_command`  | see below | `msg_console` log line (no time axis, no audio) | `msg_console` log line |

`host_mpv_command` (`{"Type":100,"Command":[...]}`) on video/gif/videostream runs this mpv
command subset:

| Command | Effect |
|---------|--------|
| `["seek", p, "absolute-percent"]` | `position = p/100 × duration` |
| `["seek", d, "relative-percent"]` | `position += d/100 × duration` |
| `["seek", s, "absolute"]`, `["seek", s, "relative"]`, `["seek", s]` | seconds, like mpv (relative is mpv's default) |
| `["set_property", "pause", bool]` | pauses/resumes independently of `cmd_suspend` (`yes`/`no` accepted too) |
| `["set_property", "volume", 0–100]` | same value as `cmd_volume` |
| `["set_property", "mute", bool]` | same as `lp_chekbox` `mute` |
| `["set_property", "speed", n]` | same as `lp_slider` `speed` |
| `["set_property", "aid", "no" / "1" / "auto" / n]` | audio track off (`"no"`) or on; combined with `mute` for `AudioOutput.muted` |

Percent seeks on media without a known duration, seeks on non-seekable media and malformed
values answer with a `msg_console` error; any other command name, seek flag or property name
produces a `msg_console` log line (category 0) and is otherwise ignored, as PROTOCOL.md
section 1 requires for unknown input.

"ignored" is the behaviour of the reference host for that message; unknown `Type` values are
ignored as PROTOCOL.md section 1 requires. `msg_console` categories: 0 log, 1 error, 2 page
console output.

JavaScript bridge arguments follow `Lively.Player.WebView2/Form1.cs` byte for byte: every call
is `if (typeof fn === 'function') { fn(<JSON literals>); }`; `livelyPropertyListener` gets the
raw value; `livelyWallpaperPlaybackChanged`, `livelySystemInformation` and `livelyCurrentTrack`
get a JSON **string** (pages `JSON.parse` it, as in the Lively docs); `livelyAudioListener`
gets the array.

### Renderers

* **video / gif / videostream** – `MediaPlayer` + `VideoOutput` + `AudioOutput`, `loops:
  MediaPlayer.Infinite`, autoplay. `videostream` plays `Source` as a URL directly. Scaler
  mapping: `none` → native resolution centered (one video pixel per device pixel, cropped),
  `fill` → `Stretch`, `uniform` → `PreserveAspectFit`, `uniformFill` → `PreserveAspectCrop`.
  `msg_wploaded` on `LoadedMedia` (or `BufferedMedia`, whichever comes first);
  `InvalidMedia`/`errorOccurred` → `msg_console` error and `Success:false`.
* **picture** – `Image` with `asynchronous: true`, same scaler mapping, `msg_wploaded` on
  `Image.Ready`, error on `Image.Error`.
* **Adjustments** (`AdjustLayer.qml`, video and picture) – mpv ranges −100..100:
  brightness/contrast/saturation → `MultiEffect` −1..1 (÷100); hue → `ShaderEffect` rotating
  the chroma by `hue/100·π` radians (mpv's hue mapping); gamma → `pow(c, 1/8^(gamma/100))`
  (mpv's gamma mapping). The chain only runs while a value is non-zero; with everything at 0
  the source item draws itself.
* **web / url** – `WebEngineView`. `web`: `file://` URL of `Source` (each path segment
  percent-encoded). `url`: Shadertoy `…/view/<id>` → `…/embed/<id>?gui=false&t=10&paused=false&muted=true`
  (query string of `StreamUtil.TryParseShadertoy`, loaded directly instead of through
  WebView2's wrapper page); YouTube links → `https://www.youtube.com/embed/<id>?version=3&rel=0&autoplay=1&loop=1&controls=0&playlist=<id>`
  with the id parsed exactly like `StreamUtil.TryParseYouTubeVideoIdFromUrl`; anything else is
  loaded as given. Settings: `playbackRequiresUserGesture: false`, `webGLEnabled: true`,
  `localContentCanAccessFileUrls: true`, `localContentCanAccessRemoteUrls: true`,
  `javascriptCanOpenWindows: false`; background transparent until the first load finishes,
  then white; user-initiated new-window requests open in the default browser
  (`Qt.openUrlExternally`); page console output → `msg_console` category 2; render process
  termination → `msg_console` error. `Interactive: false` sets `enabled: false` on the view, so
  it draws and animates normally but never receives pointer events (a MouseArea shield would
  also swallow the desktop's own right-click menu). `activeFocusOnPress` is always off.
  `msg_wploaded` is sent when the navigation succeeded **and** the page has painted its first
  frame into the scene graph (two `requestAnimationFrame` callbacks in the page followed by a
  Qt frame swap); a page that never paints is reported 3 s after the navigation finished. A
  failed navigation is reported immediately with `Success:false`.
* **Suspend for web** – Chromium refuses `LifecycleState.Frozen` while the view is visible
  (`setLifecycleState: failed to transition from Active to Frozen state: page is visible`),
  so the plugin does what `lively-web-host` does: it grabs the view into an `Image` overlay,
  hides the view and then freezes it. Timers and animation frames stop while the last frame
  stays on screen; `cmd_resume` shows the view, sets `Active`, plays media, fires the playback
  event and drops the overlay after the view has repainted.

### Screenshots

`cmd_screenshot` grabs the whole surface at device-pixel resolution and writes it with
`ItemGrabResult.saveToFile(FilePath)`. Qt chooses the codec from the file name suffix, so the
suffix must match `Format` (`0` → `.jpg`/`.jpeg`, `1` → `.png`, `2` → `.webp`, `3` → `.bmp`), which
is how the core builds the path (`filePath + ".jpg"` with `Format` jpeg). A mismatch or a
write failure answers `msg_screenshot` `Success:false` plus a `msg_console` error rather than
a file in a different format. `FileName` is the basename of `FilePath`.

## Tests

```sh
cd src/plasma
./test/run_tests.sh                    # everything: qmllint, media generation, video, gif, picture, web, span
KINDS="web span" ./test/run_tests.sh   # a subset
LIVELY_TEST_TMP=/tmp/lively-plasma ./test/run_tests.sh   # keep screenshots/logs in a fixed directory
```

`run_tests.sh`

1. runs `/usr/lib/qt6/bin/qmllint` on every QML file (package and harness);
2. generates `test.mp4` (`ffmpeg -f lavfi -i testsrc=size=1280x720:rate=30 -t 5 -pix_fmt yuv420p`),
   `test.png` (one `testsrc` frame) and `test.gif`;
3. for each kind starts `fake_core.py` on a free port and `harness.qml` under
   `/usr/lib/qt6/bin/qml` (a normal 960x540 window on the running session), then waits for both;
4. for `span` starts three fake core / harness pairs at once on the test video: a 960x540
   window without span, a 480x540 window with `--span 0,0,480,540,960,540` and one with
   `--span 480,0,480,540,960,540`. Each core pauses the player (`set_property pause`), seeks to
   0 % and takes a screenshot; `span_compare.py` then requires each half window to match its
   half of the full frame (mean absolute difference ≤ 12) and to differ from the other half
   (≥ 30).

`fake_core.py` prints every frame in both directions with timestamps and asserts, per kind:
the WebSocket path, `msg_wploaded` `Success:true`, `msg_screenshot` `Success:true` with the
right `FileName`, that every PNG has the window's pixel size, a non-blank mean and structure
(PIL); for video/gif/picture additionally that saturation −100 yields a grey image,
`IsDefault` restores colour, gamma +100 brightens, hue +100 changes the picture, brightness
−100 blacks it out, scaler `none` changes the framing, `cmd_suspend` freezes playback
(identical frames one second apart) and `cmd_resume` advances it, `cmd_reload` re-reports
exactly one `msg_wploaded`; for video/gif additionally the `host_mpv_command` subset: seek to
50 % shows a different frame than 0 %, a relative −50 % seek returns to the identical first
frame, `set_property pause true` holds the frame and `false` resumes it, volume/mute/speed/aid
are accepted without errors and an unknown command is logged at category 0 (a still image
logs that the command has no effect); for web that every `lively*` bridge function received the expected JSON
arguments (including `lp_fdropdown` present/missing/null), that the page's 250 ms timer stops
while suspended and runs after resume with the playback events in order, that the now-playing
payload is re-sent on resume, that screenshots while suspended and after resume show the page
and that `cmd_reload` reloads it; for all kinds that `Format` 0 writes a JPEG, that a
`Format`/suffix mismatch is rejected, that the plugin disconnects on `cmd_close` and does not
reconnect, and that no unexpected error console messages were produced.

Manual harness run:

```sh
QT_FORCE_STDERR_LOGGING=1 /usr/lib/qt6/bin/qml test/harness.qml -- \
    --kind web --source "$PWD/test/web/index.html" --core ws://127.0.0.1:41234 --instance demo \
    --scaler uniformFill --volume 0 --interactive false --timeout 120000 \
    [--width 960 --height 540] [--span X,Y,W,H,VW,VH]
```

`QT_FORCE_STDERR_LOGGING=1` is needed on Arch because Qt sends its log to journald when stderr
is not a terminal.
