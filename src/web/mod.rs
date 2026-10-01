//! Web wallpapers on the platform web view, with Lively's JavaScript API
//! (`livelyPropertyListener`, `livelyWallpaperPlaybackChanged`, `livelyAudioListener`) or
//! Wallpaper Engine's (`wallpaperPropertyListener`, `wallpaperRegisterAudioListener`, ...).

pub mod embedded;
#[cfg(target_os = "linux")]
pub mod serve;

use crate::audio::Spectrum;
use crate::content::{Content, ContentEvent, ContentId, PointerEvent, PointerKind, Seek, View};
use crate::error::{Error, Result};
use crate::geom::Size;
use crate::model::props::ControlKind;
use crate::model::{Control, Kind};
use crate::msg::Msg;
use crate::platform::{ContentSpec, MsgSender, MsgSenderApi};
use crate::we::project::FileType;
use serde_json::Value;
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wry::{PageLoadEvent, WebView, WebViewBuilder};

pub const SCHEME: &str = "wallpaper";

/// Lively's page API.
pub const BRIDGE: &str = r#"(() => {
  const call = (name, ...args) => { const f = window[name]; if (typeof f === 'function') { try { f(...args); } catch (e) { console.error(name, e); } } };
  const media = () => Array.from(document.querySelectorAll('video,audio'));
  let volume = 1, muted = false;
  const applyVolume = () => media().forEach(m => { m.volume = volume; m.muted = muted; });
  window.__dwp = {
    prop: (name, value) => call('livelyPropertyListener', name, value),
    pause: (paused) => { media().forEach(m => { if (paused) m.pause(); else m.play().catch(() => {}); }); call('livelyWallpaperPlaybackChanged', JSON.stringify({ IsPaused: paused })); },
    volume: (v) => { volume = v; applyVolume(); },
    mute: (m) => { muted = m; applyVolume(); },
    audio: (bins) => call('livelyAudioListener', bins),
    mouse: (kind, x, y) => {
      const target = document.elementFromPoint(x, y) || document.body || document.documentElement;
      if (!target) return;
      const init = { bubbles: true, cancelable: true, clientX: x, clientY: y, screenX: x, screenY: y, button: 0, buttons: kind === 'mousemove' ? 0 : 1, view: window };
      target.dispatchEvent(new MouseEvent(kind, init));
      if (kind === 'mouseup') target.dispatchEvent(new MouseEvent('click', init));
    },
    view: (w, h, k, r, x, y, sw, sh) => {
      const s = document.documentElement.style;
      s.width = w + 'px'; s.height = h + 'px'; s.overflow = 'hidden'; s.transformOrigin = '50% 50%';
      s.transform = 'translate(' + (sw / 2 - w / 2 + x) + 'px, ' + (sh / 2 - h / 2 + y) + 'px) rotate(' + r + 'deg) scale(' + k + ')';
    },
    unview: () => {
      const s = document.documentElement.style;
      s.width = ''; s.height = ''; s.overflow = ''; s.transformOrigin = ''; s.transform = '';
    },
  };
  document.addEventListener('DOMContentLoaded', applyVolume);
  document.addEventListener('play', applyVolume, true);
})();"#;

/// Wallpaper Engine's page API.
pub const WE_BRIDGE: &str = include_str!("../../assets/we/bridge.js");

/// The built-in page that draws Wallpaper Engine scenes, served under [`PAGE_PREFIX`].
pub const SCENE_PAGE: &str = "scene.html";
/// Route prefix of the built-in page assets.
pub const PAGE_PREFIX: &str = "__deadlywp/";
/// Route prefix of Wallpaper Engine's assets folder.
pub const ASSETS_PREFIX: &str = "__assets/";
/// Route prefix of granted user files, followed by the absolute path.
pub const FILE_PREFIX: &str = "__file/";

/// How the page must reach a `general` push: the frame-rate limit and the language.
#[derive(Clone, Debug, PartialEq)]
pub struct General {
    pub fps: u32,
    pub language: String,
}

impl General {
    pub fn from_settings(settings: &crate::model::Settings) -> General {
        General {
            fps: settings.wallpaper_engine.fps,
            language: crate::we::language(),
        }
    }

    fn script(&self) -> String {
        format!(
            "__dwp.general({{fps: {}, language: {}}});",
            self.fps,
            js(&Value::String(self.language.clone()))
        )
    }
}

/// The complete initialization script for a page: Wallpaper Engine's bridge with the general
/// properties, or Lively's.
pub fn bridge_for(we: Option<&General>) -> String {
    match we {
        Some(general) => format!("{WE_BRIDGE}\n{}", general.script()),
        None => BRIDGE.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Grant {
    File(PathBuf),
    Dir(PathBuf),
}

/// Files outside the wallpaper folder a page may read: the values of its file and directory
/// controls, keyed by control name so a new value replaces the old grant.
#[derive(Clone, Default)]
pub struct Grants(Arc<Mutex<HashMap<String, Grant>>>);

impl Grants {
    pub fn new() -> Grants {
        Grants::default()
    }

    fn set(&self, control: &str, grant: Option<Grant>) {
        if let Ok(mut all) = self.0.lock() {
            match grant {
                Some(g) => {
                    all.insert(control.to_string(), g);
                }
                None => {
                    all.remove(control);
                }
            }
        }
    }

    /// Allow one file; a path that does not resolve clears the control's grant.
    pub fn set_file(&self, control: &str, path: &Path) {
        self.set(
            control,
            path.canonicalize()
                .ok()
                .filter(|p| p.is_file())
                .map(Grant::File),
        );
    }

    /// Allow every file below one directory.
    pub fn set_dir(&self, control: &str, path: &Path) {
        self.set(
            control,
            path.canonicalize()
                .ok()
                .filter(|p| p.is_dir())
                .map(Grant::Dir),
        );
    }

    pub fn clear(&self, control: &str) {
        self.set(control, None);
    }

    /// The real path of `path` when it is a granted file or lies under a granted directory.
    pub fn granted(&self, path: &Path) -> Option<PathBuf> {
        let real = path.canonicalize().ok().filter(|p| p.is_file())?;
        let allowed = self.0.lock().is_ok_and(|all| {
            all.values().any(|g| match g {
                Grant::File(f) => *f == real,
                Grant::Dir(d) => real.starts_with(d),
            })
        });
        allowed.then_some(real)
    }
}

/// What a page may load: its own folder, Wallpaper Engine's assets, granted user files and
/// the built-in page assets.
#[derive(Clone, Default)]
pub struct Routes {
    /// The wallpaper's folder; `None` for online pages.
    pub root: Option<PathBuf>,
    /// Wallpaper Engine's assets folder, served under [`ASSETS_PREFIX`].
    pub assets: Option<PathBuf>,
    /// Files missing from the root are looked up in the assets folder, the way Wallpaper
    /// Engine combines a scene's own files with its stock assets.
    pub combined: bool,
    pub grants: Grants,
}

impl Routes {
    pub fn for_root(root: impl Into<PathBuf>) -> Routes {
        Routes {
            root: Some(root.into()),
            ..Routes::default()
        }
    }
}

/// What a route resolves to.
#[derive(Debug, PartialEq)]
pub enum Served {
    Embedded {
        mime: &'static str,
        body: &'static [u8],
    },
    File(PathBuf),
    Forbidden,
    NotFound,
}

/// Resolve a request path (without its leading slash, still percent-encoded).
pub fn route(routes: &Routes, path: &str) -> Served {
    let path = path.trim_start_matches('/');
    if let Some(name) = path.strip_prefix(PAGE_PREFIX) {
        return match embedded::get(&percent_decode(name)) {
            Some((mime, body)) => Served::Embedded { mime, body },
            None => Served::NotFound,
        };
    }
    if let Some(rel) = path.strip_prefix(ASSETS_PREFIX) {
        return match &routes.assets {
            Some(assets) => served(resolve(assets, rel)),
            None => Served::NotFound,
        };
    }
    if let Some(rest) = path.strip_prefix(FILE_PREFIX) {
        let decoded = percent_decode(rest);
        if decoded.contains('\0') {
            return Served::Forbidden;
        }
        let absolute = if cfg!(windows) {
            PathBuf::from(decoded)
        } else {
            PathBuf::from(format!("/{}", decoded.trim_start_matches('/')))
        };
        return match routes.grants.granted(&absolute) {
            Some(real) => Served::File(real),
            None if absolute.exists() => Served::Forbidden,
            None => Served::NotFound,
        };
    }
    let Some(root) = &routes.root else {
        return Served::NotFound;
    };
    match resolve(root, path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && routes.combined => {
            match &routes.assets {
                Some(assets) => served(resolve(assets, path)),
                None => Served::NotFound,
            }
        }
        other => served(other),
    }
}

fn served(r: std::io::Result<PathBuf>) -> Served {
    match r {
        Ok(p) if p.is_file() => Served::File(p),
        Ok(_) => Served::NotFound,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Served::Forbidden,
        Err(_) => Served::NotFound,
    }
}

/// URL under which a local wallpaper file is served.
pub fn page_url(relative: &str) -> String {
    let rel = encode_path(relative);
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost/{rel}")
    } else {
        format!("{SCHEME}://localhost/{rel}")
    }
}

fn origin() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

/// The page that shows a wallpaper: where it is plus what the content needs to run it.
pub struct Page {
    /// The page on the custom protocol, or the online URL.
    pub url: String,
    /// The page's path inside the served root, for servers with their own base URL.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(
            dead_code,
            reason = "read by the Plasma presenter, which serves pages from its own base URL"
        )
    )]
    pub relative: Option<String>,
    /// Percent-encoded query the page takes.
    #[cfg_attr(
        not(target_os = "linux"),
        allow(
            dead_code,
            reason = "read by the Plasma presenter, which serves pages from its own base URL"
        )
    )]
    pub query: Option<String>,
    pub kind: Kind,
    /// Wallpaper Engine page: its bridge, its value encoding, its audio feed.
    pub we: bool,
    pub routes: Routes,
}

/// Work out a wallpaper's page. Wallpaper Engine scenes need the assets folder.
pub fn page_for(spec: &ContentSpec<'_>) -> Result<Page> {
    let wp = spec.wallpaper;
    let kind = wp.kind();
    if kind.is_online() {
        return Ok(Page {
            url: wp.source.clone(),
            relative: None,
            query: None,
            kind,
            we: wp.we.is_some(),
            routes: Routes::default(),
        });
    }
    let file = PathBuf::from(&wp.source);
    if !file.is_file() {
        return Err(Error::NotFound(format!(
            "{} does not exist",
            file.display()
        )));
    }
    let root = wp.root_dir();
    let rel = file
        .strip_prefix(&root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| crate::paths::file_name(&file));
    if kind == Kind::Scene {
        let assets = spec.assets.ok_or_else(|| {
            Error::Unsupported(
                "Wallpaper Engine's assets folder was not found: install Wallpaper Engine through Steam or set the assets folder in Settings → Wallpaper Engine".into(),
            )
        })?;
        let relative = format!("{PAGE_PREFIX}{SCENE_PAGE}");
        let query = format!("scene={}", encode_query(&rel));
        return Ok(Page {
            url: format!("{}?{query}", page_url(&relative)),
            relative: Some(relative),
            query: Some(query),
            kind,
            we: true,
            routes: Routes {
                root: Some(root),
                assets: Some(assets.to_path_buf()),
                combined: true,
                grants: Grants::new(),
            },
        });
    }
    Ok(Page {
        url: page_url(&rel),
        relative: Some(rel),
        query: None,
        kind,
        we: wp.we.is_some(),
        routes: Routes::for_root(root),
    })
}

/// Configure a builder for `spec`; the platform attaches it to a slot and hands the page to
/// [`WebContent::new`].
pub fn builder(spec: &ContentSpec<'_>, tx: MsgSender) -> Result<(WebViewBuilder<'static>, Page)> {
    let page = page_for(spec)?;
    let id = spec.id;
    let load_tx = tx.clone();
    let allowed = origin();
    let general = page.we.then(|| General::from_settings(spec.settings));
    let mut b = WebViewBuilder::new()
        .with_url(&page.url)
        .with_initialization_script(bridge_for(general.as_ref()))
        .with_autoplay(true)
        .with_devtools(spec.settings.web.devtools)
        .with_hotkeys_zoom(false)
        .with_focused(false)
        .with_accept_first_mouse(true)
        .with_background_color((0, 0, 0, 255))
        .with_new_window_req_handler(|_, _| wry::NewWindowResponse::Deny)
        .with_on_page_load_handler(move |event, _| {
            if let PageLoadEvent::Finished = event {
                load_tx.send(Msg::Content(id, ContentEvent::Loaded));
            }
        });
    if page.routes.root.is_some() {
        let routes = page.routes.clone();
        b = b
            .with_custom_protocol(SCHEME.into(), move |_, req| serve(&routes, req))
            .with_navigation_handler(move |u| {
                u.starts_with(&allowed) || u.starts_with("about:") || u.starts_with("data:")
            });
    }
    Ok((b, page))
}

fn serve(routes: &Routes, req: http::Request<Vec<u8>>) -> http::Response<Cow<'static, [u8]>> {
    match route(routes, req.uri().path()) {
        Served::Embedded { mime, body } => http::Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-cache")
            .body(Cow::Borrowed(body))
            .unwrap_or_else(|_| status(500, "response")),
        Served::File(path) => match std::fs::read(&path) {
            Ok(body) => http::Response::builder()
                .status(200)
                .header("Content-Type", content_type(&path))
                .header("Access-Control-Allow-Origin", "*")
                .header("Cache-Control", "no-cache")
                .body(Cow::Owned(body))
                .unwrap_or_else(|_| status(500, "response")),
            Err(_) => status(404, "not found"),
        },
        Served::Forbidden => status(403, "forbidden"),
        Served::NotFound => status(404, "not found"),
    }
}

fn status(code: u16, text: &'static str) -> http::Response<Cow<'static, [u8]>> {
    http::Response::builder()
        .status(code)
        .header("Content-Type", "text/plain")
        .body(Cow::Borrowed(text.as_bytes()))
        .expect("static response")
}

pub fn content_type(path: &Path) -> String {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    if mime.type_() == mime_guess::mime::TEXT
        || mime.subtype() == mime_guess::mime::JAVASCRIPT
        || mime.subtype() == mime_guess::mime::JSON
    {
        format!("{mime}; charset=utf-8")
    } else {
        mime.to_string()
    }
}

pub fn encode_path(relative: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for b in relative.replace('\\', "/").trim_start_matches('/').bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(b as char);
        } else {
            write!(encoded, "%{b:02X}").expect("writing to String");
        }
    }
    encoded
}

/// Percent-encode a query value.
fn encode_query(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::new();
    for b in value.replace('\\', "/").bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~') {
            encoded.push(b as char);
        } else {
            write!(encoded, "%{b:02X}").expect("writing to String");
        }
    }
    encoded
}

fn resolve(root: &Path, relative: &str) -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    use std::path::Component;
    let relative = percent_decode(relative);
    if relative.contains(['\\', ':', '\0'])
        || Path::new(&relative)
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "path outside wallpaper folder",
        ));
    }
    let root = root.canonicalize()?;
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(&root) {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "path outside wallpaper folder",
        ));
    }
    Ok(path)
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let (Some(h), Some(l)) = (
                hex(bytes.get(i + 1).copied()),
                hex(bytes.get(i + 2).copied()),
            ) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<u8>) -> Option<u8> {
    match b? {
        c @ b'0'..=b'9' => Some(c - b'0'),
        c @ b'a'..=b'f' => Some(c - b'a' + 10),
        c @ b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn js(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// A Wallpaper Engine property push worked out from a control: the value in Wallpaper
/// Engine's encoding and, for directory controls, the files the page may pick from.
#[derive(Clone, Debug, PartialEq)]
pub struct WeUpdate {
    pub value: Value,
    /// `(files, fetchall)` for directory controls.
    pub files: Option<(Vec<String>, bool)>,
}

/// Extensions a file or directory control accepts: its own filter, or Wallpaper Engine's
/// file type.
fn accepted_extensions(control: &Control, filter: &[String]) -> Vec<String> {
    if !filter.is_empty() {
        return filter
            .iter()
            .map(|f| f.trim_start_matches('.').to_ascii_lowercase())
            .collect();
    }
    FileType::parse(control.we.as_ref().and_then(|m| m.filetype.as_deref()))
        .extensions()
        .iter()
        .map(|e| e.to_string())
        .collect()
}

/// The files directly inside `dir` whose extension is in `extensions`, sorted by name, as
/// absolute paths.
pub fn list_dir(dir: &Path, extensions: &[String]) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) || e.path().is_file())
        .map(|e| e.path())
        .filter(|p| {
            extensions.is_empty()
                || p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
        })
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    files
        .into_iter()
        .map(|p| {
            std::path::absolute(&p)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// A path from a file or directory control: absolute, or relative to the wallpaper.
/// Rebuilt from its components so every separator is the platform's own: project files and
/// pages name files with `/`, which Windows keeps verbatim when joined.
fn user_path(root: Option<&Path>, value: &str) -> PathBuf {
    let p: PathBuf = Path::new(value).components().collect();
    if p.is_absolute() {
        return p;
    }
    match root {
        Some(r) => r.join(p),
        None => p,
    }
}

/// Convert a control change into what a Wallpaper Engine page receives, granting the files
/// it names. `None` for controls that carry nothing to push.
pub fn we_apply(
    root: Option<&Path>,
    grants: &Grants,
    name: &str,
    control: &Control,
    value: Option<&Value>,
) -> Option<WeUpdate> {
    match &control.kind {
        ControlKind::Label { .. } => return None,
        ControlKind::Button { .. } => {
            return Some(WeUpdate {
                value: Value::Bool(true),
                files: None,
            });
        }
        _ => {}
    }
    let value = crate::we::project::we_value(control, value)?;
    match &control.kind {
        ControlKind::File { .. } => {
            let raw = value.as_str().unwrap_or("").trim();
            if raw.is_empty() {
                grants.clear(name);
                return Some(WeUpdate {
                    value: Value::String(String::new()),
                    files: None,
                });
            }
            let path = user_path(root, raw);
            grants.set_file(name, &path);
            Some(WeUpdate {
                value: Value::String(path.to_string_lossy().into_owned()),
                files: None,
            })
        }
        ControlKind::Folder { filter, .. } => {
            let raw = value.as_str().unwrap_or("").trim();
            let fetchall = control.we.as_ref().is_some_and(|m| m.fetchall);
            if raw.is_empty() {
                grants.clear(name);
                return Some(WeUpdate {
                    value: Value::String(String::new()),
                    files: Some((Vec::new(), fetchall)),
                });
            }
            let dir = user_path(root, raw);
            grants.set_dir(name, &dir);
            let files = list_dir(&dir, &accepted_extensions(control, filter));
            Some(WeUpdate {
                value: Value::String(dir.to_string_lossy().into_owned()),
                files: Some((files, fetchall)),
            })
        }
        _ => Some(WeUpdate { value, files: None }),
    }
}

/// Places the native web view for a view; each platform supplies its own.
pub type ViewHook = Box<dyn FnMut(&View) -> Result<()>>;

/// A web wallpaper instance.
pub struct WebContent {
    webview: WebView,
    kind: Kind,
    /// Wallpaper Engine page: values, files and audio go through its bridge.
    we: bool,
    root: Option<PathBuf>,
    grants: Grants,
    id: ContentId,
    tx: MsgSender,
    input: bool,
    slot: Size,
    view: View,
    hook: Option<ViewHook>,
    /// The page itself is transformed with CSS, on platforms where the page keeps the image's
    /// viewport while the window shows only this display's part.
    css: bool,
}

impl WebContent {
    pub fn new(
        webview: WebView,
        page: Page,
        id: ContentId,
        tx: MsgSender,
        slot: Size,
    ) -> WebContent {
        WebContent {
            webview,
            kind: page.kind,
            we: page.we,
            root: page.routes.root,
            grants: page.routes.grants,
            id,
            tx,
            input: true,
            slot,
            view: View::whole(slot),
            hook: None,
            css: false,
        }
    }

    pub fn with_view_hook(mut self, hook: ViewHook) -> WebContent {
        self.hook = Some(hook);
        self
    }

    #[cfg_attr(
        not(windows),
        allow(
            dead_code,
            reason = "only WebView2 can lay the page out at the image's size inside a smaller window"
        )
    )]
    pub fn with_css_view(mut self, css: bool) -> WebContent {
        self.css = css;
        self
    }

    fn eval(&self, script: &str) {
        if let Err(e) = self.webview.evaluate_script(script) {
            log::debug!("webview eval: {e}");
        }
    }

    fn push_css_view(&self) {
        let v = &self.view;
        if v.is_whole(self.slot) {
            self.eval("__dwp.unview()");
        } else {
            self.eval(&format!(
                "__dwp.view({}, {}, {}, {}, {}, {}, {}, {})",
                v.width, v.height, v.scale, v.rotation, v.x, v.y, self.slot.w, self.slot.h
            ));
        }
    }
}

impl Content for WebContent {
    fn set_paused(&mut self, paused: bool) {
        self.eval(&format!("__dwp.pause({paused})"));
    }

    fn set_volume(&mut self, volume: u8) {
        self.eval(&format!("__dwp.volume({})", volume.min(100) as f64 / 100.0));
    }

    fn set_muted(&mut self, muted: bool) {
        self.eval(&format!("__dwp.mute({muted})"));
    }

    fn seek(&mut self, seek: Seek) {
        if let Seek::Absolute(p) = seek {
            if p == 0.0 {
                let _ = self.webview.reload();
            }
        }
    }

    fn apply(&mut self, name: &str, control: &Control, value: Option<&Value>) {
        if self.we {
            let Some(update) = we_apply(self.root.as_deref(), &self.grants, name, control, value)
            else {
                return;
            };
            if let Some((files, fetchall)) = &update.files {
                self.eval(&format!(
                    "__dwp.dir({}, {}, {fetchall})",
                    js(&Value::String(name.into())),
                    js(&Value::Array(
                        files.iter().map(|f| Value::String(f.clone())).collect()
                    ))
                ));
            }
            let mut batch = serde_json::Map::new();
            batch.insert(name.to_string(), update.value);
            self.eval(&format!("__dwp.props({})", js(&Value::Object(batch))));
            return;
        }
        let v = match (&control.kind, value) {
            (ControlKind::Button { .. }, _) => Value::Bool(true),
            (ControlKind::Label { .. }, _) => return,
            (_, Some(v)) => v.clone(),
            (_, None) => return,
        };
        self.eval(&format!(
            "__dwp.prop({}, {})",
            js(&Value::String(name.into())),
            js(&v)
        ));
    }

    fn screenshot(&mut self, path: PathBuf) {
        let tx = self.tx.clone();
        let id = self.id;
        let p = path.clone();
        snapshot(
            &self.webview,
            path,
            Box::new(move |result| {
                tx.send(Msg::Content(
                    id,
                    ContentEvent::Screenshot { path: p, result },
                ))
            }),
        );
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
        self.eval(&format!("__dwp.mouse('{kind}', {}, {})", ev.x, ev.y));
    }

    fn set_input_enabled(&mut self, enabled: bool) {
        self.input = enabled;
        set_native_input(&self.webview, enabled);
    }

    fn audio_data(&mut self, spectrum: &Spectrum) {
        let bins = if self.we {
            &spectrum.we
        } else {
            &spectrum.lively
        };
        let mut s = String::with_capacity(bins.len() * 8 + 16);
        s.push_str("__dwp.audio([");
        for (i, b) in bins.iter().enumerate() {
            if i > 0 {
                s.push(',');
            }
            s.push_str(&format!("{:.4}", b));
        }
        s.push_str("])");
        self.eval(&s);
    }

    fn media(&mut self, event: &crate::nowplaying::MediaEvent) {
        if !self.we {
            return;
        }
        self.eval(&format!(
            "__dwp.media({}, {})",
            js(&Value::String(event.name().into())),
            js(&event.payload())
        ));
    }

    fn set_view(&mut self, view: &View) -> Result<()> {
        if let Some(hook) = self.hook.as_mut() {
            hook(view)?;
        }
        #[cfg(target_os = "macos")]
        {
            // The slot view is framed for the view; the page fills the image inside it.
            let bounds = wry::Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(view.width as f64, view.height as f64).into(),
            };
            self.webview
                .set_bounds(bounds)
                .map_err(|e| Error::Web(format!("web view bounds: {e}")))?;
        }
        self.view = *view;
        if self.css {
            self.push_css_view();
        }
        Ok(())
    }
}

type Done = Box<dyn FnOnce(Result<()>) + Send + 'static>;

#[cfg(target_os = "linux")]
fn set_native_input(webview: &WebView, enabled: bool) {
    use gtk::prelude::WidgetExt;
    use wry::WebViewExtUnix;
    webview.webview().set_sensitive(enabled);
}

#[cfg(not(target_os = "linux"))]
fn set_native_input(_webview: &WebView, _enabled: bool) {}

#[cfg(target_os = "linux")]
fn snapshot(webview: &WebView, path: PathBuf, done: Done) {
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};
    use wry::WebViewExtUnix;
    let wv = webview.webview();
    wv.snapshot(
        SnapshotRegion::Visible,
        SnapshotOptions::NONE,
        None::<&webkit2gtk::gio::Cancellable>,
        move |res| {
            let result = (|| -> Result<()> {
                let surface = res.map_err(|e| Error::Web(format!("snapshot: {e}")))?;
                let image = cairo::ImageSurface::try_from(surface)
                    .map_err(|_| Error::Web("snapshot is not an image surface".into()))?;
                let (w, h) = (image.width(), image.height());
                let pixbuf = gdk::pixbuf_get_from_surface(&image, 0, 0, w, h)
                    .ok_or_else(|| Error::Web("snapshot conversion failed".into()))?;
                save_pixbuf(&pixbuf, &path)
            })();
            done(result);
        },
    );
}

/// Save a pixbuf as PNG or JPEG (by extension); JPEG drops the alpha channel.
#[cfg(target_os = "linux")]
pub fn save_pixbuf(pixbuf: &gdk_pixbuf::Pixbuf, path: &Path) -> Result<()> {
    let (w, h) = (pixbuf.width() as u32, pixbuf.height() as u32);
    let stride = pixbuf.rowstride() as usize;
    let channels = pixbuf.n_channels() as usize;
    let bytes = pixbuf.read_pixel_bytes();
    let mut rgba = image::RgbaImage::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * stride + x * channels;
            let a = if channels == 4 { bytes[i + 3] } else { 255 };
            rgba.put_pixel(
                x as u32,
                y as u32,
                image::Rgba([bytes[i], bytes[i + 1], bytes[i + 2], a]),
            );
        }
    }
    crate::capture::save_rgba(rgba, path)
}

#[cfg(windows)]
fn snapshot(webview: &WebView, path: PathBuf, done: Done) {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG,
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
    };
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::System::Com::{STGM_CREATE, STGM_WRITE};
    use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
    use windows::core::PCWSTR;
    use wry::WebViewExtWindows;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let format = if ext == "jpg" || ext == "jpeg" {
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG
    } else {
        COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG
    };
    use std::os::windows::ffi::OsStrExt;
    let controller = webview.controller();
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let done = std::sync::Mutex::new(Some(done));
    // SAFETY: COM calls on the thread that owns the controller; WebView2 keeps the stream and
    // handler alive until the capture completes.
    let attempt: Result<()> = unsafe {
        (|| {
            let core = controller
                .CoreWebView2()
                .map_err(|e| Error::Web(format!("webview2: {e}")))?;
            let stream = SHCreateStreamOnFileEx(
                PCWSTR(wide.as_ptr()),
                (STGM_CREATE | STGM_WRITE).0,
                FILE_ATTRIBUTE_NORMAL.0,
                true,
                None,
            )
            .map_err(|e| Error::Web(format!("create {}: {e}", path.display())))?;
            let finished = std::sync::Arc::new(done);
            let handler = CapturePreviewCompletedHandler::create(Box::new(
                move |hr: windows::core::Result<()>| {
                    let result = hr.map_err(|e| Error::Web(format!("capture: {e}")));
                    if let Some(d) = finished.lock().ok().and_then(|mut g| g.take()) {
                        d(result);
                    }
                    Ok(())
                },
            ));
            core.CapturePreview(format, &stream, &handler)
                .map_err(|e| Error::Web(format!("capture preview: {e}")))
        })()
    };
    if let Err(e) = attempt {
        log::warn!("{e}");
    }
}

#[cfg(target_os = "macos")]
fn snapshot(webview: &WebView, path: PathBuf, done: Done) {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError, NSString};
    use wry::WebViewExtMacOS;
    let wv = webview.webview();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_ascii_lowercase();
    let file_type = if ext == "jpg" || ext == "jpeg" {
        NSBitmapImageFileType::JPEG
    } else {
        NSBitmapImageFileType::PNG
    };
    let done = std::sync::Mutex::new(Some(done));
    let block = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
        let result = (|| -> Result<()> {
            if image.is_null() {
                let msg = if error.is_null() {
                    "snapshot failed".to_string()
                } else {
                    // SAFETY: WebKit hands us a live NSError for the duration of the callback.
                    unsafe { (*error).localizedDescription().to_string() }
                };
                return Err(Error::Web(msg));
            }
            // SAFETY: WebKit hands us a live NSImage for the duration of the callback.
            let image: &NSImage = unsafe { &*image };
            let tiff = image
                .TIFFRepresentation()
                .ok_or_else(|| Error::Web("snapshot has no bitmap".into()))?;
            let rep = NSBitmapImageRep::imageRepWithData(&tiff)
                .ok_or_else(|| Error::Web("snapshot decode failed".into()))?;
            let props: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
            // SAFETY: valid representation and an empty (default) properties dictionary.
            let data = unsafe { rep.representationUsingType_properties(file_type, &props) }
                .ok_or_else(|| Error::Web("snapshot encode failed".into()))?;
            let ok =
                data.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true);
            if ok {
                Ok(())
            } else {
                Err(Error::Web(format!("write {}", path.display())))
            }
        })();
        if let Some(d) = done.lock().ok().and_then(|mut g| g.take()) {
            d(result);
        }
    });
    // SAFETY: the web view is alive and owned by this content; the block is retained by WebKit.
    unsafe { wv.takeSnapshotWithConfiguration_completionHandler(None, &block) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::props::WeMeta;
    use serde_json::json;

    fn request(url: &str) -> http::Request<Vec<u8>> {
        http::Request::builder().uri(url).body(Vec::new()).unwrap()
    }

    #[test]
    fn serves_filenames_with_url_characters() {
        let root = tempfile::tempdir().unwrap();
        let routes = Routes::for_root(root.path());
        let mut names = vec!["night #1 & 100%+ü=é.html"];
        if cfg!(not(windows)) {
            names.push("day ?2 * 3.html");
        }
        for name in names {
            std::fs::write(root.path().join(name), name).unwrap();
            let response = serve(&routes, request(&page_url(name)));
            assert_eq!(response.status(), 200, "{name}");
            assert_eq!(response.body().as_ref(), name.as_bytes(), "{name}");
        }
    }

    #[test]
    fn rejects_paths_outside_the_wallpaper_folder() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "../secret",
            "%2e%2e/secret",
            "..%5csecret",
            "%2Fsecret",
            "C%3A/secret",
            "%00",
        ] {
            assert_eq!(
                resolve(root.path(), path).unwrap_err().kind(),
                std::io::ErrorKind::PermissionDenied,
                "{path}"
            );
        }
        assert_eq!(
            resolve(root.path(), "missing").unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_must_stay_inside_the_wallpaper_folder() {
        let root = tempfile::tempdir().unwrap();
        let site = root.path().join("site");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(root.path().join("secret"), "secret").unwrap();
        std::fs::write(site.join("asset"), "asset").unwrap();
        std::os::unix::fs::symlink(root.path().join("secret"), site.join("outside")).unwrap();
        std::os::unix::fs::symlink(site.join("asset"), site.join("inside")).unwrap();
        assert_eq!(
            resolve(&site, "outside").unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            std::fs::read(resolve(&site, "inside").unwrap()).unwrap(),
            b"asset"
        );
    }

    /// The `__file` route of an absolute path, encoded the way the bridge encodes it.
    fn file_route(path: &Path) -> String {
        let text = path.to_string_lossy().replace('\\', "/");
        let encoded = text
            .split('/')
            .map(|seg| {
                seg.bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("/");
        format!("{FILE_PREFIX}{}", encoded.trim_start_matches('/'))
    }

    #[test]
    fn granted_files_are_served_and_nothing_else() {
        let home = tempfile::tempdir().unwrap();
        let pictures = home.path().join("my pictures");
        std::fs::create_dir_all(pictures.join("sub")).unwrap();
        let photo = pictures.join("photo #1.png");
        std::fs::write(&photo, b"png").unwrap();
        std::fs::write(pictures.join("sub").join("deep.jpg"), b"jpg").unwrap();
        let secret = home.path().join("secret.txt");
        std::fs::write(&secret, b"secret").unwrap();
        let site = home.path().join("site");
        std::fs::create_dir(&site).unwrap();
        let routes = Routes::for_root(&site);

        assert_eq!(route(&routes, &file_route(&photo)), Served::Forbidden);
        assert_eq!(
            route(&routes, &file_route(&home.path().join("nope.png"))),
            Served::NotFound
        );

        routes.grants.set_file("image", &photo);
        assert_eq!(
            route(&routes, &file_route(&photo)),
            Served::File(photo.canonicalize().unwrap())
        );
        assert_eq!(route(&routes, &file_route(&secret)), Served::Forbidden);
        assert_eq!(
            route(&routes, &file_route(&pictures.join("sub").join("deep.jpg"))),
            Served::Forbidden
        );
        let traversal = format!(
            "{}/../../secret.txt",
            file_route(&pictures.join("sub")).trim_end_matches('/')
        );
        assert_eq!(route(&routes, &traversal), Served::Forbidden);

        routes.grants.set_dir("folder", &pictures);
        assert_eq!(
            route(&routes, &file_route(&pictures.join("sub").join("deep.jpg"))),
            Served::File(
                pictures
                    .join("sub")
                    .join("deep.jpg")
                    .canonicalize()
                    .unwrap()
            )
        );
        assert_eq!(route(&routes, &traversal), Served::Forbidden);
        assert_eq!(route(&routes, &file_route(&pictures)), Served::Forbidden);
        routes.grants.clear("image");
        routes.grants.clear("folder");
        assert_eq!(route(&routes, &file_route(&photo)), Served::Forbidden);
        let uri = format!("{}/{}", origin(), file_route(&photo));
        assert_eq!(serve(&routes, request(&uri)).status(), 403);
        routes.grants.set_file("image", &photo);
        let response = serve(&routes, request(&uri));
        assert_eq!(response.status(), 200);
        assert_eq!(response.body().as_ref(), b"png");
    }

    #[test]
    fn scenes_fall_back_to_the_assets_folder() {
        let dir = tempfile::tempdir().unwrap();
        let scene = dir.path().join("scene");
        let assets = dir.path().join("assets");
        std::fs::create_dir_all(scene.join("materials")).unwrap();
        std::fs::create_dir_all(assets.join("shaders")).unwrap();
        std::fs::write(scene.join("materials").join("own.json"), b"own").unwrap();
        std::fs::write(assets.join("shaders").join("generic.frag"), b"frag").unwrap();
        let combined = Routes {
            root: Some(scene.clone()),
            assets: Some(assets.clone()),
            combined: true,
            grants: Grants::new(),
        };
        assert_eq!(
            route(&combined, "materials/own.json"),
            Served::File(
                scene
                    .join("materials")
                    .join("own.json")
                    .canonicalize()
                    .unwrap()
            )
        );
        assert_eq!(
            route(&combined, "shaders/generic.frag"),
            Served::File(
                assets
                    .join("shaders")
                    .join("generic.frag")
                    .canonicalize()
                    .unwrap()
            )
        );
        assert_eq!(
            route(&combined, "__assets/shaders/generic.frag"),
            Served::File(
                assets
                    .join("shaders")
                    .join("generic.frag")
                    .canonicalize()
                    .unwrap()
            )
        );
        assert_eq!(
            route(&combined, "__assets/../scene/materials/own.json"),
            Served::Forbidden
        );
        assert_eq!(route(&combined, "shaders/missing.frag"), Served::NotFound);
        let plain = Routes {
            combined: false,
            ..combined.clone()
        };
        assert_eq!(route(&plain, "shaders/generic.frag"), Served::NotFound);
        let no_assets = Routes::for_root(&scene);
        assert_eq!(
            route(&no_assets, "__assets/shaders/generic.frag"),
            Served::NotFound
        );
    }

    #[test]
    fn built_in_pages_are_served() {
        let routes = Routes::default();
        match route(&routes, "__deadlywp/scene.html") {
            Served::Embedded { mime, body } => {
                assert!(mime.starts_with("text/html"), "{mime}");
                assert!(!body.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(route(&routes, "__deadlywp/nope.txt"), Served::NotFound);
        assert_eq!(route(&routes, "index.html"), Served::NotFound);
        let response = serve(&routes, request(&page_url("__deadlywp/scene.html")));
        assert_eq!(response.status(), 200);
        assert!(
            bridge_for(Some(&General {
                fps: 30,
                language: "de-de".into()
            }))
            .ends_with("__dwp.general({fps: 30, language: \"de-de\"});")
        );
        assert_eq!(bridge_for(None), BRIDGE);
    }

    fn control(kind: ControlKind, we: Option<WeMeta>) -> Control {
        Control {
            text: String::new(),
            help: None,
            condition: None,
            we,
            kind,
        }
    }

    fn meta(kind: &str) -> WeMeta {
        WeMeta {
            kind: kind.into(),
            values: Vec::new(),
            filetype: None,
            fetchall: false,
        }
    }

    #[test]
    fn wallpaper_engine_values_files_and_folders_are_converted() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("wallpaper");
        std::fs::create_dir_all(root.join("materials")).unwrap();
        std::fs::write(root.join("materials").join("default.png"), b"png").unwrap();
        let pictures = dir.path().join("pictures");
        std::fs::create_dir(&pictures).unwrap();
        for name in ["b.jpg", "a.PNG", "notes.txt", "clip.mp4"] {
            std::fs::write(pictures.join(name), b"x").unwrap();
        }
        std::fs::create_dir(pictures.join("nested")).unwrap();
        std::fs::write(pictures.join("nested").join("deep.png"), b"x").unwrap();
        let grants = Grants::new();

        let color = control(
            ControlKind::Color {
                value: "#ffffff".into(),
            },
            Some(meta("color")),
        );
        let u = we_apply(
            Some(&root),
            &grants,
            "tint",
            &color,
            Some(&json!("#ff8000")),
        )
        .unwrap();
        assert_eq!(u.value, json!("1 0.501961 0"));
        assert_eq!(u.files, None);

        let combo = control(
            ControlKind::Dropdown {
                value: 0,
                items: vec!["Slow".into(), "Fast".into()],
            },
            Some(WeMeta {
                values: vec![json!("slow"), json!("fast")],
                ..meta("combo")
            }),
        );
        let u = we_apply(Some(&root), &grants, "speed", &combo, Some(&json!(1))).unwrap();
        assert_eq!(u.value, json!("fast"));

        let file = control(
            ControlKind::File {
                value: "materials/default.png".into(),
                filter: vec![],
            },
            Some(WeMeta {
                filetype: Some("image".into()),
                ..meta("file")
            }),
        );
        let u = we_apply(
            Some(&root),
            &grants,
            "image",
            &file,
            Some(&json!("materials/default.png")),
        )
        .unwrap();
        let default = root.join("materials").join("default.png");
        assert_eq!(u.value, json!(default.to_string_lossy()));
        assert!(grants.granted(&default).is_some());
        let u = we_apply(
            Some(&root),
            &grants,
            "image",
            &file,
            Some(&json!(pictures.join("b.jpg").to_string_lossy())),
        )
        .unwrap();
        assert_eq!(u.value, json!(pictures.join("b.jpg").to_string_lossy()));
        assert!(grants.granted(&pictures.join("b.jpg")).is_some());
        assert!(grants.granted(&default).is_none());
        let u = we_apply(Some(&root), &grants, "image", &file, Some(&json!(""))).unwrap();
        assert_eq!(u.value, json!(""));
        assert!(grants.granted(&pictures.join("b.jpg")).is_none());

        let folder = control(
            ControlKind::Folder {
                value: String::new(),
                filter: vec![],
            },
            Some(WeMeta {
                filetype: Some("image".into()),
                fetchall: true,
                ..meta("directory")
            }),
        );
        let u = we_apply(
            Some(&root),
            &grants,
            "slides",
            &folder,
            Some(&json!(pictures.to_string_lossy())),
        )
        .unwrap();
        assert_eq!(u.value, json!(pictures.to_string_lossy()));
        let (files, fetchall) = u.files.unwrap();
        assert!(fetchall);
        assert_eq!(
            files,
            vec![
                pictures.join("a.PNG").to_string_lossy().into_owned(),
                pictures.join("b.jpg").to_string_lossy().into_owned(),
            ]
        );
        assert!(
            grants
                .granted(&pictures.join("nested").join("deep.png"))
                .is_some()
        );
        assert!(
            grants
                .granted(&root.join("materials").join("default.png"))
                .is_none()
        );

        let videos = control(
            ControlKind::Folder {
                value: String::new(),
                filter: vec!["MP4".into()],
            },
            Some(meta("directory")),
        );
        let u = we_apply(
            Some(&root),
            &grants,
            "clips",
            &videos,
            Some(&json!(pictures.to_string_lossy())),
        )
        .unwrap();
        assert_eq!(
            u.files,
            Some((
                vec![pictures.join("clip.mp4").to_string_lossy().into_owned()],
                false
            ))
        );
        assert_eq!(list_dir(&pictures, &[]).len(), 4);

        let label = control(
            ControlKind::Label {
                value: String::new(),
            },
            None,
        );
        assert_eq!(
            we_apply(None, &grants, "l", &label, Some(&json!("x"))),
            None
        );
        let button = control(
            ControlKind::Button {
                value: String::new(),
            },
            None,
        );
        assert_eq!(
            we_apply(None, &grants, "b", &button, None).map(|u| u.value),
            Some(json!(true))
        );
    }
}
