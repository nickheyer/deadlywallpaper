//! The control window: an egui client of the daemon.

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
    Event(Event),
    /// Outcome of a background request, labeled for the toast.
    Done { label: String, result: Box<Result<Response>> },
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
            .with_inner_size([1180.0, 760.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(icon),
        persist_window: true,
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

/// Request plumbing shared by the UI: one blocking connection for quick calls and a
/// subscription thread that turns daemon events into repaints.
pub struct Backend {
    client: Option<Client>,
    tx: Sender<UiMsg>,
    pub rx: Receiver<UiMsg>,
}

impl Backend {
    pub fn connect(ctx: egui::Context) -> Backend {
        let (tx, rx) = channel();
        let mut backend = Backend { client: None, tx, rx };
        backend.reconnect(&ctx);
        backend
    }

    pub fn reconnect(&mut self, ctx: &egui::Context) {
        if self.client.is_some() {
            return;
        }
        if let Err(e) = crate::ensure_daemon() {
            log::warn!("{e}");
            return;
        }
        let Ok(client) = Client::connect_within(Duration::from_secs(3)) else { return };
        self.client = Some(client);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            if let Ok(sub) = Client::connect().and_then(Client::subscribe) {
                for ev in sub {
                    let _ = tx.send(UiMsg::Event(ev));
                    ctx.request_repaint();
                }
            }
            let _ = tx.send(UiMsg::Disconnected);
            ctx.request_repaint();
        });
    }

    pub fn connected(&self) -> bool {
        self.client.is_some()
    }

    pub fn drop_connection(&mut self) {
        self.client = None;
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
