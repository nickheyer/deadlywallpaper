//! External program wallpapers embedded through XEmbed (X11 sessions only).

use crate::content::{Content, ContentEvent, PointerEvent, Seek};
use crate::error::{Error, Result};
use crate::model::Control;
use crate::msg::Msg;
use crate::platform::MsgSenderApi;
use crate::platform::linux::MsgSender;
use crate::platform::linux::canvas::Slot;
use glib::SendWeakRef;
use gtk::prelude::*;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};

pub fn spawn(spec: &crate::platform::ContentSpec<'_>, slot: &Slot, tx: MsgSender, x11: bool) -> Result<Box<dyn Content>> {
    if !x11 {
        return Err(Error::Unsupported(
            "program wallpapers need an X11 session: this Wayland compositor does not allow embedding another application's window".into(),
        ));
    }
    let wp = spec.wallpaper;
    let exe = PathBuf::from(&wp.source);
    if !exe.is_file() {
        return Err(Error::NotFound(format!("{} does not exist", exe.display())));
    }
    let child = Command::new(&exe)
        .args(wp.info.args())
        .current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Platform(format!("start {}: {e}", exe.display())))?;
    let pid = child.id();
    let socket = gtk::Socket::new();
    socket.set_size_request(slot.size.w, slot.size.h);
    slot.container.pack_start(&socket, true, true, 0);
    socket.show();
    socket.realize();
    let weak: SendWeakRef<gtk::Socket> = socket.downgrade().into();
    let timeout = Duration::from_secs(spec.settings.video.load_timeout_secs);
    let id = spec.id;
    let finder_tx = tx.clone();
    std::thread::Builder::new()
        .name("program-window".into())
        .spawn(move || match find_window(pid, timeout) {
            Ok(xid) => {
                glib::idle_add_once(move || {
                    if let Some(s) = weak.upgrade() {
                        s.add_id(xid as _);
                    }
                });
                finder_tx.send(Msg::Content(id, ContentEvent::Loaded));
            }
            Err(e) => finder_tx.send(Msg::Content(id, ContentEvent::Exited { reason: e.to_string() })),
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(Box::new(ProgramContent::new(child, socket, id, tx)))
}

/// Wait until the process (or a descendant) maps a client window and return its id.
fn find_window(pid: u32, timeout: Duration) -> Result<u32> {
    let (conn, screen) = x11rb::connect(None).map_err(|e| Error::Platform(format!("X11: {e}")))?;
    let root = conn.setup().roots[screen].root;
    let atom = |name: &[u8]| -> Result<u32> {
        conn.intern_atom(false, name)
            .map_err(|e| Error::Platform(e.to_string()))?
            .reply()
            .map(|r| r.atom)
            .map_err(|e| Error::Platform(e.to_string()))
    };
    let client_list = atom(b"_NET_CLIENT_LIST")?;
    let net_pid = atom(b"_NET_WM_PID")?;
    let deadline = Instant::now() + timeout;
    loop {
        let family = descendants(pid);
        let clients = conn
            .get_property(false, root, client_list, AtomEnum::WINDOW, 0, u32::MAX)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| r.value32().map(|v| v.collect::<Vec<u32>>()).unwrap_or_default())
            .unwrap_or_default();
        for w in clients {
            let owner = conn
                .get_property(false, w, net_pid, AtomEnum::CARDINAL, 0, 1)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32().and_then(|mut v| v.next()));
            if owner.is_some_and(|p| family.contains(&p)) {
                return Ok(w);
            }
        }
        if Instant::now() > deadline {
            return Err(Error::Platform(format!("process {pid} showed no window within {}s", timeout.as_secs())));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// `pid` and every process descending from it, from /proc.
fn descendants(pid: u32) -> Vec<u32> {
    let mut parents: Vec<(u32, u32)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Some(p) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            if let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) {
                if let Some(rest) = stat.rsplit(')').next() {
                    if let Some(ppid) = rest.split_whitespace().nth(1).and_then(|s| s.parse().ok()) {
                        parents.push((p, ppid));
                    }
                }
            }
        }
    }
    let mut family = vec![pid];
    let mut i = 0;
    while i < family.len() {
        let cur = family[i];
        for (p, pp) in &parents {
            if *pp == cur && !family.contains(p) {
                family.push(*p);
            }
        }
        i += 1;
    }
    family
}

pub struct ProgramContent {
    pid: u32,
    socket: gtk::Socket,
    paused: bool,
}

impl ProgramContent {
    fn new(mut child: Child, socket: gtk::Socket, id: crate::content::ContentId, tx: MsgSender) -> ProgramContent {
        let pid = child.id();
        let _ = std::thread::Builder::new().name("program-wait".into()).spawn(move || {
            let status = child.wait();
            let reason = match status {
                Ok(s) => format!("program exited with {s}"),
                Err(e) => format!("program wait failed: {e}"),
            };
            tx.send(Msg::Content(id, ContentEvent::Exited { reason }));
        });
        ProgramContent { pid, socket, paused: false }
    }

    fn signal(&self, sig: libc::c_int) {
        // SAFETY: signalling a pid we spawned; the kernel validates the target.
        unsafe { libc::kill(self.pid as libc::pid_t, sig) };
    }
}

impl Drop for ProgramContent {
    fn drop(&mut self) {
        self.signal(libc::SIGCONT);
        self.signal(libc::SIGTERM);
        let pid = self.pid;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            // SAFETY: as in `signal`; a stale pid is rejected by the kernel.
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        });
        if let Some(parent) = self.socket.parent().and_then(|p| p.downcast::<gtk::Container>().ok()) {
            parent.remove(&self.socket);
        }
    }
}

impl Content for ProgramContent {
    fn set_paused(&mut self, paused: bool) {
        if paused != self.paused {
            self.paused = paused;
            self.signal(if paused { libc::SIGSTOP } else { libc::SIGCONT });
        }
    }

    fn set_volume(&mut self, _volume: u8) {}

    fn set_muted(&mut self, _muted: bool) {}

    fn seek(&mut self, _seek: Seek) {}

    fn apply(&mut self, _name: &str, _control: &Control, _value: Option<&Value>) {}

    fn screenshot(&mut self, path: PathBuf) {
        let _ = path;
    }

    fn pointer(&mut self, _ev: PointerEvent) {}

    fn set_input_enabled(&mut self, _enabled: bool) {}

    fn audio_data(&mut self, _bins: &[f32]) {}
}
