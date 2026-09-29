//! The control window: an egui client of the daemon.

mod about;
mod align;
mod app;
mod customize;
mod library;
mod screens;
mod settings;
mod theme;
mod widgets;

use crate::error::{Error, Result};
use crate::ipc::client::Client;
use crate::ipc::{Event, Request, Response};
use crate::paths::Paths;
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// Messages from worker threads to the UI thread.
pub enum UiMsg {
    /// A connection attempt succeeded; the client is handed over to the UI thread.
    Connected(Box<Client>),
    /// A connection attempt failed with this message.
    ConnectFailed(String),
    Event(Event),
    /// Outcome of a background request, labeled for the toast.
    Done { label: String, result: Box<Result<Response>> },
    /// The event stream ended: the daemon went away.
    Disconnected,
}

pub fn run() -> Result<()> {
    let paths = Paths::discover()?;
    crate::logger::init(None, true);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png")).map_err(|e| Error::Platform(format!("icon: {e}")))?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(crate::paths::APP_NAME)
            .with_app_id(crate::paths::APP_ID)
            .with_inner_size([1220.0, 780.0])
            .with_min_inner_size([880.0, 560.0])
            .with_icon(icon),
        persist_window: true,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        crate::paths::APP_NAME,
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, paths)))
        }),
    )
    .map_err(|e| Error::Platform(format!("window: {e}")))
}

/// Request plumbing shared by the UI: one blocking connection for quick calls, a subscription
/// thread that turns daemon events into repaints, and connection attempts that never block
/// the UI thread.
pub struct Backend {
    client: Option<Client>,
    connecting: bool,
    tx: Sender<UiMsg>,
    pub rx: Receiver<UiMsg>,
}

impl Backend {
    pub fn new() -> Backend {
        let (tx, rx) = channel();
        Backend { client: None, connecting: false, tx, rx }
    }

    /// Start the daemon when needed and connect, on a worker thread. The outcome arrives as
    /// [`UiMsg::Connected`] or [`UiMsg::ConnectFailed`].
    pub fn connect(&mut self, ctx: &egui::Context) {
        if self.client.is_some() || self.connecting {
            return;
        }
        self.connecting = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = crate::ensure_daemon().and_then(|_| Client::connect_within(Duration::from_secs(5)));
            let msg = match result {
                Ok(client) => UiMsg::Connected(Box::new(client)),
                Err(e) => UiMsg::ConnectFailed(e.to_string()),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Adopt a connection made by [`Backend::connect`] and start listening for events.
    pub fn attach(&mut self, ctx: &egui::Context, client: Client) {
        self.client = Some(client);
        self.connecting = false;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            match Client::connect().and_then(Client::subscribe) {
                Ok(events) => {
                    for ev in events {
                        let _ = tx.send(UiMsg::Event(ev));
                        ctx.request_repaint();
                    }
                }
                Err(e) => log::warn!("event subscription: {e}"),
            }
            let _ = tx.send(UiMsg::Disconnected);
            ctx.request_repaint();
        });
    }

    pub fn connection_failed(&mut self) {
        self.connecting = false;
    }

    pub fn disconnect(&mut self) {
        self.client = None;
        self.connecting = false;
    }

    pub fn connected(&self) -> bool {
        self.client.is_some()
    }

    pub fn connecting(&self) -> bool {
        self.connecting
    }

    /// Synchronous request; the daemon answers these immediately.
    pub fn call(&mut self, req: Request) -> Result<Response> {
        let Some(client) = self.client.as_mut() else { return Err(Error::Ipc("not connected".into())) };
        match client.call(&req) {
            Err(Error::Ipc(m)) => {
                self.client = None;
                Err(Error::Ipc(m))
            }
            other => other,
        }
    }

    /// Long-running request on its own connection; the result comes back as [`UiMsg::Done`].
    pub fn background(&self, ctx: &egui::Context, label: impl Into<String>, req: Request) {
        let (tx, ctx, label) = (self.tx.clone(), ctx.clone(), label.into());
        std::thread::spawn(move || {
            let result = Client::connect().and_then(|mut c| c.call(&req));
            let _ = tx.send(UiMsg::Done { label, result: Box::new(result) });
            ctx.request_repaint();
        });
    }
}

/// Reveal a file or folder in the system file manager.
pub fn reveal(path: &std::path::Path) {
    let target = if path.is_dir() { path.to_path_buf() } else { path.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| path.to_path_buf()) };
    open_target(&target.to_string_lossy());
}

pub fn open_url(url: &str) {
    open_target(url);
}

fn open_target(target: &str) {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(windows)]
    let program = "explorer";
    if let Err(e) = std::process::Command::new(program).arg(target).spawn() {
        log::warn!("open {target}: {e}");
    }
}
