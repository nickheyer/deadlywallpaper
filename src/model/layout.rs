use crate::error::{Result, ctx};
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

/// Desired wallpaper state, persisted as `layout.json`. Assignments for displays that are
/// currently disconnected are kept so they restore when the display returns.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub arrangement: Arrangement,
    pub per: Vec<Assignment>,
    /// Wallpaper for span and duplicate arrangements.
    pub shared: Option<String>,
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
        ctx(std::fs::write(path, text), path.display())
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

    /// Expand the layout into concrete placements for the connected displays.
    /// `spans` says whether the platform can present one region across several displays;
    /// when it cannot, span runs one clipped instance per display.
    pub fn plan(&self, displays: &[Display], spans: bool) -> Vec<Placement> {
        let Some(primary) = display::primary(displays) else { return Vec::new() };
        match self.arrangement {
            Arrangement::Per => self
                .per
                .iter()
                .filter_map(|a| {
                    let d = displays.iter().find(|d| d.id == a.display)?;
                    Some(Placement { display: d.id.clone(), wallpaper: a.wallpaper.clone(), region: d.rect, slot: d.id.clone(), audio: true })
                })
                .collect(),
            Arrangement::Span => {
                let Some(w) = &self.shared else { return Vec::new() };
                let region = display::virtual_bounds(displays);
                if spans {
                    vec![Placement { display: primary.id.clone(), wallpaper: w.clone(), region, slot: "span".into(), audio: true }]
                } else {
                    displays
                        .iter()
                        .map(|d| Placement { display: d.id.clone(), wallpaper: w.clone(), region, slot: "span".into(), audio: d.id == primary.id })
                        .collect()
                }
            }
            Arrangement::Duplicate => {
                let Some(w) = &self.shared else { return Vec::new() };
                displays
                    .iter()
                    .map(|d| Placement { display: d.id.clone(), wallpaper: w.clone(), region: d.rect, slot: "duplicate".into(), audio: d.id == primary.id })
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
        let many = l.plan(&displays(), false);
        assert_eq!(many.len(), 2);
        assert!(many.iter().all(|p| p.region == Rect::new(0, 0, 3000, 1920) && p.slot == "span"));
        assert_eq!(many.iter().filter(|p| p.audio).count(), 1);
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
