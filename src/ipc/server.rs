use crate::error::{Error, Result};
use crate::ipc::{Event, Request, Response, read_line, write_line};
use interprocess::local_socket::prelude::*;
use interprocess::local_socket::{Listener, ListenerOptions, SendHalf, Stream};
use std::io::BufReader;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

/// Answers a request; may be invoked later than the request arrived.
pub type Reply = Box<dyn FnOnce(Response) + Send + 'static>;

pub type Dispatch = Arc<dyn Fn(Request, Reply) + Send + Sync + 'static>;

#[derive(Clone, Default)]
pub struct Server {
    subscribers: Arc<Mutex<Vec<Sender<Event>>>>,
}

impl Server {
    /// Bind the daemon socket and serve connections on background threads.
    /// Fails with `Error::Ipc` when another daemon already owns the socket.
    pub fn start(dispatch: Dispatch) -> Result<Server> {
        let listener = bind()?;
        let server = Server::default();
        let subs = server.subscribers.clone();
        std::thread::Builder::new()
            .name("ipc-accept".into())
            .spawn(move || {
                for conn in listener.incoming() {
                    match conn {
                        Ok(stream) => {
                            log::debug!("ipc: connection accepted");
                            serve(stream, dispatch.clone(), subs.clone());
                        }
                        Err(e) => log::warn!("ipc accept: {e}"),
                    }
                }
            })
            .map_err(|e| Error::Ipc(e.to_string()))?;
        Ok(server)
    }

    pub fn broadcast(&self, ev: Event) {
        if let Ok(mut subs) = self.subscribers.lock() {
            subs.retain(|s| s.send(ev.clone()).is_ok());
        }
    }
}

/// Bind the daemon socket. A previous daemon that is still shutting down gets a few
/// seconds to release it; a stale socket file is reclaimed.
fn bind() -> Result<Listener> {
    let name = crate::ipc::client::listener_name()?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    loop {
        match ListenerOptions::new().name(name.clone()).create_sync() {
            Ok(l) => return Ok(l),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if Stream::connect(name.clone()).is_ok() {
                    if std::time::Instant::now() > deadline {
                        return Err(Error::Ipc(
                            "another Deadly Wallpaper daemon is already running".into(),
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    continue;
                }
                #[cfg(not(windows))]
                {
                    let _ = std::fs::remove_file(crate::paths::socket_path());
                }
                return ListenerOptions::new()
                    .name(name)
                    .create_sync()
                    .map_err(|e| Error::Ipc(format!("bind daemon socket: {e}")));
            }
            Err(e) => return Err(Error::Ipc(format!("bind daemon socket: {e}"))),
        }
    }
}

fn serve(stream: Stream, dispatch: Dispatch, subs: Arc<Mutex<Vec<Sender<Event>>>>) {
    let spawned = std::thread::Builder::new()
        .name("ipc-conn".into())
        .spawn(move || {
            let (rx, tx) = stream.split();
            let mut rx = BufReader::new(rx);
            let tx: Arc<Mutex<SendHalf>> = Arc::new(Mutex::new(tx));
            loop {
                let req = match read_line::<_, Request>(&mut rx) {
                    Ok(Some(r)) => r,
                    Ok(None) => return,
                    Err(e) => {
                        log::debug!("ipc read: {e}");
                        return;
                    }
                };
                if let Request::Subscribe = req {
                    let (etx, erx) = channel::<Event>();
                    if let Ok(mut s) = subs.lock() {
                        s.push(etx);
                    }
                    if send(&tx, &Response::Ok).is_err() {
                        return;
                    }
                    for ev in erx {
                        if send(&tx, &ev).is_err() {
                            return;
                        }
                    }
                    return;
                }
                log::debug!("ipc: request {req:?}");
                let out = tx.clone();
                let (done_tx, done_rx) = channel::<()>();
                dispatch(
                    req,
                    Box::new(move |resp| {
                        let _ = send(&out, &resp);
                        let _ = done_tx.send(());
                    }),
                );
                if done_rx.recv().is_err() {
                    return;
                }
            }
        });
    if let Err(e) = spawned {
        log::warn!("ipc: connection thread: {e}");
    }
}

fn send<T: serde::Serialize>(tx: &Arc<Mutex<SendHalf>>, msg: &T) -> std::io::Result<()> {
    let mut guard = tx
        .lock()
        .map_err(|_| std::io::Error::other("ipc writer poisoned"))?;
    write_line(&mut *guard, msg).inspect_err(|e| log::debug!("ipc write: {e}"))
}
