//! KDE Plasma presenter: plasmashell renders wallpapers through the Deadly Wallpaper plugin
//! package on each desktop containment, underneath its own icons and widgets. The daemon
//! drives the plugin through plasmashell's scripting interface and remembers which plugin
//! every containment showed before, so closing a wallpaper hands the desktop back to Plasma.

pub mod content;
pub mod package;
pub mod script;

use crate::error::{Error, Result, ctx};
use crate::geom::Rect;
use crate::model::Display;
use crate::msg::Msg;
use crate::paths::Paths;
use crate::platform::linux::MsgSender;
use crate::platform::{Capabilities, MsgSenderApi, ShellApi};
use crate::web::serve::Server;
use gtk::gio;
use gtk::prelude::*;
use script::{Client, PLUGIN};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn available() -> bool {
    script::available()
}

#[derive(Clone, Debug, Deserialize)]
struct Containment {
    id: i32,
    screen: i32,
    plugin: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

#[derive(Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    previous: BTreeMap<String, String>,
}

/// Which wallpaper plugin each containment showed before Deadly Wallpaper took it over,
/// persisted so a crash never strands a desktop on the live plugin.
pub struct Memory {
    path: PathBuf,
    file: Mutex<StateFile>,
    restoring: Mutex<HashSet<i32>>,
}

impl Memory {
    fn load(path: PathBuf) -> Memory {
        let file = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Memory { path, file: Mutex::new(file), restoring: Mutex::new(HashSet::new()) }
    }

    fn save(&self, file: &StateFile) {
        match serde_json::to_string_pretty(file) {
            Ok(text) => {
                if let Err(e) = ctx(std::fs::write(&self.path, text), self.path.display()) {
                    log::warn!("{e}");
                }
            }
            Err(e) => log::warn!("plasma state: {e}"),
        }
    }

    /// Remember the plugin a containment showed before ours; an earlier record wins.
    pub fn record(&self, containment: i32, previous: &str) {
        if previous == PLUGIN {
            return;
        }
        let Ok(mut file) = self.file.lock() else { return };
        let key = containment.to_string();
        if file.previous.contains_key(&key) {
            return;
        }
        file.previous.insert(key, previous.to_string());
        self.save(&file);
    }

    pub fn forget(&self, containment: i32) {
        if let Ok(mut file) = self.file.lock() {
            if file.previous.remove(&containment.to_string()).is_some() {
                self.save(&file);
            }
        }
        if let Ok(mut r) = self.restoring.lock() {
            r.remove(&containment);
        }
    }

    fn previous(&self) -> Vec<(i32, String)> {
        self.file.lock().map(|f| f.previous.iter().filter_map(|(k, v)| k.parse().ok().map(|id| (id, v.clone()))).collect()).unwrap_or_default()
    }

    fn begin_restore(&self, containment: i32) -> bool {
        self.restoring.lock().map(|mut r| r.insert(containment)).unwrap_or(false)
    }

    fn end_restore(&self, containment: i32) {
        if let Ok(mut r) = self.restoring.lock() {
            r.remove(&containment);
        }
    }
}

/// Containment id → (slot serial, display id) for every containment a slot holds.
type Occupancy = Arc<Mutex<HashMap<i32, (u64, String)>>>;

pub struct Slot {
    pub containments: Vec<i32>,
    serial: u64,
    occupancy: Occupancy,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Ok(mut o) = self.occupancy.lock() {
            o.retain(|_, (serial, _)| *serial != self.serial);
        }
    }
}

pub struct Shell {
    client: Client,
    serve: Server,
    memory: Arc<Memory>,
    /// Implementation directory of the installed package, written as the `Impl` key.
    impl_name: String,
    containments: Vec<Containment>,
    map: HashMap<String, Vec<i32>>,
    occupancy: Occupancy,
    next_serial: u64,
}

/// Detects containments the user switched away from the live plugin in Plasma's own
/// desktop settings, so the engine drops them instead of fighting the user.
#[derive(Clone)]
struct Dismissal {
    client: Client,
    occupancy: Occupancy,
    memory: Arc<Memory>,
    tx: MsgSender,
}

impl Dismissal {
    fn check(&self) {
        let snapshot: Vec<(i32, String)> = self.occupancy.lock().map(|o| o.iter().map(|(id, (_, display))| (*id, display.clone())).collect()).unwrap_or_default();
        if snapshot.is_empty() {
            return;
        }
        let (tx, memory) = (self.tx.clone(), self.memory.clone());
        self.client.eval_then(script::containments(), move |r| {
            let list: Vec<Containment> = match r.and_then(|out| serde_json::from_str(&out).map_err(|e| Error::Platform(e.to_string()))) {
                Ok(l) => l,
                Err(e) => {
                    log::warn!("plasma dismissal check: {e}");
                    return;
                }
            };
            let mut seen = HashSet::new();
            for c in list.iter().filter(|c| c.plugin != PLUGIN) {
                for (id, display) in snapshot.iter().filter(|(id, _)| *id == c.id) {
                    if seen.insert(display.clone()) {
                        memory.forget(*id);
                        tx.send(Msg::WallpaperDismissed { display: display.clone() });
                    }
                }
            }
        });
    }
}

impl Shell {
    pub fn new(paths: &Paths, tx: MsgSender) -> Result<Shell> {
        let client = Client::connect()?;
        let impl_name = package::install()?;
        let serve = Server::start()?;
        let memory = Arc::new(Memory::load(paths.config_dir.join("plasma.json")));
        let mut shell = Shell { client, serve, memory, impl_name, containments: Vec::new(), map: HashMap::new(), occupancy: Arc::default(), next_serial: 0 };
        shell.refresh()?;
        watch_bus(tx.clone());
        watch_config(Dismissal { client: shell.client.clone(), occupancy: shell.occupancy.clone(), memory: shell.memory.clone(), tx });
        Ok(shell)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn serve(&self) -> &Server {
        &self.serve
    }

    pub fn memory(&self) -> &Arc<Memory> {
        &self.memory
    }

    pub fn impl_name(&self) -> &str {
        &self.impl_name
    }

    fn refresh(&mut self) -> Result<()> {
        let out = self.client.eval_sync(script::containments(), Duration::from_secs(8))?;
        self.containments = serde_json::from_str(&out).map_err(|e| Error::Platform(format!("plasmashell containment list: {e}")))?;
        Ok(())
    }

    /// The containment on the current activity whose screen overlaps the display most.
    fn containments_for(&self, d: &Display) -> Vec<i32> {
        self.containments
            .iter()
            .filter(|c| c.screen >= 0)
            .filter_map(|c| {
                let r = Rect::new(c.x, c.y, c.w, c.h);
                let overlap = r.intersection(&d.rect)?.area();
                let smaller = r.area().min(d.rect.area()).max(1);
                (overlap * 2 >= smaller).then_some((overlap, c.id))
            })
            .max_by_key(|(overlap, _)| *overlap)
            .map(|(_, id)| vec![id])
            .unwrap_or_default()
    }
}

impl Drop for Shell {
    /// Restores queued by the final `settle` run on the worker thread; wait for them (and
    /// their bookkeeping) before the process ends.
    fn drop(&mut self) {
        let _ = self.client.eval_sync("print('OK|done')".into(), Duration::from_secs(5));
    }
}

impl ShellApi for Shell {
    type Slot = Slot;

    fn spans_displays(&self) -> bool {
        false
    }

    fn sync_displays(&mut self, displays: &[Display]) -> Result<bool> {
        self.refresh()?;
        let map: HashMap<String, Vec<i32>> = displays.iter().map(|d| (d.id.clone(), self.containments_for(d))).collect();
        let changed = self.map.iter().any(|(id, old)| map.get(id).is_some_and(|new| new != old));
        self.map = map;
        Ok(changed)
    }

    /// A Plasma desktop covers exactly its display, so the region is the display itself.
    fn slot(&mut self, display: &Display, _region: Rect) -> Result<Slot> {
        let containments = self.map.get(&display.id).cloned().unwrap_or_default();
        if containments.is_empty() {
            return Err(Error::Platform(format!("Plasma has no desktop on {} ({}x{} at {},{})", display.name, display.rect.w, display.rect.h, display.rect.x, display.rect.y)));
        }
        self.next_serial += 1;
        let serial = self.next_serial;
        if let Ok(mut o) = self.occupancy.lock() {
            for c in &containments {
                o.insert(*c, (serial, display.id.clone()));
            }
        }
        Ok(Slot { containments, serial, occupancy: self.occupancy.clone() })
    }

    fn settle(&mut self) {
        let occupied: HashSet<i32> = self.occupancy.lock().map(|o| o.keys().copied().collect()).unwrap_or_default();
        for (id, previous) in self.memory.previous() {
            if occupied.contains(&id) || !self.memory.begin_restore(id) {
                continue;
            }
            let memory = self.memory.clone();
            self.client.eval_then(script::restore(id, &previous), move |r| match r {
                Ok(_) => memory.forget(id),
                Err(e) => {
                    log::warn!("hand desktop containment {id} back to {previous}: {e}");
                    memory.end_restore(id);
                }
            });
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { presenter: "plasma".into(), pointer_motion: true, pointer_clicks: false, global_pointer: true, programs: false, web_devtools: false, rotate_web: true }
    }
}

/// plasmashell restarting or the current activity changing reassigns containments to screens.
fn watch_bus(tx: MsgSender) {
    let owner_tx = tx.clone();
    let _ = std::thread::Builder::new().name("plasma-owner".into()).spawn(move || {
        let Ok(conn) = zbus::blocking::Connection::session() else { return };
        let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else { return };
        let Ok(changes) = dbus.receive_name_owner_changed() else { return };
        for change in changes {
            if let Ok(args) = change.args() {
                if args.name() == "org.kde.plasmashell" && args.new_owner().is_some() {
                    std::thread::sleep(Duration::from_secs(3));
                    owner_tx.send(Msg::DesktopChanged);
                }
            }
        }
    });
    let _ = std::thread::Builder::new().name("plasma-activity".into()).spawn(move || {
        let Ok(conn) = zbus::blocking::Connection::session() else { return };
        let Ok(proxy) = zbus::blocking::Proxy::new(&conn, "org.kde.ActivityManager", "/ActivityManager/Activities", "org.kde.ActivityManager.Activities") else { return };
        let Ok(signals) = proxy.receive_signal("CurrentActivityChanged") else { return };
        for _ in signals {
            std::thread::sleep(Duration::from_millis(400));
            tx.send(Msg::DesktopChanged);
        }
    });
}

/// Plasma writes its desktop configuration whenever a wallpaper plugin changes, including
/// when the user picks another one in the desktop settings dialog.
fn watch_config(dismissal: Dismissal) {
    let Some(config) = dirs::config_dir() else { return };
    let file = gio::File::for_path(config.join("plasma-org.kde.plasma.desktop-appletsrc"));
    let monitor = match file.monitor_file(gio::FileMonitorFlags::NONE, None::<&gio::Cancellable>) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("watch Plasma desktop configuration: {e}");
            return;
        }
    };
    let pending: std::rc::Rc<std::cell::RefCell<Option<glib::SourceId>>> = std::rc::Rc::default();
    monitor.connect_changed(move |_, _, _, event| {
        if !matches!(event, gio::FileMonitorEvent::ChangesDoneHint | gio::FileMonitorEvent::Created | gio::FileMonitorEvent::Changed) {
            return;
        }
        if let Some(id) = pending.borrow_mut().take() {
            id.remove();
        }
        let dismissal = dismissal.clone();
        let p2 = pending.clone();
        let id = glib::timeout_add_local_once(Duration::from_millis(600), move || {
            p2.borrow_mut().take();
            dismissal.check();
        });
        *pending.borrow_mut() = Some(id);
    });
    // The monitor lives as long as the process.
    std::mem::forget(monitor);
}
