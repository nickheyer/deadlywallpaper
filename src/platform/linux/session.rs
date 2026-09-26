//! Session lock state from systemd-logind.

use crate::msg::Msg;
use crate::platform::MsgSenderApi;
use crate::platform::linux::MsgSender;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

pub fn watch(tx: MsgSender) {
    let _ = std::thread::Builder::new().name("session".into()).spawn(move || {
        if let Err(e) = run(&tx) {
            log::info!("session lock tracking unavailable: {e}");
        }
    });
}

fn run(tx: &MsgSender) -> zbus::Result<()> {
    let conn = Connection::system()?;
    let manager = Proxy::new(&conn, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager")?;
    let path: OwnedObjectPath = match manager.call("GetSessionByPID", &(std::process::id(),)) {
        Ok(p) => p,
        Err(_) => active_session(&conn, &manager)?,
    };
    let session = Proxy::new(&conn, "org.freedesktop.login1", path, "org.freedesktop.login1.Session")?;
    let locked: bool = session.get_property("LockedHint")?;
    tx.send(Msg::Session { locked });
    for change in session.receive_property_changed::<bool>("LockedHint") {
        if let Ok(locked) = change.get() {
            tx.send(Msg::Session { locked });
        }
    }
    Ok(())
}

/// The active graphical session of this user, for daemons started outside a session scope.
fn active_session(conn: &Connection, manager: &Proxy<'_>) -> zbus::Result<OwnedObjectPath> {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let sessions: Vec<(String, u32, String, String, OwnedObjectPath)> = manager.call("ListSessions", &())?;
    let mut fallback = None;
    for (_, session_uid, _, _, path) in sessions {
        if session_uid != uid {
            continue;
        }
        let session = Proxy::new(conn, "org.freedesktop.login1", path.clone(), "org.freedesktop.login1.Session")?;
        let kind: String = session.get_property("Type").unwrap_or_default();
        if !matches!(kind.as_str(), "wayland" | "x11" | "mir") {
            continue;
        }
        if session.get_property::<bool>("Active").unwrap_or(false) {
            return Ok(path);
        }
        fallback.get_or_insert(path);
    }
    fallback.ok_or_else(|| zbus::Error::Failure("no graphical session for this user".into()))
}
