//! X11 window monitor over EWMH root and client properties, polled at the rule interval.

use crate::geom::Rect;
use crate::msg::Msg;
use crate::platform::linux::MsgSender;
use crate::platform::{MsgSenderApi, Snapshot, WindowInfo, WindowPlacement};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, MapState, Window};
use x11rb::rust_connection::RustConnection;

struct Atoms {
    client_list: Atom,
    active: Atom,
    current_desktop: Atom,
    wm_type: Atom,
    wm_state: Atom,
    wm_desktop: Atom,
    frame_extents: Atom,
    wm_pid: Atom,
    fullscreen: Atom,
    hidden: Atom,
    max_h: Atom,
    max_v: Atom,
    skip: Vec<Atom>,
}

pub fn start(tx: MsgSender, interval: Arc<AtomicU64>) -> bool {
    let Ok((conn, screen)) = x11rb::connect(None) else { return false };
    let root = conn.setup().roots[screen].root;
    let Some(atoms) = Atoms::intern(&conn) else { return false };
    std::thread::Builder::new()
        .name("ewmh".into())
        .spawn(move || {
            loop {
                match snapshot(&conn, root, &atoms) {
                    Some(s) => tx.send(Msg::Windows(s)),
                    None => log::debug!("ewmh snapshot failed"),
                }
                std::thread::sleep(Duration::from_millis(interval.load(Ordering::Relaxed).max(100)));
            }
        })
        .is_ok()
}

impl Atoms {
    fn intern(conn: &RustConnection) -> Option<Atoms> {
        let atom = |n: &str| conn.intern_atom(false, n.as_bytes()).ok()?.reply().ok().map(|r| r.atom);
        let skip = ["_NET_WM_WINDOW_TYPE_DESKTOP", "_NET_WM_WINDOW_TYPE_DOCK", "_NET_WM_WINDOW_TYPE_TOOLBAR", "_NET_WM_WINDOW_TYPE_MENU",
            "_NET_WM_WINDOW_TYPE_SPLASH", "_NET_WM_WINDOW_TYPE_NOTIFICATION", "_NET_WM_WINDOW_TYPE_TOOLTIP", "_NET_WM_WINDOW_TYPE_DND",
            "_NET_WM_WINDOW_TYPE_COMBO", "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU", "_NET_WM_WINDOW_TYPE_POPUP_MENU"]
            .iter()
            .filter_map(|n| atom(n))
            .collect();
        Some(Atoms {
            client_list: atom("_NET_CLIENT_LIST_STACKING")?,
            active: atom("_NET_ACTIVE_WINDOW")?,
            current_desktop: atom("_NET_CURRENT_DESKTOP")?,
            wm_type: atom("_NET_WM_WINDOW_TYPE")?,
            wm_state: atom("_NET_WM_STATE")?,
            wm_desktop: atom("_NET_WM_DESKTOP")?,
            frame_extents: atom("_NET_FRAME_EXTENTS")?,
            wm_pid: atom("_NET_WM_PID")?,
            fullscreen: atom("_NET_WM_STATE_FULLSCREEN")?,
            hidden: atom("_NET_WM_STATE_HIDDEN")?,
            max_h: atom("_NET_WM_STATE_MAXIMIZED_HORZ")?,
            max_v: atom("_NET_WM_STATE_MAXIMIZED_VERT")?,
            skip,
        })
    }
}

fn prop32(conn: &RustConnection, w: Window, atom: Atom, ty: AtomEnum) -> Vec<u32> {
    conn.get_property(false, w, atom, ty, 0, 4096)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect()))
        .unwrap_or_default()
}

fn class(conn: &RustConnection, w: Window) -> String {
    conn.get_property(false, w, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| {
            let parts: Vec<String> = r.value.split(|b| *b == 0).filter(|s| !s.is_empty()).map(|s| String::from_utf8_lossy(s).into_owned()).collect();
            parts.get(1).or(parts.first()).cloned().unwrap_or_default()
        })
        .unwrap_or_default()
}

fn snapshot(conn: &RustConnection, root: Window, a: &Atoms) -> Option<Snapshot> {
    let active = prop32(conn, root, a.active, AtomEnum::WINDOW).first().copied().unwrap_or(0);
    let current = prop32(conn, root, a.current_desktop, AtomEnum::CARDINAL).first().copied().unwrap_or(0);
    let clients = prop32(conn, root, a.client_list, AtomEnum::WINDOW);
    let mut windows = Vec::new();
    for w in clients {
        let Ok(attrs) = conn.get_window_attributes(w).ok()?.reply() else { continue };
        if attrs.map_state != MapState::VIEWABLE {
            continue;
        }
        let types = prop32(conn, w, a.wm_type, AtomEnum::ATOM);
        if types.iter().any(|t| a.skip.contains(t)) {
            continue;
        }
        let states = prop32(conn, w, a.wm_state, AtomEnum::ATOM);
        if states.contains(&a.hidden) {
            continue;
        }
        let desktop = prop32(conn, w, a.wm_desktop, AtomEnum::CARDINAL).first().copied();
        if desktop.is_some_and(|d| d != current && d != 0xFFFF_FFFF) {
            continue;
        }
        let Ok(geo) = conn.get_geometry(w).ok()?.reply() else { continue };
        let Ok(pos) = conn.translate_coordinates(w, root, 0, 0).ok()?.reply() else { continue };
        let ext = prop32(conn, w, a.frame_extents, AtomEnum::CARDINAL);
        let (l, r, t, b) = if ext.len() == 4 { (ext[0] as i32, ext[1] as i32, ext[2] as i32, ext[3] as i32) } else { (0, 0, 0, 0) };
        let rect = Rect::new(pos.dst_x as i32 - l, pos.dst_y as i32 - t, geo.width as i32 + l + r, geo.height as i32 + t + b);
        windows.push(WindowInfo {
            placement: WindowPlacement::Rect(rect),
            fullscreen: states.contains(&a.fullscreen),
            maximized: states.contains(&a.max_h) && states.contains(&a.max_v),
            focused: w == active,
            app: class(conn, w),
            pid: prop32(conn, w, a.wm_pid, AtomEnum::CARDINAL).first().copied(),
        });
    }
    Some(Snapshot { windows })
}
