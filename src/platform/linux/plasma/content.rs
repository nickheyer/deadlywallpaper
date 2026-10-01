//! Plasma wallpaper controlled through containment settings and a loopback event stream.

use crate::content::{Content, ContentEvent, ContentId, PointerEvent, PointerKind, Seek};
use crate::error::{Error, Result};
use crate::model::props::ControlKind;
use crate::model::settings::Scaler;
use crate::model::{Control, Kind};
use crate::msg::Msg;
use crate::platform::linux::MsgSender;
use crate::platform::linux::plasma::script::{self, Client, Val};
use crate::platform::linux::plasma::{Memory, Shell, Slot};
use crate::platform::{ContentSpec, MsgSenderApi};
use crate::web::serve::Server;
use crate::web::{self, Grants, Routes};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub fn spawn(
    spec: &ContentSpec<'_>,
    slot: &Slot,
    tx: MsgSender,
    shell: &Shell,
) -> Result<Box<dyn Content>> {
    let wp = spec.wallpaper;
    let kind = wp.kind();
    if kind == Kind::Program {
        return Err(Error::Unsupported("program wallpapers cannot run on KDE Plasma: plasmashell draws the desktop and cannot embed another application's window".into()));
    }
    if !kind.is_online() && !Path::new(&wp.source).is_file() {
        return Err(Error::NotFound(format!("{} does not exist", wp.source)));
    }
    let generation = next_generation();
    let mut values: Vec<(&'static str, Val)> = vec![
        ("Impl", Val::Str(shell.impl_name().to_string())),
        ("Kind", Val::Str(kind.to_string())),
        ("Title", Val::Str(wp.title())),
        ("Fit", Val::Str(fit_name(spec.settings.video.scaler).into())),
        ("Paused", Val::Bool(false)),
        ("Muted", Val::Bool(!spec.audio)),
        ("Volume", Val::Num(spec.volume.min(100) as f64 / 100.0)),
        ("Speed", Val::Num(1.0)),
        (
            "LoopBlend",
            Val::Num(crate::media::looper::DEFAULT_BLEND_SECS),
        ),
        ("Saturation", Val::Int(0)),
        ("Hue", Val::Int(0)),
        ("Brightness", Val::Int(0)),
        ("Contrast", Val::Int(0)),
        ("Gamma", Val::Int(0)),
        // The view follows through `set_view` as soon as the instance exists.
        ("RegionW", Val::Int(0)),
        ("RegionH", Val::Int(0)),
        ("ViewX", Val::Num(0.0)),
        ("ViewY", Val::Num(0.0)),
        ("ViewScale", Val::Num(1.0)),
        ("ViewRotation", Val::Num(0.0)),
        ("Seek", Val::Str(String::new())),
        ("Screenshot", Val::Str(String::new())),
        ("ScreenshotResult", Val::Str(String::new())),
        ("State", Val::Str(String::new())),
        ("Bridge", Val::Str(String::new())),
    ];
    let serve = shell.serve().clone();
    let web = kind.is_web();
    let mut we = false;
    let mut routes = Routes::default();
    if web {
        let page = web::page_for(spec)?;
        we = page.we;
        routes = page.routes.clone();
        let token = serve.register(spec.id, page.routes);
        let source = match &page.relative {
            None => page.url.clone(),
            Some(relative) => {
                let mut url = serve.page_url(spec.id, relative);
                if let Some(query) = &page.query {
                    url.push('?');
                    url.push_str(query);
                }
                url
            }
        };
        values.push(("Source", Val::Str(source)));
        let general = we.then(|| web::General::from_settings(spec.settings));
        values.push((
            "Bridge",
            Val::Str(bridge_script(
                &serve.events_url(spec.id, &token),
                general.as_ref(),
            )),
        ));
    } else if kind != Kind::VideoStream {
        values.push(("Source", Val::Str(file_url(&wp.source))));
    }
    let content = PlasmaContent {
        client: shell.client().clone(),
        serve,
        memory: shell.memory().clone(),
        containments: slot.containments.clone(),
        id: spec.id,
        kind,
        we,
        root: routes.root.clone(),
        grants: routes.grants.clone(),
        tx: tx.clone(),
        generation,
        serial: 0,
        stop: Arc::new(AtomicBool::new(false)),
        input: true,
        engine_muted: !spec.audio,
        user_muted: false,
    };
    if kind == Kind::VideoStream {
        let (client, containments, memory, id, url, quality) = (
            content.client.clone(),
            content.containments.clone(),
            content.memory.clone(),
            spec.id,
            wp.source.clone(),
            spec.settings.video.stream_quality,
        );
        let stop = content.stop.clone();
        let resolve_tx = tx.clone();
        std::thread::Builder::new()
            .name("plasma-stream".into())
            .spawn(
                move || match crate::engine::stream::direct_url(&url, quality) {
                    Ok(direct) => {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        values.push(("Source", Val::Str(direct)));
                        apply_all(
                            &client,
                            &containments,
                            &values,
                            generation,
                            &memory,
                            &resolve_tx,
                            id,
                        );
                    }
                    Err(e) => resolve_tx.send(Msg::Content(
                        id,
                        ContentEvent::Exited {
                            reason: e.to_string(),
                        },
                    )),
                },
            )
            .map_err(|e| Error::Platform(e.to_string()))?;
    } else {
        apply_all(
            &content.client,
            &content.containments,
            &values,
            generation,
            &content.memory,
            &tx,
            spec.id,
        );
    }
    content.watch_state(Duration::from_secs(
        spec.settings.video.load_timeout_secs + 10,
    ));
    Ok(Box::new(content))
}

/// Millisecond-based generation fitting Plasma's 32-bit key; increment on collisions.
fn next_generation() -> i64 {
    use std::sync::atomic::AtomicI64;
    static LAST: AtomicI64 = AtomicI64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_millis() % 1_000_000_000) as i64)
        .unwrap_or(1)
        .max(1);
    LAST.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| {
        Some(if now > last { now } else { last + 1 })
    })
    .map(|last| if now > last { now } else { last + 1 })
    .unwrap_or(now)
}

fn fit_name(scaler: Scaler) -> &'static str {
    match scaler {
        Scaler::None => "none",
        Scaler::Fill => "fill",
        Scaler::Uniform => "uniform",
        Scaler::UniformFill => "uniformfill",
    }
}

fn file_url(path: &str) -> String {
    let abs = std::path::absolute(path)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string());
    let mut out = String::from("file://");
    for b in abs.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The page API (Lively's, or Wallpaper Engine's with its general properties) plus the event
/// stream that carries engine pushes into the page.
fn bridge_script(events_url: &str, general: Option<&web::General>) -> String {
    format!(
        "{}\n(() => {{\n  const es = new EventSource({});\n  const parse = (e) => JSON.parse(e.data);\n  const on = (name, fn) => es.addEventListener(name, e => {{ try {{ fn(parse(e)); }} catch (err) {{ console.error(name, err); }} }});\n  on('prop', m => __dwp.prop(m.name, m.value));\n  on('props', m => __dwp.props(m));\n  on('general', m => __dwp.general(m));\n  on('dir', m => __dwp.dir(m.name, m.files, m.fetchall));\n  on('media', m => __dwp.media(m.name, m.payload));\n  on('pause', p => __dwp.pause(p));\n  on('volume', v => __dwp.volume(v));\n  on('mute', m => __dwp.mute(m));\n  on('audio', a => __dwp.audio(a));\n  on('pointer', p => __dwp.mouse(p.k, p.x, p.y));\n  es.addEventListener('reload', () => location.reload());\n}})();",
        web::bridge_for(general),
        script::js_str(events_url)
    )
}

fn apply_all(
    client: &Client,
    containments: &[i32],
    values: &[(&'static str, Val)],
    generation: i64,
    memory: &Arc<Memory>,
    tx: &MsgSender,
    id: ContentId,
) {
    for &c in containments {
        let (memory, tx) = (memory.clone(), tx.clone());
        client.eval_then(script::apply(c, values, generation), move |r| match r {
            Ok(previous) => memory.record(c, &previous),
            Err(e) => tx.send(Msg::Content(
                id,
                ContentEvent::Exited {
                    reason: e.to_string(),
                },
            )),
        });
    }
}

pub struct PlasmaContent {
    client: Client,
    serve: Server,
    memory: Arc<Memory>,
    containments: Vec<i32>,
    id: ContentId,
    kind: Kind,
    /// Wallpaper Engine page: values, files and audio go through its bridge.
    we: bool,
    root: Option<PathBuf>,
    grants: Grants,
    tx: MsgSender,
    generation: i64,
    serial: u64,
    stop: Arc<AtomicBool>,
    input: bool,
    engine_muted: bool,
    user_muted: bool,
}

impl PlasmaContent {
    fn write(&self, values: &[(&'static str, Val)]) {
        for &c in &self.containments {
            self.client.eval(script::write(c, values));
        }
    }

    fn push(&self, event: &str, data: &str, retain: Option<&str>) {
        if self.kind.is_web() {
            self.serve.push(self.id, event, data, retain);
        }
    }

    fn write_muted(&self) {
        let muted = self.engine_muted || self.user_muted;
        self.write(&[("Muted", Val::Bool(muted))]);
        self.push("mute", &muted.to_string(), Some("mute"));
    }

    /// Poll the wallpaper's reported state until it plays or fails.
    fn watch_state(&self, timeout: Duration) {
        let Some(&containment) = self.containments.first() else {
            return;
        };
        let (client, tx, id, generation, stop) = (
            self.client.clone(),
            self.tx.clone(),
            self.id,
            self.generation,
            self.stop.clone(),
        );
        let _ = std::thread::Builder::new().name("plasma-state".into()).spawn(move || {
            let deadline = Instant::now() + timeout;
            let prefix = format!("{generation}|");
            loop {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                match client.eval_sync(script::read(containment, &["State"]), Duration::from_secs(5)).and_then(|out| serde_json::from_str::<Value>(&out).map_err(|e| Error::Platform(e.to_string()))) {
                    Ok(v) => {
                        let state = v["State"].as_str().unwrap_or("");
                        if let Some(rest) = state.strip_prefix(&prefix) {
                            if rest == "playing" {
                                tx.send(Msg::Content(id, ContentEvent::Loaded));
                                return;
                            }
                            if let Some(msg) = rest.strip_prefix("error|") {
                                tx.send(Msg::Content(id, ContentEvent::Exited { reason: msg.to_string() }));
                                return;
                            }
                        }
                        if v["plugin"].as_str().is_some_and(|p| p != script::PLUGIN) && Instant::now() > deadline - timeout + Duration::from_secs(5) {
                            tx.send(Msg::Content(id, ContentEvent::Exited { reason: "Plasma switched the desktop to another wallpaper plugin".into() }));
                            return;
                        }
                    }
                    Err(e) => log::debug!("plasma state: {e}"),
                }
                if Instant::now() > deadline {
                    tx.send(Msg::Content(id, ContentEvent::Exited { reason: format!("plasmashell did not report the wallpaper within {}s", timeout.as_secs()) }));
                    return;
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        });
    }
}

impl Drop for PlasmaContent {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self.kind.is_web() {
            self.serve.unregister(self.id);
        }
    }
}

impl Content for PlasmaContent {
    fn set_paused(&mut self, paused: bool) {
        self.write(&[("Paused", Val::Bool(paused))]);
        self.push("pause", &paused.to_string(), Some("pause"));
    }

    fn set_volume(&mut self, volume: u8) {
        let v = volume.min(100) as f64 / 100.0;
        self.write(&[("Volume", Val::Num(v))]);
        self.push("volume", &format!("{v}"), Some("volume"));
    }

    fn set_muted(&mut self, muted: bool) {
        self.engine_muted = muted;
        self.write_muted();
    }

    /// The plugin sizes the content to the image and transforms it about its centre.
    fn set_view(&mut self, view: &crate::content::View) -> Result<()> {
        self.write(&[
            ("RegionW", Val::Int(view.width as i64)),
            ("RegionH", Val::Int(view.height as i64)),
            ("ViewX", Val::Num(view.x)),
            ("ViewY", Val::Num(view.y)),
            ("ViewScale", Val::Num(view.scale)),
            ("ViewRotation", Val::Num(view.rotation)),
        ]);
        Ok(())
    }

    fn seek(&mut self, seek: Seek) {
        if self.kind == Kind::Picture {
            return;
        }
        if self.kind.is_web() {
            if seek == Seek::Absolute(0.0) {
                self.push("reload", "1", None);
            }
            return;
        }
        self.serial += 1;
        let request = match seek {
            Seek::Absolute(p) => format!("{}:absolute:{p}", self.serial),
            Seek::Relative(p) => format!("{}:relative:{p}", self.serial),
        };
        self.write(&[("Seek", Val::Str(request))]);
    }

    fn apply(&mut self, name: &str, control: &Control, value: Option<&Value>) {
        if self.we {
            let Some(update) =
                web::we_apply(self.root.as_deref(), &self.grants, name, control, value)
            else {
                return;
            };
            if let Some((files, fetchall)) = &update.files {
                let data =
                    serde_json::json!({ "name": name, "files": files, "fetchall": fetchall })
                        .to_string();
                self.push("dir", &data, Some(&format!("dir:{name}")));
            }
            let data = serde_json::json!({ name: update.value }).to_string();
            self.push("props", &data, Some(&format!("prop:{name}")));
            return;
        }
        if self.kind.is_web() {
            let v = match (&control.kind, value) {
                (ControlKind::Button { .. }, _) => Value::Bool(true),
                (ControlKind::Label { .. }, _) => return,
                (_, Some(v)) => v.clone(),
                (_, None) => return,
            };
            let data = serde_json::json!({ "name": name, "value": v }).to_string();
            self.push("prop", &data, Some(&format!("prop:{name}")));
            return;
        }
        let Some(value) = value else { return };
        match name {
            "saturation" | "hue" | "brightness" | "contrast" | "gamma" => {
                if let Some(v) = value.as_f64() {
                    let key = match name {
                        "saturation" => "Saturation",
                        "hue" => "Hue",
                        "brightness" => "Brightness",
                        "contrast" => "Contrast",
                        _ => "Gamma",
                    };
                    self.write(&[(key, Val::Int(v.round().clamp(-100.0, 100.0) as i64))]);
                }
            }
            "speed" => {
                if let Some(v) = value.as_f64() {
                    self.write(&[("Speed", Val::Num(v.clamp(0.05, 16.0)))]);
                }
            }
            crate::media::looper::BLEND_CONTROL => {
                if let Some(v) = value.as_f64() {
                    self.write(&[("LoopBlend", Val::Num(v.max(0.0)))]);
                }
            }
            "scaler" => {
                if let Some(i) = value.as_i64() {
                    self.write(&[("Fit", Val::Str(fit_name(Scaler::from_index(i)).into()))]);
                }
            }
            "mute" => {
                if let Some(b) = value.as_bool() {
                    self.user_muted = b;
                    self.write_muted();
                }
            }
            other => log::warn!("media wallpapers on Plasma have no '{other}' control"),
        }
    }

    fn screenshot(&mut self, path: PathBuf) {
        let Some(&containment) = self.containments.first() else {
            self.tx.send(Msg::Content(
                self.id,
                ContentEvent::Screenshot {
                    path,
                    result: Err(Error::Platform("no desktop containment".into())),
                },
            ));
            return;
        };
        self.serial += 1;
        let serial = self.serial;
        self.write(&[(
            "Screenshot",
            Val::Str(format!("{serial}|{}", path.to_string_lossy())),
        )]);
        let (client, tx, id, stop) = (
            self.client.clone(),
            self.tx.clone(),
            self.id,
            self.stop.clone(),
        );
        let _ = std::thread::Builder::new()
            .name("plasma-shot".into())
            .spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(8);
                let prefix = format!("{serial}|");
                loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    if let Ok(v) = client
                        .eval_sync(
                            script::read(containment, &["ScreenshotResult"]),
                            Duration::from_secs(5),
                        )
                        .and_then(|out| {
                            serde_json::from_str::<Value>(&out)
                                .map_err(|e| Error::Platform(e.to_string()))
                        })
                    {
                        if let Some(rest) = v["ScreenshotResult"]
                            .as_str()
                            .and_then(|s| s.strip_prefix(&prefix))
                        {
                            let result = if rest == "ok" {
                                Ok(())
                            } else {
                                Err(Error::Platform(
                                    rest.trim_start_matches("error|").to_string(),
                                ))
                            };
                            tx.send(Msg::Content(id, ContentEvent::Screenshot { path, result }));
                            return;
                        }
                    }
                    if Instant::now() > deadline {
                        tx.send(Msg::Content(
                            id,
                            ContentEvent::Screenshot {
                                path,
                                result: Err(Error::Platform(
                                    "plasmashell did not capture the wallpaper in time".into(),
                                )),
                            },
                        ));
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            });
    }

    fn pointer(&mut self, ev: PointerEvent) {
        if !self.input || !self.kind.accepts_pointer() {
            return;
        }
        let kind = match ev.kind {
            PointerKind::Move => "mousemove",
            PointerKind::Down => "mousedown",
            PointerKind::Up => "mouseup",
        };
        self.push(
            "pointer",
            &format!("{{\"k\":\"{kind}\",\"x\":{},\"y\":{}}}", ev.x, ev.y),
            None,
        );
    }

    fn set_input_enabled(&mut self, enabled: bool) {
        self.input = enabled;
    }

    fn media(&mut self, event: &crate::nowplaying::MediaEvent) {
        if !self.we {
            return;
        }
        let data =
            serde_json::json!({ "name": event.name(), "payload": event.payload() }).to_string();
        self.push("media", &data, Some(&format!("media:{}", event.name())));
    }

    fn audio_data(&mut self, spectrum: &crate::audio::Spectrum) {
        let bins = if self.we {
            &spectrum.we
        } else {
            &spectrum.lively
        };
        let mut s = String::with_capacity(bins.len() * 8 + 2);
        s.push('[');
        for (i, b) in bins.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{b:.4}"));
        }
        s.push(']');
        self.push("audio", &s, None);
    }
}
