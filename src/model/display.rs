use crate::geom::Rect;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Display {
    /// Stable identifier across reboots and reconnects.
    pub id: String,
    pub name: String,
    pub rect: Rect,
    /// Area outside panels and docks.
    pub workarea: Rect,
    pub scale: f64,
    pub primary: bool,
}

/// The whole desktop as one rectangle; the Windows shell parents one surface under it.
#[cfg(windows)]
pub fn virtual_bounds(displays: &[Display]) -> Rect {
    Rect::bounds(displays.iter().map(|d| &d.rect))
}

pub fn primary(displays: &[Display]) -> Option<&Display> {
    displays
        .iter()
        .find(|d| d.primary)
        .or_else(|| displays.iter().find(|d| d.rect.x == 0 && d.rect.y == 0))
        .or_else(|| displays.first())
}

/// Resolve a user reference: a display id, or a 1-based index in layout order.
pub fn find<'a>(displays: &'a [Display], reference: &str) -> Option<&'a Display> {
    let r = reference.trim();
    displays.iter().find(|d| d.id == r).or_else(|| {
        r.parse::<usize>()
            .ok()
            .filter(|i| *i >= 1)
            .and_then(|i| displays.get(i - 1))
    })
}

pub fn index_of(displays: &[Display], id: &str) -> Option<usize> {
    displays.iter().position(|d| d.id == id).map(|i| i + 1)
}

pub fn at_point(displays: &[Display], x: i32, y: i32) -> Option<&Display> {
    displays.iter().find(|d| d.rect.contains(x, y))
}

/// Sort displays left-to-right, top-to-bottom so indices are predictable.
pub fn sort(displays: &mut [Display]) {
    displays.sort_by_key(|d| (d.rect.x, d.rect.y, d.id.clone()));
}

/// Build stable ids for displays whose backend reports only make and model:
/// `make-model` plus an ordinal among identical models in layout order.
#[cfg(target_os = "linux")]
pub fn composite_id(make: &str, model: &str, ordinal: usize) -> String {
    let mut base = format!("{}-{}", crate::paths::slug(make), crate::paths::slug(model));
    if base == "wallpaper-wallpaper" {
        base = "display".into();
    }
    if ordinal == 0 {
        base
    } else {
        format!("{base}-{}", ordinal + 1)
    }
}
