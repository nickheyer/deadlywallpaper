use crate::error::Result;
use crate::model::Control;
use serde_json::Value;
use std::path::PathBuf;

pub type ContentId = u64;

/// Notifications from a running content instance to the engine.
#[derive(Debug)]
pub enum ContentEvent {
    /// The content is displaying and ready to receive property values.
    Loaded,
    /// The content stopped on its own (crash, exit, navigation failure).
    Exited { reason: String },
    Screenshot { path: PathBuf, result: Result<()> },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(target_os = "linux", allow(dead_code, reason = "Linux delivers pointer input to wallpaper surfaces natively"))]
pub enum PointerKind {
    Move,
    Down,
    Up,
}

/// Pointer event in content-local pixel coordinates.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PointerEvent {
    pub x: i32,
    pub y: i32,
    pub kind: PointerKind,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Seek {
    /// Percentage of the timeline.
    Absolute(f64),
    /// Percentage offset from the current position.
    Relative(f64),
}

impl Seek {
    /// Parse `50`, `+10`, `-10` as used by the CLI.
    pub fn parse(s: &str) -> Option<Seek> {
        let t = s.trim();
        if let Some(rest) = t.strip_prefix('+') {
            return rest.parse().ok().map(|v: f64| Seek::Relative(v.clamp(-100.0, 100.0)));
        }
        if let Some(rest) = t.strip_prefix('-') {
            return rest.parse().ok().map(|v: f64| Seek::Relative(-v.clamp(-100.0, 100.0)));
        }
        t.parse().ok().map(|v: f64| Seek::Absolute(v.clamp(0.0, 100.0)))
    }
}

/// A live wallpaper instance. Implementations own their native surface; dropping one
/// removes it from the desktop.
pub trait Content {
    fn set_paused(&mut self, paused: bool);
    /// 0..=100
    fn set_volume(&mut self, volume: u8);
    /// Engine-level mute, independent of any user "mute" control.
    fn set_muted(&mut self, muted: bool);
    fn seek(&mut self, seek: Seek);
    /// Push a property to the wallpaper. `value` is `None` for button presses.
    fn apply(&mut self, name: &str, control: &Control, value: Option<&Value>);
    /// Capture the current frame; completion arrives as [`ContentEvent::Screenshot`].
    fn screenshot(&mut self, path: PathBuf);
    fn pointer(&mut self, ev: PointerEvent);
    fn set_input_enabled(&mut self, enabled: bool);
    /// Audio spectrum for visualizer wallpapers, 128 bins.
    fn audio_data(&mut self, bins: &[f32]);
}
