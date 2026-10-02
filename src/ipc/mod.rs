pub mod client;
pub mod server;

use crate::error::Error;
use crate::model::{Arrangement, Control, Display, Kind, Layout, Pose, Settings, Summary};
use crate::we::steam::SteamInfo;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Requests from the CLI and UI to the daemon. Newline-delimited JSON, one reply per request.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Status,
    Displays,
    Library,
    Settings,
    SetSettings {
        settings: Settings,
    },
    Layout,
    SetArrangement {
        arrangement: Arrangement,
        display: Option<String>,
    },
    /// Move, scale or rotate the spanning image.
    AlignImage {
        pose: Pose,
    },
    /// Move, scale or rotate one display inside the spanning image; the identity pose puts it
    /// back where the desktop reports it.
    AlignDisplay {
        display: String,
        pose: Pose,
    },
    ResetAlignment,
    /// `target`: wallpaper id, library directory, file path, URL, `random`, or `reload`.
    Set {
        target: String,
        display: Option<String>,
    },
    /// `display: None` closes every wallpaper.
    Close {
        display: Option<String>,
    },
    Import {
        source: String,
    },
    Delete {
        wallpaper: String,
    },
    Export {
        wallpaper: String,
        file: PathBuf,
    },
    EditInfo {
        wallpaper: String,
        patch: InfoPatch,
    },
    /// Controls of the property copy used on `display` (or the running instance).
    Properties {
        wallpaper: String,
        display: Option<String>,
    },
    SetProperty {
        wallpaper: String,
        display: Option<String>,
        name: String,
        value: serde_json::Value,
    },
    ResetProperties {
        wallpaper: String,
        display: Option<String>,
    },
    Seek {
        display: Option<String>,
        value: String,
    },
    /// Absolute `0..=100`, or `+n` / `-n` relative.
    Volume {
        value: String,
    },
    Play {
        play: bool,
    },
    Screenshot {
        display: Option<String>,
        file: PathBuf,
    },
    Thumbnail {
        wallpaper: String,
    },
    AudioDevices,
    /// Steam, Wallpaper Engine, every workshop item Steam has downloaded and every download
    /// under way.
    WorkshopStatus,
    /// Get a workshop item: add it when Steam already holds it, otherwise have the Steam
    /// client subscribe to and download it, then add it as soon as it lands. `display`
    /// applies it there afterwards.
    WorkshopGet {
        id: u64,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        author: Option<String>,
        #[serde(default)]
        display: Option<String>,
    },
    /// Stop a download, unsubscribing again when the daemon subscribed for it.
    WorkshopCancel {
        id: u64,
    },
    /// Download every subscription Steam has not fetched yet, add every downloaded item the
    /// library lacks and refresh the ones Steam updated.
    WorkshopSync,
    OpenUi,
    Subscribe,
    Quit,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Response {
    Ok,
    Error {
        kind: String,
        message: String,
    },
    Status(Status),
    Displays(Vec<Display>),
    Library(Vec<Summary>),
    Settings(Settings),
    Layout(Layout),
    Controls {
        path: PathBuf,
        controls: Vec<(String, Control)>,
    },
    Devices(Vec<AudioDevice>),
    Wallpaper(Summary),
    /// Everything one import brought in.
    Wallpapers(Vec<Summary>),
    Workshop(WorkshopStatus),
    Text(String),
}

impl Response {
    pub fn error(e: &Error) -> Response {
        Response::Error {
            kind: e.kind().into(),
            message: e.to_string(),
        }
    }

    pub fn into_result(self) -> crate::error::Result<Response> {
        match self {
            Response::Error { kind, message } => Err(match kind.as_str() {
                "not-found" => Error::NotFound(message),
                "unsupported" => Error::Unsupported(message),
                "invalid" => Error::Invalid(message),
                "network" => Error::Network(message),
                _ => Error::Ipc(message),
            }),
            r => Ok(r),
        }
    }
}

/// Pushed to subscribed clients whenever daemon state changes.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    Library,
    Layout,
    Displays,
    Settings,
    Playback,
    /// Downloads started, moved, landed or were added; the Steam client came or went; Steam
    /// paths changed.
    Workshop,
    Error {
        message: String,
    },
    Info {
        message: String,
    },
    /// The daemon is exiting for good; clients close rather than reconnect.
    Quit,
}

/// What the presenter on this desktop can do; the UI shows only settings that apply.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Capabilities {
    /// `plasma`, `layer-shell`, `x11`, `win32` or `quartz`.
    pub presenter: String,
    /// Interactive wallpapers receive pointer motion.
    pub pointer_motion: bool,
    /// Interactive wallpapers receive clicks.
    pub pointer_clicks: bool,
    /// Motion is tracked across the whole desktop, so it can keep flowing while another
    /// application is focused.
    pub global_pointer: bool,
    /// Program wallpapers can be embedded.
    pub programs: bool,
    /// Web wallpapers can open developer tools.
    pub web_devtools: bool,
    /// A spanning web wallpaper can be turned by any angle.
    pub rotate_web: bool,
    /// Video and GIF passes can cross-fade into the next one: the presenter composites two
    /// decoders. Without it looping is seamless but cuts.
    pub loop_blend: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub version: String,
    pub platform: String,
    pub session: String,
    pub window_monitor: String,
    pub capabilities: Capabilities,
    pub displays: Vec<Display>,
    pub layout: Layout,
    pub active: Vec<ActiveInfo>,
    /// User-requested pause (tray or `pause` command).
    pub paused: bool,
    pub locked: bool,
    pub on_battery: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActiveInfo {
    pub display: String,
    pub wallpaper: String,
    pub title: String,
    pub kind: Kind,
    pub loaded: bool,
    pub paused: bool,
    pub volume: u8,
    pub customizable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoPatch {
    pub title: Option<String>,
    pub desc: Option<String>,
    pub author: Option<String>,
    pub contact: Option<String>,
    pub license: Option<String>,
    pub arguments: Option<String>,
}

/// Steam's side of the Workshop as the daemon sees it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkshopStatus {
    pub steam: SteamInfo,
    /// Whether the Steam client is running, or why it cannot be reached at all.
    pub client: SteamClient,
    /// The signed-in account's name, once a download has connected to Steam.
    pub account: Option<String>,
    /// Every item Steam has finished downloading.
    pub items: Vec<WorkshopItemStatus>,
    /// Items the Steam client is fetching for the daemon.
    pub downloads: Vec<WorkshopDownload>,
}

/// The Steam client, as far as the daemon can tell without connecting to it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SteamClient {
    #[default]
    NotRunning,
    Running,
    /// Steam's client library cannot be used on this machine.
    Unavailable {
        reason: String,
    },
}

/// One item on its way from the Steam Workshop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkshopDownload {
    pub id: u64,
    pub title: Option<String>,
    pub phase: DownloadPhase,
    /// Bytes fetched so far and in total; the total is zero until Steam has started.
    pub done: u64,
    pub total: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    /// Steam is subscribing the account to the item.
    Subscribing,
    /// Steam has the item in its download queue.
    #[default]
    Queued,
    Downloading,
    /// The files landed and the library entry is being made.
    Importing,
}

impl DownloadPhase {
    pub fn label(self) -> &'static str {
        match self {
            DownloadPhase::Subscribing => "Subscribing",
            DownloadPhase::Queued => "Queued in Steam",
            DownloadPhase::Downloading => "Downloading",
            DownloadPhase::Importing => "Adding to library",
        }
    }
}

impl WorkshopDownload {
    /// Fraction fetched, once Steam has said how much there is.
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f64 / self.total as f64).clamp(0.0, 1.0) as f32)
    }

    pub fn name(&self) -> String {
        self.title
            .clone()
            .unwrap_or_else(|| format!("item {}", self.id))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkshopItemStatus {
    pub id: u64,
    pub dir: PathBuf,
    /// Steam's `timeupdated` for the download.
    pub updated: Option<u64>,
    /// The library entry made from it.
    pub wallpaper: Option<String>,
    pub title: String,
    /// Steam holds a newer download than the library entry was made from.
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

pub fn write_line<W: Write, T: Serialize>(w: &mut W, msg: &T) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(msg)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

/// Read one message; `Ok(None)` at end of stream.
pub fn read_line<R: BufRead, T: DeserializeOwned>(r: &mut R) -> std::io::Result<Option<T>> {
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if line.trim().is_empty() {
            continue;
        }
        return serde_json::from_str(&line)
            .map(Some)
            .map_err(std::io::Error::other);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_response_shape_round_trips() {
        let d = Display {
            id: "a".into(),
            name: "A".into(),
            rect: crate::geom::Rect::new(0, 0, 1, 1),
            workarea: crate::geom::Rect::new(0, 0, 1, 1),
            scale: 1.0,
            primary: true,
        };
        let samples = vec![
            Response::Ok,
            Response::Error {
                kind: "invalid".into(),
                message: "m".into(),
            },
            Response::Displays(vec![d.clone()]),
            Response::Library(vec![]),
            Response::Settings(Settings::default()),
            Response::Layout(Layout::default()),
            Response::Controls {
                path: PathBuf::from("/p"),
                controls: vec![],
            },
            Response::Devices(vec![AudioDevice {
                id: "x".into(),
                name: "X".into(),
            }]),
            Response::Wallpapers(vec![]),
            Response::Workshop(WorkshopStatus {
                steam: SteamInfo::default(),
                items: vec![WorkshopItemStatus {
                    id: 1,
                    dir: PathBuf::from("/w/1"),
                    updated: Some(2),
                    wallpaper: Some("x".into()),
                    title: "T".into(),
                    stale: true,
                }],
                client: SteamClient::Unavailable { reason: "r".into() },
                account: Some("a".into()),
                downloads: vec![WorkshopDownload {
                    id: 3,
                    title: None,
                    phase: DownloadPhase::Downloading,
                    done: 1,
                    total: 2,
                }],
            }),
            Response::Text("t".into()),
            Response::Status(Status {
                version: "v".into(),
                platform: "p".into(),
                session: "s".into(),
                window_monitor: "m".into(),
                capabilities: Capabilities::default(),
                displays: vec![d],
                layout: Layout::default(),
                active: vec![],
                paused: false,
                locked: false,
                on_battery: false,
            }),
        ];
        for r in samples {
            let text = serde_json::to_string(&r).expect("serialize");
            let back: Response = serde_json::from_str(&text).expect("deserialize");
            assert_eq!(serde_json::to_string(&back).unwrap(), text);
        }
        for r in [
            Request::Status,
            Request::Set {
                target: "x".into(),
                display: None,
            },
            Request::SetProperty {
                wallpaper: String::new(),
                display: None,
                name: "n".into(),
                value: serde_json::Value::Null,
            },
            Request::AlignImage {
                pose: Pose::default(),
            },
            Request::AlignDisplay {
                display: "a".into(),
                pose: Pose {
                    x: 1.0,
                    y: -2.0,
                    scale: 1.5,
                    rotation: 90.0,
                },
            },
            Request::WorkshopGet {
                id: 5,
                title: Some("t".into()),
                author: None,
                display: None,
            },
            Request::WorkshopCancel { id: 5 },
            Request::WorkshopSync,
            Request::WorkshopStatus,
        ] {
            let text = serde_json::to_string(&r).expect("serialize request");
            let _: Request = serde_json::from_str(&text).expect("deserialize request");
        }
        for e in [
            Event::Library,
            Event::Workshop,
            Event::Quit,
            Event::Error {
                message: "e".into(),
            },
        ] {
            let text = serde_json::to_string(&e).expect("serialize event");
            let _: Event = serde_json::from_str(&text).expect("deserialize event");
        }
    }
}
