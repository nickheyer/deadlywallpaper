//! `project.json`: what a Wallpaper Engine wallpaper is, and the user properties it exposes.

use crate::error::{Error, Result, ctx};
use crate::model::props::Properties;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::path::Path;

pub const FILE_NAME: &str = "project.json";

/// Steam Workshop page of an item.
pub fn workshop_url(id: u64) -> String {
    format!("https://steamcommunity.com/sharedfiles/filedetails/?id={id}")
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectType {
    Scene,
    Video,
    Web,
    Application,
}

impl ProjectType {
    pub const ALL: [ProjectType; 4] = [
        ProjectType::Scene,
        ProjectType::Video,
        ProjectType::Web,
        ProjectType::Application,
    ];

    /// The Workshop tag naming this type.
    pub fn tag(self) -> &'static str {
        match self {
            ProjectType::Scene => "Scene",
            ProjectType::Video => "Video",
            ProjectType::Web => "Web",
            ProjectType::Application => "Application",
        }
    }

    pub fn parse(s: &str) -> Option<ProjectType> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "scene" => ProjectType::Scene,
            "video" => ProjectType::Video,
            "web" => ProjectType::Web,
            "application" => ProjectType::Application,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            ProjectType::Scene => "scene",
            ProjectType::Video => "video",
            ProjectType::Web => "web",
            ProjectType::Application => "application",
        }
    }
}

/// Where a `file` or `directory` property looks for content.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FileType {
    Image,
    Video,
}

impl FileType {
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            FileType::Image => &["jpeg", "jpg", "png", "pnga", "bmp", "gif", "svg", "webp"],
            FileType::Video => &["webm", "ogg", "ogv", "mp4"],
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FileType::Image => "image",
            FileType::Video => "video",
        }
    }

    pub fn parse(s: Option<&str>) -> FileType {
        match s.map(|s| s.trim().to_ascii_lowercase()).as_deref() {
            Some("video") => FileType::Video,
            _ => FileType::Image,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropertyKind {
    Slider {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
    },
    /// Components 0..=1.
    Color([f64; 3]),
    Bool(bool),
    /// The selected option's value, and every option as `(label, value)`.
    Combo {
        value: Value,
        options: Vec<(String, Value)>,
    },
    Text(String),
    File {
        value: String,
        file_type: FileType,
    },
    Directory {
        value: String,
        file_type: FileType,
        /// `true` for `fetchall`: the page receives every file up front.
        fetch_all: bool,
    },
    /// A scene texture chooser: a path inside the project.
    SceneTexture(String),
    /// A heading in the property list.
    Group,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub key: String,
    /// Localized label.
    pub text: String,
    pub kind: PropertyKind,
    pub order: i64,
    /// Display condition over other properties, as `name.value == true`.
    pub condition: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub title: String,
    pub kind: ProjectType,
    /// Content file relative to the project folder: `scene.pkg`, `scene.json`, a video, an
    /// HTML page or an executable.
    pub file: String,
    pub preview: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub workshop_id: Option<u64>,
    pub content_rating: Option<String>,
    /// `general.supportsaudioprocessing`: the wallpaper wants the audio spectrum.
    pub audio: bool,
    /// User properties in display order.
    pub properties: Vec<Property>,
    /// The whole document, for the scene renderer and for export.
    pub raw: Value,
}

impl Project {
    pub fn load(path: &Path) -> Result<Project> {
        let text = ctx(std::fs::read_to_string(path), path.display())?;
        Project::parse(&text).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
    }

    pub fn parse(text: &str) -> Result<Project> {
        let raw: Value = serde_json::from_str(text)?;
        let o = raw
            .as_object()
            .ok_or_else(|| Error::Invalid("project.json must be an object".into()))?;
        let kind_name = o.get("type").and_then(Value::as_str).unwrap_or("");
        let kind = ProjectType::parse(kind_name).ok_or_else(|| {
            Error::Unsupported(if kind_name.is_empty() {
                "project.json has no wallpaper type".into()
            } else {
                format!("wallpaper type '{kind_name}' is not a Wallpaper Engine type")
            })
        })?;
        let file = o
            .get("file")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .ok_or_else(|| Error::Invalid("project.json names no content file".into()))?
            .replace('\\', "/");
        let title = o
            .get("title")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("Wallpaper")
            .to_string();
        let general = o.get("general").and_then(Value::as_object);
        let localization = general
            .and_then(|g| g.get("localization"))
            .and_then(Value::as_object);
        let localize = |key: &str| localize(key, localization);
        let mut properties: Vec<Property> = general
            .and_then(|g| g.get("properties"))
            .and_then(Value::as_object)
            .map(|props| {
                props
                    .iter()
                    .filter_map(|(k, v)| parse_property(k, v, &localize))
                    .collect()
            })
            .unwrap_or_default();
        properties.sort_by_key(|p| p.order);
        let workshop_id = o
            .get("workshopid")
            .and_then(|v| match v {
                Value::String(s) => s.trim().parse().ok(),
                Value::Number(n) => n.as_u64(),
                _ => None,
            })
            .filter(|id| *id > 0);
        Ok(Project {
            title,
            kind,
            file,
            preview: o
                .get("preview")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string),
            description: o
                .get("description")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_string),
            tags: o
                .get("tags")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            workshop_id,
            content_rating: o
                .get("contentrating")
                .and_then(Value::as_str)
                .map(str::to_string),
            audio: general
                .and_then(|g| g.get("supportsaudioprocessing"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
            properties,
            raw,
        })
    }

    /// Controls in LivelyProperties.json form, carrying the Wallpaper Engine type of each so
    /// values can be sent back in Wallpaper Engine's encoding.
    pub fn controls(&self) -> Properties {
        let mut out = Map::new();
        for p in &self.properties {
            let mut c = match &p.kind {
                PropertyKind::Slider {
                    value,
                    min,
                    max,
                    step,
                } => json!({
                    "type": "slider", "value": value, "min": min, "max": max, "step": step,
                    "we": {"type": "slider"}
                }),
                PropertyKind::Color(rgb) => json!({
                    "type": "color", "value": hex_of(*rgb), "we": {"type": "color"}
                }),
                PropertyKind::Bool(b) => json!({
                    "type": "checkbox", "value": b, "we": {"type": "bool"}
                }),
                PropertyKind::Combo { value, options } => {
                    let index = options
                        .iter()
                        .position(|(_, v)| loosely_equal(v, value))
                        .unwrap_or(0);
                    json!({
                        "type": "dropdown", "value": index,
                        "items": options.iter().map(|(l, _)| l.clone()).collect::<Vec<_>>(),
                        "we": {"type": "combo", "values": options.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>()}
                    })
                }
                PropertyKind::Text(t) => json!({
                    "type": "textbox", "value": t, "we": {"type": "textinput"}
                }),
                PropertyKind::File { value, file_type } => json!({
                    "type": "file", "value": value, "filter": file_type.extensions(),
                    "we": {"type": "file", "filetype": file_type.name()}
                }),
                PropertyKind::Directory {
                    value,
                    file_type,
                    fetch_all,
                } => json!({
                    "type": "folder", "value": value, "filter": file_type.extensions(),
                    "we": {"type": "directory", "filetype": file_type.name(), "fetchall": fetch_all}
                }),
                PropertyKind::SceneTexture(t) => json!({
                    "type": "textbox", "value": t, "we": {"type": "scenetexture"}
                }),
                PropertyKind::Group => json!({
                    "type": "label", "value": p.text, "we": {"type": "group"}
                }),
            };
            if let Some(o) = c.as_object_mut() {
                o.insert("text".into(), Value::String(p.text.clone()));
                if let Some(cond) = &p.condition {
                    o.insert("condition".into(), Value::String(cond.clone()));
                }
            }
            out.insert(p.key.clone(), c);
        }
        Properties::from_value(Value::Object(out)).expect("built as an object")
    }

    /// Whether `file` is a scene the renderer reads from an unpacked `scene.json`.
    pub fn is_packed_scene(&self) -> bool {
        self.kind == ProjectType::Scene
            && Path::new(&self.file)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pkg"))
    }
}

fn parse_property(
    key: &str,
    v: &Value,
    localize: &dyn Fn(&str) -> String,
) -> Option<Property> {
    let o = v.as_object()?;
    let s = |k: &str| o.get(k).and_then(Value::as_str);
    let f = |k: &str| o.get(k).and_then(number);
    let ty = s("type")?.trim().to_ascii_lowercase();
    let kind = match ty.as_str() {
        "slider" => {
            let min = f("min").unwrap_or(0.0);
            let max = f("max").unwrap_or(100.0);
            let fraction = o.get("fraction").and_then(Value::as_bool).unwrap_or(false);
            let step = f("step").filter(|s| *s > 0.0).unwrap_or_else(|| {
                if fraction {
                    let precision = f("precision").unwrap_or(2.0).clamp(0.0, 6.0);
                    10f64.powi(-(precision as i32))
                } else {
                    1.0
                }
            });
            PropertyKind::Slider {
                value: f("value").unwrap_or(min).clamp(min.min(max), max.max(min)),
                min,
                max,
                step,
            }
        }
        "color" => PropertyKind::Color(parse_color(s("value").unwrap_or("1 1 1"))),
        "bool" => PropertyKind::Bool(match o.get("value") {
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
            Some(Value::String(s)) => matches!(s.trim(), "true" | "1"),
            _ => false,
        }),
        "combo" => {
            let options: Vec<(String, Value)> = o
                .get("options")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|opt| {
                            let oo = opt.as_object()?;
                            let label = oo.get("label").and_then(Value::as_str).unwrap_or("");
                            Some((
                                localize(label),
                                oo.get("value").cloned().unwrap_or(Value::Null),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let value = o
                .get("value")
                .cloned()
                .or_else(|| options.first().map(|(_, v)| v.clone()))
                .unwrap_or(Value::Null);
            PropertyKind::Combo { value, options }
        }
        "textinput" => PropertyKind::Text(s("value").unwrap_or("").to_string()),
        "file" => PropertyKind::File {
            value: s("value").unwrap_or("").to_string(),
            file_type: FileType::parse(s("filetype").or_else(|| s("fileType"))),
        },
        "directory" => PropertyKind::Directory {
            value: s("value").unwrap_or("").to_string(),
            file_type: FileType::parse(s("filetype").or_else(|| s("fileType"))),
            fetch_all: s("mode").is_some_and(|m| m.eq_ignore_ascii_case("fetchall")),
        },
        "scenetexture" => PropertyKind::SceneTexture(s("value").unwrap_or("").to_string()),
        "group" => PropertyKind::Group,
        _ => return None,
    };
    Some(Property {
        key: key.to_string(),
        text: localize(s("text").unwrap_or(key)),
        kind,
        order: o.get("order").and_then(number).unwrap_or(0.0) as i64,
        condition: s("condition")
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_string),
    })
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
    .filter(|n| n.is_finite())
}

/// `==` as the page would see it: numbers and their string forms match.
fn loosely_equal(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    match (number(a), number(b)) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-9,
        _ => a.as_str().zip(b.as_str()).is_some_and(|(x, y)| x == y),
    }
}

/// `"r g b"` floats to components 0..=1.
pub fn parse_color(s: &str) -> [f64; 3] {
    let mut out = [1.0; 3];
    for (i, part) in s.split_whitespace().take(3).enumerate() {
        out[i] = part.parse::<f64>().unwrap_or(1.0).clamp(0.0, 1.0);
    }
    out
}

/// Components 0..=1 to `#rrggbb`.
pub fn hex_of(rgb: [f64; 3]) -> String {
    let c = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(rgb[0]), c(rgb[1]), c(rgb[2]))
}

/// `#rrggbb` (or `#rgb`) to the `"r g b"` string Wallpaper Engine uses.
pub fn hex_to_we(hex: &str) -> String {
    let h = hex.trim().trim_start_matches('#');
    let digits: Vec<u8> = h.bytes().filter_map(|b| (b as char).to_digit(16).map(|d| d as u8)).collect();
    let (r, g, b) = match digits.len() {
        3 => (digits[0] * 17, digits[1] * 17, digits[2] * 17),
        n if n >= 6 => (
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        ),
        _ => (255, 255, 255),
    };
    let f = |c: u8| {
        let v = c as f64 / 255.0;
        let s = format!("{v:.6}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        if s.is_empty() { "0".to_string() } else { s.to_string() }
    };
    format!("{} {} {}", f(r), f(g), f(b))
}

/// A control's value in the encoding Wallpaper Engine hands to wallpapers: colors as
/// `"r g b"`, combos as the chosen option's value, everything else as stored. `None` for
/// controls that carry no value.
pub fn we_value(control: &crate::model::Control, value: Option<&Value>) -> Option<Value> {
    let value = value?;
    let Some(meta) = &control.we else {
        return Some(value.clone());
    };
    Some(match meta.kind.as_str() {
        "color" => Value::String(hex_to_we(value.as_str().unwrap_or("#ffffff"))),
        "combo" => {
            let index = value.as_i64().or_else(|| number(value).map(|n| n as i64))?;
            meta.values
                .get(index.max(0) as usize)
                .cloned()
                .unwrap_or(value.clone())
        }
        "bool" => Value::Bool(match value {
            Value::Bool(b) => *b,
            other => number(other).is_some_and(|n| n != 0.0),
        }),
        "slider" => number(value).map(|n| json!(n)).unwrap_or(value.clone()),
        _ => value.clone(),
    })
}

/// Built-in label keys Wallpaper Engine translates itself.
const BUILTIN_TEXT: &[(&str, &str)] = &[
    ("ui_browse_properties_scheme_color", "Scheme color"),
    ("ui_browse_properties_scheme_colour", "Scheme color"),
];

/// The English text of a label: the project's own translation, a built-in, or the key
/// itself made readable.
pub fn localize(key: &str, localization: Option<&Map<String, Value>>) -> String {
    let key = key.trim();
    if !key.starts_with("ui_") {
        return key.to_string();
    }
    if let Some(l) = localization {
        for lang in [crate::we::language().as_str(), "en-us", "en-gb"] {
            if let Some(text) = l
                .get(lang)
                .and_then(Value::as_object)
                .and_then(|t| t.get(key))
                .and_then(Value::as_str)
            {
                return text.to_string();
            }
        }
        if let Some(text) = l
            .values()
            .filter_map(Value::as_object)
            .find_map(|t| t.get(key).and_then(Value::as_str))
        {
            return text.to_string();
        }
    }
    if let Some((_, text)) = BUILTIN_TEXT.iter().find(|(k, _)| *k == key) {
        return (*text).to_string();
    }
    let words = key.trim_start_matches("ui_").replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::props::ControlKind;

    const SAMPLE: &str = r#"{
      "contentrating": "Everyone",
      "description": "A test",
      "file": "scene.pkg",
      "general": {
        "properties": {
          "schemecolor": {"order": 0, "text": "ui_browse_properties_scheme_color", "type": "color", "value": "0.5 0.25 1"},
          "speed": {"fraction": true, "max": 5, "min": 0.5, "order": 103, "precision": 1, "text": "Speed", "type": "slider", "value": 2},
          "count": {"max": 10, "min": 1, "order": 102, "text": "ui_count", "type": "slider", "value": 3},
          "showsun": {"order": 101, "text": "Show the sun", "type": "bool", "value": true, "condition": "count.value > 2"},
          "style": {"order": 104, "options": [{"label": "ui_style_a", "value": "a"}, {"label": "ui_style_b", "value": 2}], "text": "Style", "type": "combo", "value": 2},
          "name": {"order": 105, "text": "Name", "type": "textinput", "value": "hi"},
          "bg": {"order": 106, "text": "Background", "type": "file", "value": "", "filetype": "image"},
          "pics": {"order": 107, "text": "Pictures", "type": "directory", "value": "", "mode": "fetchall"},
          "hdr": {"order": 100, "text": "Look", "type": "group"}
        },
        "localization": {"en-us": {"ui_style_a": "Alpha", "ui_style_b": "Beta"}},
        "supportsaudioprocessing": true
      },
      "preview": "preview.gif",
      "tags": ["Abstract", "Relaxing"],
      "title": "Test Scene",
      "type": "scene",
      "visibility": "public",
      "workshopid": "123456789"
    }"#;

    #[test]
    fn parses_project_and_orders_properties() {
        let p = Project::parse(SAMPLE).unwrap();
        assert_eq!(p.title, "Test Scene");
        assert_eq!(p.kind, ProjectType::Scene);
        assert!(p.is_packed_scene());
        assert_eq!(p.preview.as_deref(), Some("preview.gif"));
        assert_eq!(p.workshop_id, Some(123456789));
        assert!(p.audio);
        assert_eq!(p.tags, ["Abstract", "Relaxing"]);
        let keys: Vec<&str> = p.properties.iter().map(|p| p.key.as_str()).collect();
        assert_eq!(
            keys,
            ["schemecolor", "hdr", "showsun", "count", "speed", "style", "name", "bg", "pics"]
        );
        assert_eq!(p.properties[0].text, "Scheme color");
        assert_eq!(p.properties[3].text, "Count");
        assert_eq!(
            p.properties[2].condition.as_deref(),
            Some("count.value > 2")
        );
        match &p.properties[4].kind {
            PropertyKind::Slider { value, min, max, step } => {
                assert_eq!((*value, *min, *max), (2.0, 0.5, 5.0));
                assert!((step - 0.1).abs() < 1e-9);
            }
            other => panic!("{other:?}"),
        }
        match &p.properties[3].kind {
            PropertyKind::Slider { step, .. } => assert_eq!(*step, 1.0),
            other => panic!("{other:?}"),
        }
        match &p.properties[5].kind {
            PropertyKind::Combo { value, options } => {
                assert_eq!(value, &json!(2));
                assert_eq!(options[0], ("Alpha".to_string(), json!("a")));
                assert_eq!(options[1].0, "Beta");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            &p.properties[8].kind,
            PropertyKind::Directory { fetch_all: true, file_type: FileType::Image, .. }
        ));
        assert_eq!(p.properties[1].kind, PropertyKind::Group);
    }

    #[test]
    fn controls_carry_wallpaper_engine_types() {
        let p = Project::parse(SAMPLE).unwrap();
        let controls = p.controls();
        let names: Vec<String> = controls.controls().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names[0], "schemecolor");
        let color = controls.get("schemecolor").unwrap();
        assert_eq!(color.kind, ControlKind::Color { value: "#8040ff".into() });
        assert_eq!(color.we.as_ref().unwrap().kind, "color");
        let style = controls.get("style").unwrap();
        match style.kind {
            ControlKind::Dropdown { value, ref items } => {
                assert_eq!(value, 1);
                assert_eq!(items, &["Alpha", "Beta"]);
            }
            _ => panic!(),
        }
        assert_eq!(style.we.as_ref().unwrap().values, vec![json!("a"), json!(2)]);
        let sun = controls.get("showsun").unwrap();
        assert_eq!(sun.condition.as_deref(), Some("count.value > 2"));
        assert!(matches!(controls.get("pics").unwrap().kind, ControlKind::Folder { .. }));
        assert!(matches!(controls.get("bg").unwrap().kind, ControlKind::File { .. }));
        assert!(matches!(controls.get("hdr").unwrap().kind, ControlKind::Label { .. }));
    }

    #[test]
    fn values_travel_in_wallpaper_engine_encoding() {
        let p = Project::parse(SAMPLE).unwrap();
        let controls = p.controls();
        let color = controls.get("schemecolor").unwrap();
        assert_eq!(
            we_value(&color, Some(&json!("#ff8000"))),
            Some(json!("1 0.501961 0"))
        );
        let style = controls.get("style").unwrap();
        assert_eq!(we_value(&style, Some(&json!(0))), Some(json!("a")));
        assert_eq!(we_value(&style, Some(&json!(1))), Some(json!(2)));
        let sun = controls.get("showsun").unwrap();
        assert_eq!(we_value(&sun, Some(&json!(true))), Some(json!(true)));
        let plain = crate::model::Control::parse(&json!({"type": "textbox", "value": "x"})).unwrap();
        assert_eq!(we_value(&plain, Some(&json!("y"))), Some(json!("y")));
        assert_eq!(we_value(&plain, None), None);
    }

    #[test]
    fn colors_convert_both_ways() {
        assert_eq!(hex_of(parse_color("1 0.5 0")), "#ff8000");
        assert_eq!(hex_to_we("#ff8000"), "1 0.501961 0");
        assert_eq!(hex_to_we("#000"), "0 0 0");
        assert_eq!(hex_to_we("garbage"), "1 1 1");
        assert_eq!(parse_color("2 -1"), [1.0, 0.0, 1.0]);
    }

    #[test]
    fn rejects_non_wallpaper_engine_documents() {
        assert!(matches!(
            Project::parse(r#"{"title":"x","file":"a.html"}"#),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            Project::parse(r#"{"type":"plugin","file":"a"}"#),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            Project::parse(r#"{"type":"video"}"#),
            Err(Error::Invalid(_))
        ));
        assert!(Project::parse("[]").is_err());
    }

    #[test]
    fn localizes_labels() {
        assert_eq!(localize("Plain", None), "Plain");
        assert_eq!(localize("ui_some_thing", None), "Some thing");
        assert_eq!(localize("ui_browse_properties_scheme_color", None), "Scheme color");
        let l: Map<String, Value> = serde_json::from_str(r#"{"de-de": {"ui_x": "Ex"}}"#).unwrap();
        assert_eq!(localize("ui_x", Some(&l)), "Ex");
    }
}
