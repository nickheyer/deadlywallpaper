use crate::error::Result;
use crate::geom::Size;
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
pub enum PointerKind {
    Move,
    /// Reported by the Windows and macOS pointer hooks; on Linux clicks reach wallpaper
    /// surfaces from the display server directly, and KWin reports motion only.
    #[cfg_attr(target_os = "linux", allow(dead_code, reason = "constructed by the Windows and macOS pointer hooks"))]
    Down,
    #[cfg_attr(target_os = "linux", allow(dead_code, reason = "constructed by the Windows and macOS pointer hooks"))]
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

/// How an instance's image maps onto its slot. The image is `width`×`height` logical pixels;
/// it is drawn scaled by `scale`, rotated `rotation` degrees clockwise, with its centre `x`, `y`
/// pixels from the slot centre. [`View::whole`] shows the whole image edge to edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub rotation: f64,
    pub x: f64,
    pub y: f64,
}

impl View {
    pub fn whole(size: Size) -> View {
        View { width: size.w, height: size.h, scale: 1.0, rotation: 0.0, x: 0.0, y: 0.0 }
    }

    /// The image covers the slot exactly, one image pixel per slot pixel.
    pub fn is_whole(&self, slot: Size) -> bool {
        self.width == slot.w && self.height == slot.h && self.is_plain() && self.x == 0.0 && self.y == 0.0
    }

    /// Neither scaled nor rotated.
    pub fn is_plain(&self) -> bool {
        self.scale == 1.0 && self.rotation == 0.0
    }

    /// Top-left corner of the scaled, unrotated image in slot pixels.
    pub fn origin(&self, slot: Size) -> (f64, f64) {
        (slot.w as f64 / 2.0 + self.x - self.scale * self.width as f64 / 2.0, slot.h as f64 / 2.0 + self.y - self.scale * self.height as f64 / 2.0)
    }

    /// Slot pixel to image pixel.
    pub fn to_image(&self, slot: Size, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = (x - slot.w as f64 / 2.0 - self.x, y - slot.h as f64 / 2.0 - self.y);
        let (s, c) = (-self.rotation).to_radians().sin_cos();
        let (rx, ry) = (dx * c - dy * s, dx * s + dy * c);
        (rx / self.scale + self.width as f64 / 2.0, ry / self.scale + self.height as f64 / 2.0)
    }

    /// Image pixel to slot pixel.
    #[cfg(test)]
    pub fn to_slot(&self, slot: Size, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = ((x - self.width as f64 / 2.0) * self.scale, (y - self.height as f64 / 2.0) * self.scale);
        let (s, c) = self.rotation.to_radians().sin_cos();
        (dx * c - dy * s + slot.w as f64 / 2.0 + self.x, dx * s + dy * c + slot.h as f64 / 2.0 + self.y)
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
    /// Show this part of the image; called right after creation and whenever it changes.
    fn set_view(&mut self, view: &View) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_maps_both_ways() {
        let slot = Size { w: 1080, h: 1920 };
        let v = View { width: 3000, height: 1920, scale: 1.25, rotation: 37.0, x: -300.0, y: 80.0 };
        for (x, y) in [(0.0, 0.0), (540.0, 960.0), (1080.0, 1920.0), (-200.0, 55.5)] {
            let (ix, iy) = v.to_image(slot, x, y);
            let (bx, by) = v.to_slot(slot, ix, iy);
            assert!((bx - x).abs() < 1e-6 && (by - y).abs() < 1e-6, "{x},{y} came back as {bx},{by}");
        }
        let whole = View::whole(slot);
        assert!(whole.is_whole(slot));
        assert_eq!(whole.to_image(slot, 10.0, 20.0), (10.0, 20.0));
        assert_eq!(whole.origin(slot), (0.0, 0.0));
        assert!(!v.is_whole(slot) && !v.is_plain());
        let shifted = View { x: -960.0, ..View::whole(Size { w: 3000, h: 1920 }) };
        assert_eq!(shifted.to_image(slot, 0.0, 0.0), (1920.0, 0.0));
        assert_eq!(shifted.origin(slot), (-1920.0, 0.0));
    }
}
