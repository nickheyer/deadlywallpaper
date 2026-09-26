//! Web wallpapers on the platform web view, with Lively's JavaScript API:
//! `livelyPropertyListener`, `livelyWallpaperPlaybackChanged`, `livelyAudioListener`.

use crate::content::{Content, ContentEvent, ContentId, PointerEvent, PointerKind, Seek};
use crate::error::{Error, Result};
use crate::model::props::ControlKind;
use crate::model::{Control, Kind};
use crate::msg::Msg;
use crate::platform::{ContentSpec, MsgSender, MsgSenderApi};
use serde_json::Value;
use std::borrow::Cow;
use std::path::{Path, PathBuf};
use wry::{PageLoadEvent, WebView, WebViewBuilder};

#[cfg(target_os = "linux")]
pub mod serve;

pub const SCHEME: &str = "wallpaper";

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
  };
  document.addEventListener('DOMContentLoaded', applyVolume);
  document.addEventListener('play', applyVolume, true);
})();"#;

/// URL under which a local wallpaper file is served.
pub fn page_url(relative: &str) -> String {
    let rel = relative.replace('\\', "/");
    let rel = rel.trim_start_matches('/');
    if cfg!(windows) { format!("http://{SCHEME}.localhost/{rel}") } else { format!("{SCHEME}://localhost/{rel}") }
}

fn origin() -> String {
    if cfg!(windows) { format!("http://{SCHEME}.localhost") } else { format!("{SCHEME}://localhost") }
}

/// Configure a builder for `spec`; the platform attaches it to a slot.
pub fn builder(spec: &ContentSpec<'_>, tx: MsgSender) -> Result<WebViewBuilder<'static>> {
    let wp = spec.wallpaper;
    let kind = wp.kind();
    let id = spec.id;
    let (url, root) = if kind.is_online() {
        (wp.source.clone(), None)
    } else {
        let file = PathBuf::from(&wp.source);
        if !file.is_file() {
            return Err(Error::NotFound(format!("{} does not exist", file.display())));
        }
        let root = wp.root_dir();
        let rel = file.strip_prefix(&root).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| crate::paths::file_name(&file));
        (page_url(&rel), Some(root))
    };
    let load_tx = tx.clone();
    let allowed = origin();
    let mut b = WebViewBuilder::new()
        .with_url(&url)
        .with_initialization_script(BRIDGE)
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
    if let Some(root) = root {
        b = b
            .with_custom_protocol(SCHEME.into(), move |_, req| serve(&root, req))
            .with_navigation_handler(move |u| u.starts_with(&allowed) || u.starts_with("about:") || u.starts_with("data:"));
    }
    Ok(b)
}

fn serve(root: &Path, req: http::Request<Vec<u8>>) -> http::Response<Cow<'static, [u8]>> {
    let rel = percent_decode(req.uri().path()).trim_start_matches('/').to_string();
    let mut path = root.to_path_buf();
    for part in rel.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return status(403, "forbidden");
        }
        path.push(part);
    }
    match std::fs::read(&path) {
        Ok(body) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let ct = if mime.type_() == mime_guess::mime::TEXT || mime.subtype() == mime_guess::mime::JAVASCRIPT || mime.subtype() == mime_guess::mime::JSON {
                format!("{mime}; charset=utf-8")
            } else {
                mime.to_string()
            };
            http::Response::builder()
                .status(200)
                .header("Content-Type", ct)
                .header("Access-Control-Allow-Origin", "*")
                .header("Cache-Control", "no-cache")
                .body(Cow::Owned(body))
                .unwrap_or_else(|_| status(500, "response"))
        }
        Err(_) => status(404, "not found"),
    }
}

fn status(code: u16, text: &'static str) -> http::Response<Cow<'static, [u8]>> {
    http::Response::builder()
        .status(code)
        .header("Content-Type", "text/plain")
        .body(Cow::Borrowed(text.as_bytes()))
        .expect("static response")
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if let (Some(h), Some(l)) = (hex(bytes.get(i + 1).copied()), hex(bytes.get(i + 2).copied())) {
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

fn js(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// A web wallpaper instance.
pub struct WebContent {
    webview: WebView,
    kind: Kind,
    id: ContentId,
    tx: MsgSender,
    input: bool,
}

impl WebContent {
    pub fn new(webview: WebView, kind: Kind, id: ContentId, tx: MsgSender) -> WebContent {
        WebContent { webview, kind, id, tx, input: true }
    }

    fn eval(&self, script: &str) {
        if let Err(e) = self.webview.evaluate_script(script) {
            log::debug!("webview eval: {e}");
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
        let v = match (&control.kind, value) {
            (ControlKind::Button { .. }, _) => Value::Bool(true),
            (ControlKind::Label { .. }, _) => return,
            (_, Some(v)) => v.clone(),
            (_, None) => return,
        };
        self.eval(&format!("__dwp.prop({}, {})", js(&Value::String(name.into())), js(&v)));
    }

    fn screenshot(&mut self, path: PathBuf) {
        let tx = self.tx.clone();
        let id = self.id;
        let p = path.clone();
        snapshot(&self.webview, path, Box::new(move |result| tx.send(Msg::Content(id, ContentEvent::Screenshot { path: p, result }))));
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

    fn audio_data(&mut self, bins: &[f32]) {
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
    wv.snapshot(SnapshotRegion::Visible, SnapshotOptions::NONE, None::<&webkit2gtk::gio::Cancellable>, move |res| {
        let result = (|| -> Result<()> {
            let surface = res.map_err(|e| Error::Web(format!("snapshot: {e}")))?;
            let image = cairo::ImageSurface::try_from(surface).map_err(|_| Error::Web("snapshot is not an image surface".into()))?;
            let (w, h) = (image.width(), image.height());
            let pixbuf = gdk::pixbuf_get_from_surface(&image, 0, 0, w, h).ok_or_else(|| Error::Web("snapshot conversion failed".into()))?;
            save_pixbuf(&pixbuf, &path)
        })();
        done(result);
    });
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
            rgba.put_pixel(x as u32, y as u32, image::Rgba([bytes[i], bytes[i + 1], bytes[i + 2], a]));
        }
    }
    crate::capture::save_rgba(rgba, path)
}

#[cfg(windows)]
fn snapshot(webview: &WebView, path: PathBuf, done: Done) {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG};
    use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
    use windows::Win32::System::Com::{STGM_CREATE, STGM_WRITE};
    use windows::Win32::UI::Shell::SHCreateStreamOnFileEx;
    use windows::core::PCWSTR;
    use wry::WebViewExtWindows;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let format = if ext == "jpg" || ext == "jpeg" { COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_JPEG } else { COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG };
    use std::os::windows::ffi::OsStrExt;
    let controller = webview.controller();
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let done = std::sync::Mutex::new(Some(done));
    // SAFETY: COM calls on the thread that owns the controller; WebView2 keeps the stream and
    // handler alive until the capture completes.
    let attempt: Result<()> = unsafe {
        (|| {
            let core = controller.CoreWebView2().map_err(|e| Error::Web(format!("webview2: {e}")))?;
            let stream = SHCreateStreamOnFileEx(PCWSTR(wide.as_ptr()), (STGM_CREATE | STGM_WRITE).0, FILE_ATTRIBUTE_NORMAL.0, true, None)
                .map_err(|e| Error::Web(format!("create {}: {e}", path.display())))?;
            let finished = std::sync::Arc::new(done);
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |hr: windows::core::Result<()>| {
                let result = hr.map_err(|e| Error::Web(format!("capture: {e}")));
                if let Some(d) = finished.lock().ok().and_then(|mut g| g.take()) {
                    d(result);
                }
                Ok(())
            }));
            core.CapturePreview(format, &stream, &handler).map_err(|e| Error::Web(format!("capture preview: {e}")))
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
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let file_type = if ext == "jpg" || ext == "jpeg" { NSBitmapImageFileType::JPEG } else { NSBitmapImageFileType::PNG };
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
            let tiff = image.TIFFRepresentation().ok_or_else(|| Error::Web("snapshot has no bitmap".into()))?;
            let rep = NSBitmapImageRep::imageRepWithData(&tiff).ok_or_else(|| Error::Web("snapshot decode failed".into()))?;
            let props: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
            // SAFETY: valid representation and an empty (default) properties dictionary.
            let data = unsafe { rep.representationUsingType_properties(file_type, &props) }.ok_or_else(|| Error::Web("snapshot encode failed".into()))?;
            let ok = data.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true);
            if ok { Ok(()) } else { Err(Error::Web(format!("write {}", path.display()))) }
        })();
        if let Some(d) = done.lock().ok().and_then(|mut g| g.take()) {
            d(result);
        }
    });
    // SAFETY: the web view is alive and owned by this content; the block is retained by WebKit.
    unsafe { wv.takeSnapshotWithConfiguration_completionHandler(None, &block) };
}
