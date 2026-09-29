//! KWin (X11 and Wayland): a KWin script streams window state, and optionally the pointer
//! position, back over D-Bus.

use crate::content::PointerKind;
use crate::error::{Error, Result};
use crate::geom::Rect;
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::linux::MsgSender;
use crate::platform::{MsgSenderApi, Snapshot, WindowInfo, WindowPlacement};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zbus::blocking::connection::Builder;
use zbus::blocking::{Connection, Proxy};

pub const SERVICE: &str = "org.deadlywp.Daemon";
const PLUGIN: &str = "deadlywp";

const SCRIPT: &str = r#"
const SERVICE = "org.deadlywp.Daemon";
const list = () => (workspace.windowList ? workspace.windowList() : workspace.clientList());
const current = () => workspace.currentDesktop;
function onCurrent(w) {
  if (w.onAllDesktops) return true;
  const c = current();
  if (Array.isArray(w.desktops)) return w.desktops.length === 0 || w.desktops.indexOf(c) >= 0;
  return w.desktop === undefined || w.desktop === c || w.desktop === -1;
}
function snapshot() {
  const out = [];
  for (const w of list()) {
    if (w.desktopWindow || w.dock || w.minimized || w.skipTaskbar && !w.normalWindow && !w.fullScreen) continue;
    if (!(w.normalWindow || w.dialog || w.utility || w.fullScreen)) continue;
    if (!onCurrent(w)) continue;
    const g = w.frameGeometry;
    out.push({ x: Math.round(g.x), y: Math.round(g.y), w: Math.round(g.width), h: Math.round(g.height),
               fs: !!w.fullScreen, act: !!w.active, app: String(w.resourceClass || w.resourceName || ""), pid: w.pid || 0 });
  }
  callDBus(SERVICE, "/org/deadlywp/Monitor", "org.deadlywp.Monitor", "Windows", JSON.stringify(out));
}
const timer = new QTimer();
timer.singleShot = true;
timer.interval = 120;
timer.timeout.connect(snapshot);
function schedule() { timer.start(); }
function hook(w) {
  for (const s of ["frameGeometryChanged", "fullScreenChanged", "minimizedChanged", "desktopsChanged", "desktopChanged", "outputChanged", "activeChanged", "skipTaskbarChanged"]) {
    if (w[s] && w[s].connect) w[s].connect(schedule);
  }
}
list().forEach(hook);
const added = workspace.windowAdded || workspace.clientAdded;
const removed = workspace.windowRemoved || workspace.clientRemoved;
const activated = workspace.windowActivated || workspace.clientActivated;
if (added) added.connect(w => { hook(w); schedule(); });
if (removed) removed.connect(schedule);
if (activated) activated.connect(schedule);
if (workspace.currentDesktopChanged) workspace.currentDesktopChanged.connect(schedule);
snapshot();
if (__TRACK_POINTER__) {
  const ptimer = new QTimer();
  ptimer.singleShot = true;
  ptimer.interval = 25;
  ptimer.timeout.connect(() => { const p = workspace.cursorPos; callDBus(SERVICE, "/org/deadlywp/Monitor", "org.deadlywp.Monitor", "Pointer", Math.round(p.x) + "," + Math.round(p.y)); });
  workspace.cursorPosChanged.connect(() => { if (!ptimer.active) ptimer.start(); });
}
"#;

fn script_text(track_pointer: bool) -> String {
    SCRIPT.replace(
        "__TRACK_POINTER__",
        if track_pointer { "true" } else { "false" },
    )
}

pub fn available() -> bool {
    let Ok(conn) = Connection::session() else {
        return false;
    };
    let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from("org.kde.KWin") else {
        return false;
    };
    dbus.name_has_owner(name).unwrap_or(false)
}

struct Sink {
    tx: MsgSender,
}

#[derive(Deserialize)]
struct Win {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    fs: bool,
    act: bool,
    app: String,
    #[serde(default)]
    pid: i64,
}

#[zbus::interface(name = "org.deadlywp.Monitor")]
impl Sink {
    fn windows(&self, json: String) {
        match serde_json::from_str::<Vec<Win>>(&json) {
            Ok(wins) => {
                let windows = wins
                    .into_iter()
                    .map(|w| WindowInfo {
                        placement: WindowPlacement::Rect(Rect::new(w.x, w.y, w.w, w.h)),
                        fullscreen: w.fs,
                        maximized: false,
                        focused: w.act,
                        app: w.app,
                        pid: u32::try_from(w.pid).ok().filter(|p| *p > 0),
                    })
                    .collect();
                self.tx.send(Msg::Windows(Snapshot { windows }));
            }
            Err(e) => log::warn!("kwin snapshot: {e}"),
        }
    }

    fn pointer(&self, position: String) {
        let Some((x, y)) = position.split_once(',') else {
            return;
        };
        if let (Ok(x), Ok(y)) = (x.trim().parse::<i32>(), y.trim().parse::<i32>()) {
            self.tx.send(Msg::Pointer {
                x,
                y,
                kind: PointerKind::Move,
            });
        }
    }
}

pub struct Kwin {
    conn: Connection,
    script: PathBuf,
    track: Arc<AtomicBool>,
}

impl Kwin {
    pub fn start(tx: MsgSender, paths: &Paths, track_pointer: bool) -> Result<Kwin> {
        let conn = Builder::session()
            .and_then(|b| b.name(SERVICE))
            .and_then(|b| b.serve_at("/org/deadlywp/Monitor", Sink { tx }))
            .and_then(|b| b.build())
            .map_err(|e| Error::Platform(format!("session bus: {e}")))?;
        let script = paths.cache_dir.join("kwin-monitor.js");
        let track = Arc::new(AtomicBool::new(track_pointer));
        std::fs::write(&script, script_text(track_pointer))?;
        load(&conn, &script)?;
        let (watcher, watched_script, watched_track) =
            (conn.clone(), script.clone(), track.clone());
        let _ = std::thread::Builder::new()
            .name("kwin-watch".into())
            .spawn(move || {
                let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&watcher) else {
                    return;
                };
                let Ok(changes) = dbus.receive_name_owner_changed() else {
                    return;
                };
                for change in changes {
                    if let Ok(args) = change.args() {
                        if args.name() == "org.kde.KWin" && args.new_owner().is_some() {
                            std::thread::sleep(std::time::Duration::from_secs(2));
                            let text = script_text(watched_track.load(Ordering::Relaxed));
                            if let Err(e) = std::fs::write(&watched_script, text)
                                .map_err(Error::Io)
                                .and_then(|_| load(&watcher, &watched_script))
                            {
                                log::warn!("reload KWin monitor script: {e}");
                            }
                        }
                    }
                }
            });
        Ok(Kwin {
            conn,
            script,
            track,
        })
    }

    /// Rewrite and reload the script with pointer streaming on or off.
    pub fn set_pointer_tracking(&mut self, track_pointer: bool) {
        if self.track.swap(track_pointer, Ordering::Relaxed) == track_pointer {
            return;
        }
        if let Err(e) = std::fs::write(&self.script, script_text(track_pointer))
            .map_err(Error::Io)
            .and_then(|_| load(&self.conn, &self.script))
        {
            log::warn!("reload KWin monitor script: {e}");
        }
    }
}

impl Drop for Kwin {
    fn drop(&mut self) {
        if let Ok(scripting) = Proxy::new(
            &self.conn,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        ) {
            let _: std::result::Result<bool, _> = scripting.call("unloadScript", &(PLUGIN,));
        }
    }
}

fn load(conn: &Connection, script: &std::path::Path) -> Result<()> {
    let scripting = Proxy::new(conn, "org.kde.KWin", "/Scripting", "org.kde.kwin.Scripting")
        .map_err(|e| Error::Platform(e.to_string()))?;
    let _: std::result::Result<bool, _> = scripting.call("unloadScript", &(PLUGIN,));
    let id: i32 = scripting
        .call("loadScript", &(script.to_string_lossy().as_ref(), PLUGIN))
        .map_err(|e| Error::Platform(format!("loadScript: {e}")))?;
    if id < 0 {
        return Err(Error::Platform("KWin rejected the monitor script".into()));
    }
    scripting
        .call_method("start", &())
        .map_err(|e| Error::Platform(format!("start scripts: {e}")))?;
    log::info!("KWin window monitor script loaded");
    Ok(())
}
