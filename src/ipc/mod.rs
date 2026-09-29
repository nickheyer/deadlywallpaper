pub mod client;
pub mod server;

use crate::error::Error;
use crate::model::{Arrangement, Control, Display, Kind, Layout, Pose, Settings, Summary};
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
    Error { message: String },
    Info { message: String },
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
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
        ] {
            let text = serde_json::to_string(&r).expect("serialize request");
            let _: Request = serde_json::from_str(&text).expect("deserialize request");
        }
        for e in [
            Event::Library,
            Event::Error {
                message: "e".into(),
            },
        ] {
            let text = serde_json::to_string(&e).expect("serialize event");
            let _: Event = serde_json::from_str(&text).expect("deserialize event");
        }
    }
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
