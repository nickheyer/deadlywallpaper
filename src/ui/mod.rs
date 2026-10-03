mod about;
mod align;
mod app;
mod customize;
mod edit;
mod fonts;
mod library;
mod order;
mod screens;
mod settings;
mod theme;
mod widgets;
mod workshop;

use crate::error::{Error, Result};
use crate::ipc::client::Client;
use crate::ipc::{Event, Request, Response};
use crate::paths::Paths;
use eframe::egui;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

pub enum UiMsg {
    Connected(Box<Client>),
    ConnectFailed(String),
    Event(Event),
    Done {
        label: String,
        result: Box<Result<Response>>,
    },
    /// Outcome of a [`Backend::batch`]: how many requests succeeded and why the rest failed.
    Batch {
        verb: &'static str,
        noun: &'static str,
        done: usize,
        failed: Vec<String>,
    },
    /// Workshop browsing, preview and status results.
    Workshop(workshop::Msg),
    Disconnected,
}

pub fn run() -> Result<()> {
    let paths = Paths::discover()?;
    crate::logger::init(None, true);
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png"))
        .map_err(|e| Error::Platform(format!("icon: {e}")))?;
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
            fonts::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, paths)))
        }),
    )
    .map_err(|e| Error::Platform(format!("window: {e}")))
}

/// Daemon connection and event subscription.
pub struct Backend {
    client: Option<Client>,
    connecting: bool,
    tx: Sender<UiMsg>,
    pub rx: Receiver<UiMsg>,
}

impl Backend {
    pub fn new() -> Backend {
        let (tx, rx) = channel();
        Backend {
            client: None,
            connecting: false,
            tx,
            rx,
        }
    }

    /// Connect on a worker thread; report success or failure through UiMsg.
    pub fn connect(&mut self, ctx: &egui::Context) {
        if self.client.is_some() || self.connecting {
            return;
        }
        self.connecting = true;
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result =
                crate::ensure_daemon().and_then(|_| Client::connect_within(Duration::from_secs(5)));
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

    /// Blocking request.
    pub fn call(&mut self, req: Request) -> Result<Response> {
        let Some(client) = self.client.as_mut() else {
            return Err(Error::Ipc("not connected".into()));
        };
        match client.call(&req) {
            Err(Error::Ipc(m)) => {
                self.client = None;
                Err(Error::Ipc(m))
            }
            other => other,
        }
    }

    /// Run several requests in order on one background connection; the tally comes back as
    /// [`UiMsg::Batch`], worded as "`verb` N `noun`s".
    pub fn batch(
        &self,
        ctx: &egui::Context,
        verb: &'static str,
        noun: &'static str,
        reqs: Vec<Request>,
    ) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut done = 0;
            let mut failed = Vec::new();
            match Client::connect() {
                Ok(mut client) => {
                    for req in &reqs {
                        match client.call(req) {
                            Ok(_) => done += 1,
                            Err(e) => failed.push(e.to_string()),
                        }
                    }
                }
                Err(e) => failed.push(e.to_string()),
            }
            let _ = tx.send(UiMsg::Batch {
                verb,
                noun,
                done,
                failed,
            });
            ctx.request_repaint();
        });
    }

    /// Run `work` on a worker thread and deliver what it returns.
    pub fn spawn(&self, ctx: &egui::Context, work: impl FnOnce() -> UiMsg + Send + 'static) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
    }

    /// Long-running request on its own connection; the result comes back as [`UiMsg::Done`].
    pub fn background(&self, ctx: &egui::Context, label: impl Into<String>, req: Request) {
        let (tx, ctx, label) = (self.tx.clone(), ctx.clone(), label.into());
        std::thread::spawn(move || {
            let result = Client::connect().and_then(|mut c| c.call(&req));
            let _ = tx.send(UiMsg::Done {
                label,
                result: Box::new(result),
            });
            ctx.request_repaint();
        });
    }
}

/// Reveal a file or folder in the system file manager.
pub fn reveal(path: &std::path::Path) {
    let target = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf())
    };
    open_target(&target.to_string_lossy());
}

pub fn open_url(url: &str) {
    open_target(url);
}

fn open_target(target: &str) {
    if let Err(e) = crate::paths::open_external(target) {
        log::warn!("{e}");
    }
}
