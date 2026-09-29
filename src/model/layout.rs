use crate::content::View;
use crate::error::Result;
use crate::geom::Rect;
use crate::model::Display;
use crate::model::display;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Arrangement {
    /// A different wallpaper on each display.
    #[default]
    Per,
    /// One wallpaper stretched across every display.
    Span,
    /// The same wallpaper repeated on every display.
    Duplicate,
}

impl Arrangement {
    pub fn label(self) -> &'static str {
        match self {
            Arrangement::Per => "Per display",
            Arrangement::Span => "Span",
            Arrangement::Duplicate => "Duplicate",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub display: String,
    pub wallpaper: String,
}

/// Centre offset in pixels, scale, and clockwise rotation in degrees.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pose {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation: f64,
}

impl Default for Pose {
    fn default() -> Pose {
        Pose { x: 0.0, y: 0.0, scale: 1.0, rotation: 0.0 }
    }
}

impl Pose {
    pub const MIN_SCALE: f64 = 0.05;
    pub const MAX_SCALE: f64 = 20.0;

    pub fn is_identity(&self) -> bool {
        *self == Pose::default()
    }

    /// Finite values, scale within range, rotation in `(-180, 180]`.
    pub fn normalized(self) -> Pose {
        let finite = |v: f64, fallback: f64| if v.is_finite() { v } else { fallback };
        let mut rotation = finite(self.rotation, 0.0) % 360.0;
        if rotation <= -180.0 {
            rotation += 360.0;
        } else if rotation > 180.0 {
            rotation -= 360.0;
        }
        Pose { x: finite(self.x, 0.0), y: finite(self.y, 0.0), scale: finite(self.scale, 1.0).clamp(Pose::MIN_SCALE, Pose::MAX_SCALE), rotation }
    }
}

/// A display's pose inside a spanning wallpaper.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    pub display: String,
    pub pose: Pose,
}

/// layout.json; retains assignments for disconnected displays.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub arrangement: Arrangement,
    pub per: Vec<Assignment>,
    /// Wallpaper for span and duplicate arrangements.
    pub shared: Option<String>,
    /// The spanning image, relative to the desktop's bounds.
    pub image: Pose,
    /// Each display, relative to where the desktop reports it.
    pub alignment: Vec<Alignment>,
}

/// One content instance to run: `region` in global coordinates, presented on `display`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Placement {
    pub display: String,
    pub wallpaper: String,
    pub region: Rect,
    /// Key of the per-slot property copy: the display id, `span`, or `duplicate`.
    pub slot: String,
    /// Whether this instance is the one that carries audio.
    pub audio: bool,
    /// The instance shows its display's part of the spanning image, see [`Layout::view_for`].
    pub spanning: bool,
}

impl Layout {
    pub fn load(path: &Path) -> Layout {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                log::warn!("{}: {e}; starting with an empty layout", path.display());
                Layout::default()
            }),
            Err(_) => Layout::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        crate::paths::write(path, text)
    }

    pub fn assign(&mut self, display: &str, wallpaper: &str) {
        match self.arrangement {
            Arrangement::Per => match self.per.iter_mut().find(|a| a.display == display) {
                Some(a) => a.wallpaper = wallpaper.to_string(),
                None => self.per.push(Assignment { display: display.into(), wallpaper: wallpaper.into() }),
            },
            _ => self.shared = Some(wallpaper.to_string()),
        }
    }

    pub fn clear_display(&mut self, display: &str) {
        match self.arrangement {
            Arrangement::Per => self.per.retain(|a| a.display != display),
            _ => self.shared = None,
        }
    }

    pub fn clear(&mut self) {
        self.per.clear();
        self.shared = None;
    }

    pub fn remove_wallpaper(&mut self, wallpaper: &str) {
        self.per.retain(|a| a.wallpaper != wallpaper);
        if self.shared.as_deref() == Some(wallpaper) {
            self.shared = None;
        }
    }

    pub fn wallpaper_for(&self, display: &str) -> Option<&str> {
        match self.arrangement {
            Arrangement::Per => self.per.iter().find(|a| a.display == display).map(|a| a.wallpaper.as_str()),
            _ => self.shared.as_deref(),
        }
    }

    pub fn pose(&self, display: &str) -> Pose {
        self.alignment.iter().find(|a| a.display == display).map(|a| a.pose).unwrap_or_default()
    }

    pub fn set_image_pose(&mut self, pose: Pose) {
        self.image = pose.normalized();
    }

    /// Place `display`; the identity pose puts it back where the desktop reports it.
    pub fn set_display_pose(&mut self, display: &str, pose: Pose) {
        let pose = pose.normalized();
        self.alignment.retain(|a| a.display != display);
        if !pose.is_identity() {
            self.alignment.push(Alignment { display: display.into(), pose });
        }
    }

    pub fn reset_alignment(&mut self) {
        self.image = Pose::default();
        self.alignment.clear();
    }

    /// Whether the image or any connected display has been moved, scaled or rotated.
    pub fn is_aligned(&self, displays: &[Display]) -> bool {
        !self.image.is_identity() || displays.iter().any(|d| !self.pose(&d.id).is_identity())
    }

    /// The spanning image's extent before any pose: the desktop's bounds.
    pub fn span_bounds(displays: &[Display]) -> Rect {
        Rect::bounds(displays.iter().map(|d| &d.rect))
    }

    /// What `d` shows of the spanning image whose extent is `bounds`: the image transform
    /// composed with the inverse of the display's own.
    pub fn view_for(&self, d: &Display, bounds: Rect) -> View {
        let image = self.image;
        let own = self.pose(&d.id);
        let (ci_x, ci_y) = (bounds.x as f64 + bounds.w as f64 / 2.0 + image.x, bounds.y as f64 + bounds.h as f64 / 2.0 + image.y);
        let (cd_x, cd_y) = (d.rect.x as f64 + d.rect.w as f64 / 2.0 + own.x, d.rect.y as f64 + d.rect.h as f64 / 2.0 + own.y);
        let (dx, dy) = ((ci_x - cd_x) / own.scale, (ci_y - cd_y) / own.scale);
        let (s, c) = (-own.rotation).to_radians().sin_cos();
        View { width: bounds.w, height: bounds.h, scale: image.scale / own.scale, rotation: image.rotation - own.rotation, x: dx * c - dy * s, y: dx * s + dy * c }
    }

    /// Switch arrangement, carrying the most relevant wallpaper over.
    pub fn set_arrangement(&mut self, arrangement: Arrangement, preferred_display: Option<&str>) {
        if self.arrangement == arrangement {
            return;
        }
        let carried = match self.arrangement {
            Arrangement::Per => preferred_display
                .and_then(|d| self.wallpaper_for(d))
                .or_else(|| self.per.first().map(|a| a.wallpaper.as_str()))
                .map(str::to_owned),
            _ => self.shared.clone(),
        };
        self.arrangement = arrangement;
        match arrangement {
            Arrangement::Per => {
                self.shared = None;
                if let (Some(w), Some(d)) = (carried, preferred_display) {
                    if self.wallpaper_for(d).is_none() {
                        self.assign(d, &w);
                    }
                }
            }
            _ => {
                self.per.clear();
                self.shared = carried;
            }
        }
    }

    /// Use one spanning surface when supported and untransformed; otherwise crop per display.
    pub fn plan(&self, displays: &[Display], spans: bool) -> Vec<Placement> {
        let Some(primary) = display::primary(displays) else { return Vec::new() };
        match self.arrangement {
            Arrangement::Per => self
                .per
                .iter()
                .filter_map(|a| {
                    let d = displays.iter().find(|d| d.id == a.display)?;
                    Some(Placement { display: d.id.clone(), wallpaper: a.wallpaper.clone(), region: d.rect, slot: d.id.clone(), audio: true, spanning: false })
                })
                .collect(),
            Arrangement::Span => {
                let Some(w) = &self.shared else { return Vec::new() };
                if spans && !self.is_aligned(displays) {
                    let region = Layout::span_bounds(displays);
                    vec![Placement { display: primary.id.clone(), wallpaper: w.clone(), region, slot: "span".into(), audio: true, spanning: false }]
                } else {
                    displays
                        .iter()
                        .map(|d| Placement { display: d.id.clone(), wallpaper: w.clone(), region: d.rect, slot: "span".into(), audio: d.id == primary.id, spanning: true })
                        .collect()
                }
            }
            Arrangement::Duplicate => {
                let Some(w) = &self.shared else { return Vec::new() };
                displays
                    .iter()
                    .map(|d| Placement { display: d.id.clone(), wallpaper: w.clone(), region: d.rect, slot: "duplicate".into(), audio: d.id == primary.id, spanning: false })
                    .collect()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn displays() -> Vec<Display> {
        vec![
            Display { id: "a".into(), name: "A".into(), rect: Rect::new(0, 0, 1920, 1080), workarea: Rect::new(0, 0, 1920, 1040), scale: 1.0, primary: true },
            Display { id: "b".into(), name: "B".into(), rect: Rect::new(1920, 0, 1080, 1920), workarea: Rect::new(1920, 0, 1080, 1920), scale: 1.0, primary: false },
        ]
    }

    #[test]
    fn per_keeps_disconnected_assignments() {
        let mut l = Layout::default();
        l.assign("a", "w1");
        l.assign("gone", "w2");
        let p = l.plan(&displays(), true);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].slot, "a");
        assert!(p[0].audio);
        assert_eq!(l.per.len(), 2);
    }

    #[test]
    fn span_depends_on_platform_capability() {
        let mut l = Layout { arrangement: Arrangement::Span, ..Layout::default() };
        l.assign("a", "w");
        let one = l.plan(&displays(), true);
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].region, Rect::new(0, 0, 3000, 1920));
        assert!(!one[0].spanning);
        let many = l.plan(&displays(), false);
        assert_eq!(many.len(), 2);
        assert!(many.iter().all(|p| p.slot == "span" && p.spanning));
        assert_eq!(many.iter().find(|p| p.display == "b").unwrap().region, Rect::new(1920, 0, 1080, 1920));
        assert_eq!(many.iter().filter(|p| p.audio).count(), 1);
    }

    #[test]
    fn views_show_each_display_its_part_of_the_image() {
        let ds = displays();
        let b = &ds[1];
        let slot = crate::geom::Size { w: b.rect.w, h: b.rect.h };
        let mut l = Layout { arrangement: Arrangement::Span, ..Layout::default() };
        l.assign("a", "w");
        let bounds = Layout::span_bounds(&ds);
        assert_eq!(bounds, Rect::new(0, 0, 3000, 1920));

        // Untouched: b's top-left corner shows image pixel (1920, 0).
        let v = l.view_for(b, bounds);
        assert!(v.is_plain());
        assert_eq!(v.to_image(slot, 0.0, 0.0), (1920.0, 0.0));

        // b sits 200 px lower than the desktop says: its corner shows the image 200 px down.
        l.set_display_pose("b", Pose { y: 200.0, ..Pose::default() });
        assert!(l.is_aligned(&ds));
        assert_eq!(l.plan(&ds, true).len(), 2, "posed displays need one instance each");
        assert_eq!(l.view_for(b, bounds).to_image(slot, 0.0, 0.0), (1920.0, 200.0));

        // b rotated a quarter turn: its centre still shows the same image pixel, its corner
        // shows what lies a quarter turn away.
        l.set_display_pose("b", Pose { rotation: 90.0, ..Pose::default() });
        let v = l.view_for(b, bounds);
        assert_eq!(v.rotation, -90.0);
        let (cx, cy) = v.to_image(slot, 540.0, 960.0);
        assert!((cx - 2460.0).abs() < 1e-6 && (cy - 960.0).abs() < 1e-6);
        let (x, y) = v.to_image(slot, 0.0, 0.0);
        assert!((x - 3420.0).abs() < 1e-6 && (y - 420.0).abs() < 1e-6, "{x},{y}");

        // The image scaled up twice shows half as much per display.
        l.reset_alignment();
        l.set_image_pose(Pose { scale: 2.0, ..Pose::default() });
        let v = l.view_for(b, bounds);
        assert_eq!(v.scale, 2.0);
        let (x, y) = v.to_image(slot, 540.0, 960.0);
        assert!((x - 1980.0).abs() < 1e-6 && (y - 960.0).abs() < 1e-6, "{x},{y}");

        l.reset_alignment();
        assert!(!l.is_aligned(&ds));
        assert_eq!(l.plan(&ds, true).len(), 1);
    }

    #[test]
    fn poses_are_normalized() {
        let p = Pose { x: f64::NAN, y: 5.0, scale: 0.0, rotation: 540.0 }.normalized();
        assert_eq!(p, Pose { x: 0.0, y: 5.0, scale: Pose::MIN_SCALE, rotation: 180.0 });
        assert_eq!(Pose { rotation: -180.0, ..Pose::default() }.normalized().rotation, 180.0);
        assert_eq!(Pose { rotation: -190.0, ..Pose::default() }.normalized().rotation, 170.0);
        let mut l = Layout::default();
        l.set_display_pose("a", Pose { rotation: 360.0, ..Pose::default() });
        assert!(l.alignment.is_empty(), "a full turn is the identity");
    }

    #[test]
    fn duplicate_mutes_secondary() {
        let mut l = Layout { arrangement: Arrangement::Duplicate, ..Layout::default() };
        l.assign("b", "w");
        let p = l.plan(&displays(), true);
        assert_eq!(p.len(), 2);
        assert!(p.iter().find(|p| p.display == "a").unwrap().audio);
        assert!(!p.iter().find(|p| p.display == "b").unwrap().audio);
    }

    #[test]
    fn arrangement_switch_carries_wallpaper() {
        let mut l = Layout::default();
        l.assign("a", "w1");
        l.assign("b", "w2");
        l.set_arrangement(Arrangement::Span, Some("b"));
        assert_eq!(l.shared.as_deref(), Some("w2"));
        assert!(l.per.is_empty());
        l.set_arrangement(Arrangement::Per, Some("a"));
        assert_eq!(l.wallpaper_for("a"), Some("w2"));
    }
}
