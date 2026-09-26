# Lively.UI.Avalonia

Linux desktop UI of Lively Wallpaper, built with Avalonia 11. It talks to the wallpaper core
(`Lively.Core.Linux`) over the same gRPC named-pipe channel as the WinUI client and shares its view models
(`Lively.UI.Shared`).

## Build and run

```sh
dotnet build Lively.UI.Avalonia/Lively.UI.Avalonia.csproj
dotnet run --project Lively.UI.Avalonia -- --showApp true
```

The project targets `net9.0` and rolls forward to the installed major runtime (`RollForward=Major`), so a
machine with only the .NET 10 runtime runs it. Compiled bindings are on for every view (`x:DataType`).

The UI needs the core: it waits for the core's single-instance mutex (`Constants.SingleInstance.MutexName`) and
its gRPC pipe socket. Until both are reachable the main window shows a "core not running" state with a Retry
button; the rest of the interface appears once the connection succeeds.

Runtime dependencies used by the Linux services: `ffmpeg` (thumbnails of videos and gifs), `pactl`
(audio device list), `yt-dlp` or `youtube-dl` (stream wallpapers), `xdg-open` (open folders and links),
the XDG desktop portals (file pickers, screen colour picker), a Secret Service keyring (gallery tokens).

## Start flags

| Flag | Effect |
| --- | --- |
| `--showApp true` | Show the main window on start (also the default when no other flag is given). |
| `--trayWidget true` | Open only the customise window of the running wallpaper on the primary display; exits when it is closed unless the main window was shown meanwhile. |
| `--appUpdate true` | Show the main window on the update page. |
| `--core-managed` | Set by the core when it launches the UI: closing the main window hides it (the core re-shows it with `WM SHOW`), and the process exits when stdin reaches end of file. |

A second UI process forwards `WM SHOW` to the running one through a Unix socket in `$XDG_RUNTIME_DIR` and exits.

## stdin commands (from the core)

| Line | Action |
| --- | --- |
| `WM SHOW` | Show and activate the main window. |
| `WM HIDE` | Hide the main window. |
| `WM QUIT` | Exit. |
| `LM SHOWBUSY` / `LM HIDEBUSY` | Show / hide the busy overlay of the library. |
| `LM SHOWCUSTOMISEPANEL` | Open the control panel dialog. |
| `LM SHOWAPPUPDATEPAGE` | Navigate to the update page. |
| `LM WALLPAPERDATA {"infoPath","title","author","desc","contact","thumbnail"}` | Show the wallpaper metadata dialog pre-filled with the values and thumbnail, then answer on stdout. |

End of file on stdin exits the process in `--core-managed` mode; otherwise the listener stops and commands keep
arriving through the single-instance socket.

## stdout lines (to the core)

| Line | When |
| --- | --- |
| `LM UIVISIBLE true` / `LM UIVISIBLE false` | Once at start and whenever the main window is shown or hidden. |
| `LM WALLPAPERDATA {"ok":true,"title":"..","author":"..","desc":"..","contact":".."}` | The metadata dialog was confirmed. |
| `LM WALLPAPERDATA {"ok":false}` | The metadata dialog was cancelled or the request was invalid. |

Nothing else is written to stdout; logs go to stderr and `Constants.CommonPaths.LogDirUI`.

## Platform differences

| Area | Windows (WinUI) | Linux (Avalonia) |
| --- | --- | --- |
| Dialogs | WinUI `ContentDialog` | `Controls/ContentDialog` inside the window's `DialogHost`, one at a time |
| Changelog / supporters pages | WebView2 | Page text fetched with `WebPageTextFetcher`, no web view |
| Player plugins | wmf / mpv / vlc, CefSharp / WebView2 pickers | Single player per media kind (mpv, WebKitGTK host); pickers hidden through `IPlatformUiFeatures.SupportsPlayerSelection` |
| Stream wallpapers | `youtube-dl.exe` next to the core | `yt-dlp` or `youtube-dl` on `PATH` |
| Thumbnails | Windows shell thumbnails | Magick.NET for pictures, `ffmpeg` for videos and gifs, icon theme for other files |
| Gallery tokens | DPAPI | Secret Service keyring (`SecretServiceTokenProtector`) |
| Audio devices | WASAPI | `pactl` (PulseAudio / PipeWire) |
| Screen colour picker | Custom eyedropper window | XDG desktop portal `PickColor` |
| Desktop icons toggle, taskbar theme, system screensaver page and control panel tab, remote desktop pause rule, accent colour page | Available | Hidden through `IPlatformUiFeatures` capabilities (accent colour page opens the KDE / GNOME settings when found) |
| App theme | Fixed at start | Applied immediately (Auto follows the desktop) |
| Accent colour | Windows accent through `UISettings` | Desktop accent from the settings portal (`org.freedesktop.appearance` `accent-color`, live) written into the Fluent `SystemAccentColor*` resources by `LinuxAccentColorService` |
| Title bar | Extended into the window, search box in the caption | The compositor's own title bar; the search box sits between the navigation tabs and the toolbar |
| Library grid | Virtualising `GridView` | `Controls/VirtualizingUniformWrapPanel`, realising only the tile rows around the viewport |
| Theme colours as brushes | `{ThemeResource SystemAccentColor}` on brush properties | A `Color` resource never converts to a brush in Avalonia; `Resources/ThemeBrushes.axaml` provides the brushes (`AccentFillColorDefaultBrush`, `ChromeLowFillBrush`, ...) |
| Window placement | Saved through `SaveRectUI` | Owned by the compositor |
| Running applications (pause rules) | Top-level windows | Processes of the user attached to the display server, resolved through `.desktop` entries |
