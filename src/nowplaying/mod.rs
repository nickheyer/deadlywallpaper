//! Media integration: what the system's media session is playing, delivered in the event
//! shapes Wallpaper Engine gives wallpapers that register media listeners.

use crate::error::Result;
use crate::msg::Msg;
use crate::platform::{MsgSender, MsgSenderApi};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(target_os = "linux")]
mod mpris;
#[cfg(target_os = "linux")]
use mpris as backend;

#[cfg(windows)]
mod smtc;
#[cfg(windows)]
use smtc as backend;

#[cfg(target_os = "macos")]
mod applescript;
#[cfg(target_os = "macos")]
use applescript as backend;

/// How often the session is read.
pub const POLL: Duration = Duration::from_secs(1);

/// Wallpaper Engine's playback states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(into = "u8")]
pub enum Playback {
    #[default]
    Stopped,
    Playing,
    Paused,
}

impl From<Playback> for u8 {
    fn from(p: Playback) -> u8 {
        match p {
            Playback::Stopped => 0,
            Playback::Playing => 1,
            Playback::Paused => 2,
        }
    }
}

/// Text describing the current media.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Properties {
    pub title: String,
    pub artist: String,
    pub sub_title: String,
    pub album_title: String,
    pub album_artist: String,
    /// Comma separated.
    pub genres: String,
    /// `music`, `video` or `image`.
    pub content_type: String,
}

/// The album art as a PNG data URL, with the colours Wallpaper Engine derives from it as
/// CSS `rgb(r, g, b)` strings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnail {
    pub thumbnail: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub tertiary_color: String,
    pub text_color: String,
    pub high_contrast_color: String,
}

/// One media integration event, named as the page API names its listeners.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum MediaEvent {
    Status { enabled: bool },
    Properties(Properties),
    Thumbnail(Thumbnail),
    Playback { state: Playback },
    Timeline { position: f64, duration: f64 },
}

impl MediaEvent {
    /// The listener this event goes to: `status`, `properties`, `thumbnail`, `playback` or
    /// `timeline`.
    pub fn name(&self) -> &'static str {
        match self {
            MediaEvent::Status { .. } => "status",
            MediaEvent::Properties(_) => "properties",
            MediaEvent::Thumbnail(_) => "thumbnail",
            MediaEvent::Playback { .. } => "playback",
            MediaEvent::Timeline { .. } => "timeline",
        }
    }

    pub fn payload(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Where a backend found the album art.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Art {
    /// The picture itself, as the Windows and macOS players hand it over.
    #[cfg_attr(
        target_os = "linux",
        allow(
            dead_code,
            reason = "MPRIS players publish album art as a URL; the other backends send bytes"
        )
    )]
    Bytes(Vec<u8>),
    /// A `file://`, `http(s)://` or `data:` URL, as MPRIS players and Spotify publish it.
    #[cfg_attr(
        windows,
        allow(
            dead_code,
            reason = "the Windows media session hands the picture over as bytes"
        )
    )]
    Url(String),
}

impl Art {
    /// A key that changes when the picture does.
    fn key(&self) -> String {
        match self {
            Art::Bytes(b) => format!("bytes:{}:{:016x}", b.len(), fnv(b)),
            Art::Url(u) => format!("url:{u}"),
        }
    }

    fn bytes(&self) -> Result<Vec<u8>> {
        match self {
            Art::Bytes(b) => Ok(b.clone()),
            Art::Url(u) => {
                if let Some(rest) = u.strip_prefix("data:") {
                    let (_, data) = rest.split_once(',').ok_or_else(|| {
                        crate::error::Error::Invalid("album art data URL has no payload".into())
                    })?;
                    use base64::Engine;
                    return base64::engine::general_purpose::STANDARD
                        .decode(data.trim())
                        .map_err(|e| {
                            crate::error::Error::Invalid(format!("album art data URL: {e}"))
                        });
                }
                if let Some(path) = u.strip_prefix("file://") {
                    let decoded = crate::web::percent_decode(path);
                    return Ok(std::fs::read(&decoded)?);
                }
                let mut response = ureq::get(u)
                    .call()
                    .map_err(|e| crate::error::Error::Network(format!("{u}: {e}")))?;
                response
                    .body_mut()
                    .with_config()
                    .limit(32 * 1024 * 1024)
                    .read_to_vec()
                    .map_err(|e| crate::error::Error::Network(format!("{u}: {e}")))
            }
        }
    }
}

/// One reading of the active media session.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    pub properties: Properties,
    pub art: Option<Art>,
    pub playback: Playback,
    /// Seconds.
    pub position: Option<f64>,
    pub duration: Option<f64>,
}

/// Turns successive readings into the events Wallpaper Engine fires when something changes.
pub struct Tracker {
    last: Option<NowPlaying>,
    art_key: Option<String>,
    sink: Box<dyn FnMut(MediaEvent) + Send>,
}

impl Tracker {
    pub fn new(sink: Box<dyn FnMut(MediaEvent) + Send>) -> Tracker {
        Tracker {
            last: None,
            art_key: None,
            sink,
        }
    }

    /// Compare `now` with the previous reading and emit what changed. `None` means no media
    /// session exists.
    pub fn update(&mut self, now: Option<NowPlaying>) {
        let now = now.unwrap_or_default();
        let first = self.last.is_none();
        let last = self.last.take().unwrap_or_default();
        if first || now.properties != last.properties {
            (self.sink)(MediaEvent::Properties(now.properties.clone()));
        }
        let key = now.art.as_ref().map(Art::key);
        if first || key != self.art_key {
            self.art_key = key;
            let thumbnail = match &now.art {
                Some(art) => match art.bytes().and_then(|b| thumbnail(&b)) {
                    Ok(t) => t,
                    Err(e) => {
                        log::warn!("album art: {e}");
                        Thumbnail::default()
                    }
                },
                None => Thumbnail::default(),
            };
            (self.sink)(MediaEvent::Thumbnail(thumbnail));
        }
        if first || now.playback != last.playback {
            (self.sink)(MediaEvent::Playback {
                state: now.playback,
            });
        }
        if let (Some(position), Some(duration)) = (now.position, now.duration) {
            if first || now.position != last.position || now.duration != last.duration {
                (self.sink)(MediaEvent::Timeline { position, duration });
            }
        }
        self.last = Some(now);
    }
}

/// Watches the system's media session and streams events to the engine.
pub struct Monitor {
    stop: Arc<AtomicBool>,
}

impl Monitor {
    pub fn start(tx: MsgSender) -> Result<Monitor> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut tracker = Tracker::new(Box::new(move |ev| tx.send(Msg::Media(ev))));
        backend::start(
            stop.clone(),
            Box::new(move |reading| tracker.update(reading)),
        )?;
        Ok(Monitor { stop })
    }
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Receives one reading per poll; `None` when no session exists.
pub type Sink = Box<dyn FnMut(Option<NowPlaying>) + Send + 'static>;

/// Shrink the album art, encode it as a PNG data URL and pick its colours the way Wallpaper
/// Engine does: the three most common distinct colours, a text colour that reads on the
/// primary one, and black or white for the strongest contrast.
pub fn thumbnail(bytes: &[u8]) -> Result<Thumbnail> {
    use base64::Engine;
    use image::imageops::FilterType;
    let img = image::load_from_memory(bytes)
        .map_err(|e| crate::error::Error::Media(format!("album art: {e}")))?;
    let small = img.resize(256, 256, FilterType::Triangle).to_rgba8();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(small.clone())
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| crate::error::Error::Media(format!("album art: {e}")))?;
    let data = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    );
    let sample = image::imageops::resize(&small, 32, 32, FilterType::Triangle);
    let [primary, secondary, tertiary] = palette(sample.pixels().map(|p| [p[0], p[1], p[2]]));
    let text = [secondary, tertiary]
        .into_iter()
        .filter(|c| contrast(*c, primary) >= 4.5)
        .max_by(|a, b| contrast(*a, primary).total_cmp(&contrast(*b, primary)))
        .unwrap_or_else(|| high_contrast(primary));
    Ok(Thumbnail {
        thumbnail: data,
        primary_color: css(primary),
        secondary_color: css(secondary),
        tertiary_color: css(tertiary),
        text_color: css(text),
        high_contrast_color: css(high_contrast(primary)),
    })
}

/// The three most common colours that differ visibly from each other. Transparent pixels
/// are skipped; when the picture has fewer distinct colours, the missing ones are lighter
/// and darker versions of the ones it has.
fn palette(pixels: impl Iterator<Item = [u8; 3]>) -> [[u8; 3]; 3] {
    let mut counts: std::collections::HashMap<[u8; 3], (u32, [u64; 3])> =
        std::collections::HashMap::new();
    for p in pixels {
        let key = [p[0] & 0xf0, p[1] & 0xf0, p[2] & 0xf0];
        let e = counts.entry(key).or_insert((0, [0; 3]));
        e.0 += 1;
        for (sum, channel) in e.1.iter_mut().zip(p) {
            *sum += channel as u64;
        }
    }
    let mut ranked: Vec<([u8; 3], u32)> = counts
        .into_iter()
        .map(|(_, (n, sum))| {
            (
                [
                    (sum[0] / n as u64) as u8,
                    (sum[1] / n as u64) as u8,
                    (sum[2] / n as u64) as u8,
                ],
                n,
            )
        })
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out: Vec<[u8; 3]> = Vec::new();
    for (color, _) in &ranked {
        if out.iter().all(|c| distance(*c, *color) >= 60.0) {
            out.push(*color);
        }
        if out.len() == 3 {
            break;
        }
    }
    let base = out.first().copied().unwrap_or([0, 0, 0]);
    let mut shade = 0;
    while out.len() < 3 {
        let step: i32 = 70 + 40 * (shade / 2);
        let candidate = if shade % 2 == 0 {
            shift(base, step)
        } else {
            shift(base, -step)
        };
        shade += 1;
        if out.iter().all(|c| distance(*c, candidate) >= 30.0) || shade > 8 {
            out.push(candidate);
        }
    }
    [out[0], out[1], out[2]]
}

fn shift(c: [u8; 3], by: i32) -> [u8; 3] {
    let f = |v: u8| (v as i32 + by).clamp(0, 255) as u8;
    [f(c[0]), f(c[1]), f(c[2])]
}

fn distance(a: [u8; 3], b: [u8; 3]) -> f64 {
    (0..3)
        .map(|i| (a[i] as f64 - b[i] as f64).powi(2))
        .sum::<f64>()
        .sqrt()
}

fn luminance(c: [u8; 3]) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

/// WCAG contrast ratio.
fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

fn high_contrast(c: [u8; 3]) -> [u8; 3] {
    if contrast(c, [0, 0, 0]) >= contrast(c, [255, 255, 255]) {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    }
}

fn css(c: [u8; 3]) -> String {
    format!("rgb({}, {}, {})", c[0], c[1], c[2])
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(pixels: &[[u8; 3]], w: u32, h: u32) -> Vec<u8> {
        let mut img = image::RgbaImage::new(w, h);
        for (i, p) in img.pixels_mut().enumerate() {
            let c = pixels[i % pixels.len()];
            *p = image::Rgba([c[0], c[1], c[2], 255]);
        }
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn thumbnails_carry_a_data_url_and_ranked_colours() {
        // Mostly dark blue, some yellow, a little red.
        let mut pixels = vec![[10, 20, 120]; 60];
        pixels.extend(std::iter::repeat_n([250, 220, 30], 30));
        pixels.extend(std::iter::repeat_n([200, 20, 20], 10));
        let t = thumbnail(&png(&pixels, 10, 10)).unwrap();
        assert!(t.thumbnail.starts_with("data:image/png;base64,"));
        assert!(t.primary_color.starts_with("rgb("), "{}", t.primary_color);
        let parse = |s: &str| -> [u8; 3] {
            let n: Vec<u8> = s
                .trim_start_matches("rgb(")
                .trim_end_matches(')')
                .split(',')
                .map(|v| v.trim().parse().unwrap())
                .collect();
            [n[0], n[1], n[2]]
        };
        let p = parse(&t.primary_color);
        assert!(p[2] > p[0] && p[2] > p[1], "primary is the blue: {p:?}");
        let s = parse(&t.secondary_color);
        assert!(s[0] > 200 && s[1] > 180, "secondary is the yellow: {s:?}");
        assert_eq!(t.high_contrast_color, "rgb(255, 255, 255)");
        assert!(contrast(parse(&t.text_color), p) >= 4.5);

        let flat = thumbnail(&png(&[[128, 128, 128]], 4, 4)).unwrap();
        assert_eq!(flat.primary_color, "rgb(128, 128, 128)");
        assert_ne!(flat.secondary_color, flat.primary_color);
        assert_ne!(flat.tertiary_color, flat.secondary_color);
    }

    #[test]
    fn tracker_emits_only_what_changed() {
        let seen: Arc<std::sync::Mutex<Vec<&'static str>>> = Arc::default();
        let log = seen.clone();
        let mut t = Tracker::new(Box::new(move |ev| log.lock().unwrap().push(ev.name())));
        let mut np = NowPlaying {
            properties: Properties {
                title: "Song".into(),
                artist: "Band".into(),
                content_type: "music".into(),
                ..Properties::default()
            },
            art: None,
            playback: Playback::Playing,
            position: Some(1.0),
            duration: Some(200.0),
        };
        t.update(Some(np.clone()));
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["properties", "thumbnail", "playback", "timeline"]
        );
        seen.lock().unwrap().clear();
        np.position = Some(2.0);
        t.update(Some(np.clone()));
        assert_eq!(seen.lock().unwrap().as_slice(), ["timeline"]);
        seen.lock().unwrap().clear();
        np.playback = Playback::Paused;
        t.update(Some(np.clone()));
        assert_eq!(seen.lock().unwrap().as_slice(), ["playback"]);
        seen.lock().unwrap().clear();
        np.art = Some(Art::Bytes(png(&[[1, 2, 3]], 2, 2)));
        t.update(Some(np.clone()));
        assert_eq!(seen.lock().unwrap().as_slice(), ["thumbnail"]);
        seen.lock().unwrap().clear();
        t.update(None);
        assert_eq!(
            seen.lock().unwrap().as_slice(),
            ["properties", "thumbnail", "playback"]
        );
        let name = MediaEvent::Timeline {
            position: 1.0,
            duration: 2.0,
        };
        assert_eq!(name.payload(), serde_json::json!({"position": 1.0, "duration": 2.0}));
        assert_eq!(
            MediaEvent::Playback {
                state: Playback::Paused
            }
            .payload(),
            serde_json::json!({"state": 2})
        );
    }
}
