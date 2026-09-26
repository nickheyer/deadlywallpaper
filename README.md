<p align="center">
  <img alt="Files Logo" src="resources/figma_promo_16x9.jpg" width="450" />
  <h2 align="center">Deadly Wallpaper</h2>
</p>

Cross platform port of W11 Lively Wallpaper that runs on Linux, Mac, and Windows

## Linux

Everything is driven from the top-level `Makefile`:

```
make deps            # Arch Linux packages (asks for sudo)
make all             # native helpers + core + UI + CLI + build/dist
make run             # run the core (it starts the UI) from build/dist
make test            # native self-tests, Plasma plugin harness, .NET unit tests
make install         # ~/.local/bin/lively-core, lively-ui, livelycu + app menu entry
make help            # every target
```

Requirements: a Wayland session (KDE Plasma 6, or a wlroots compositor such as Sway,
Hyprland, river, Wayfire), .NET SDK 9 or newer, libmpv, GTK 3 + WebKitGTK 4.1,
json-glib, ffmpeg, PipeWire/PulseAudio (`parec`), and on KDE the Qt 6 Multimedia,
WebEngine and WebSockets modules. `make deps` installs all of them on Arch.

### How it works

Lively is a headless core process plus a desktop UI that talks to it over gRPC. The
Linux port keeps that split and replaces only the platform layer:

| Piece | Windows | Linux |
|-------|---------|-------|
| Core daemon | `Lively` (WPF, WorkerW reparenting) | `Lively.Core.Linux` (console daemon) |
| Desktop attach on KDE | – | Plasma wallpaper plugin `com.lively.wallpaper`, driven over `org.kde.PlasmaShell` scripting and a local WebSocket |
| Desktop attach on wlroots | – | `wlr-layer-shell` background surfaces, one native host per output |
| Video / gif / picture / stream player | `mpv.exe` window | `lively-mpv-host` (libmpv + EGL on a layer surface) |
| Web wallpapers | WebView2 / CefSharp player | `lively-web-host` (WebKitGTK + gtk-layer-shell) |
| Displays and window tracking | Win32 monitors, foreground hooks | `lively-wl-monitor` (xdg-output, wlr-foreign-toplevel / plasma-window-management) |
| Desktop UI | WinUI 3 | `Lively.UI.Avalonia` (same view models via `Lively.UI.Shared`) |
| Tray, notifications, keyring, autostart | Win32 / DPAPI / registry | StatusNotifierItem + dbusmenu, `org.freedesktop.Notifications`, Secret Service, XDG autostart |
| Audio visualizer, hardware stats, now playing | WASAPI, PerformanceCounter, NPSM | PulseAudio monitor + FFT, `/proc` + `/sys`, MPRIS |

The shared code (`Lively.Common`, `Lively.Core.Common`, `Lively.Models`, `Lively.Grpc.*`,
`Lively.UI.Shared`) is portable and builds on Linux; Windows-only code lives in
`Lively.Common.Windows`, `Lively.Common.Services.Windows`, the WPF core and the WinUI project.
`make windows-check` cross-compiles the Windows core on Linux to prove the shared refactor
still builds for Windows.

Protocol between the core and the hosts/plugin: `src/native/PROTOCOL.md`.

### Platform differences on Linux

| Feature | Status on Linux |
|---------|-----------------|
| Video, gif, picture, video stream, web (HTML/URL/Shadertoy/YouTube), web with audio visualizer | Supported |
| Per-display, span and duplicate arrangements | Supported (span = one host per output showing its slice) |
| Pause under fullscreen / focused apps, per-app rules, battery and power-saver pause, pause while locked | Supported where the compositor exposes a window list (KWin, wlroots), with the Windows rules: a maximized, fullscreen or 95%-covering window pauses, the grid algorithm pauses once the uncovered share of the work area (screen minus always-visible panels) is down to the threshold. On KDE the show-desktop mode plays the wallpapers, and windows on other virtual desktops or activities are ignored. The core registers `lively-wl-monitor` with KWin automatically; the first start may take a few seconds while KWin reloads its service cache |
| Unity, Godot, BizHawk and other program wallpapers | Not possible: Wayland does not let one process reparent another's window. The core reports a clear error |
| Screensaver mode, taskbar theming, "set as static desktop wallpaper", remote-desktop pause, mouse forwarding through the shell | Windows-only; the UI hides them |
| Updates | The core checks this repository's GitHub releases and opens the release page; installs come from `make install` or your package manager |
| Desktop environments without layer-shell (GNOME) | Not supported |

Environment variables for developers: `LIVELY_BACKEND=plasma|layer-shell` forces a backend,
`LIVELY_NATIVE_DIR` points at the native helper binaries, `LIVELY_PLASMA_PKG` at the Plasma
plugin package, `LIVELY_UI_COMMAND` at the UI executable, `LIVELY_HOST_VERBOSE=1` makes the
hosts log every message. Logs: `~/.local/share/Lively Wallpaper/logs` (`make logs`).
