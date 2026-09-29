use crate::error::{Error, Result, ctx};
use crate::model::Kind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::Path;

/// One control of a `LivelyProperties.json` file, typed for the UI and content engines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Control {
    pub text: String,
    pub help: Option<String>,
    #[serde(flatten)]
    pub kind: ControlKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ControlKind {
    Slider { value: f64, min: f64, max: f64, step: f64 },
    Textbox { value: String },
    Dropdown { value: i64, items: Vec<String> },
    ScalerDropdown { value: i64, items: Vec<String> },
    FolderDropdown { value: Option<String>, folder: String, filter: String },
    Button { value: String },
    Label { value: String },
    Color { value: String },
    Checkbox { value: bool },
}

impl Control {
    pub fn parse(v: &Value) -> Option<Control> {
        let o = v.as_object()?;
        let s = |k: &str| o.get(k).and_then(Value::as_str).map(str::to_owned);
        let f = |k: &str| o.get(k).and_then(number);
        let strings = |k: &str| -> Vec<String> {
            o.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|x| x.as_str().map(str::to_owned).unwrap_or_else(|| x.to_string())).collect())
                .unwrap_or_default()
        };
        let ty = s("type")?.to_ascii_lowercase();
        let kind = match ty.as_str() {
            "slider" => ControlKind::Slider {
                value: f("value").unwrap_or(0.0),
                min: f("min").unwrap_or(0.0),
                max: f("max").unwrap_or(100.0),
                step: f("step").filter(|s| *s > 0.0).unwrap_or(1.0),
            },
            "textbox" => ControlKind::Textbox { value: s("value").unwrap_or_default() },
            "dropdown" => ControlKind::Dropdown { value: f("value").unwrap_or(0.0) as i64, items: strings("items") },
            "scalerdropdown" => ControlKind::ScalerDropdown { value: f("value").unwrap_or(0.0) as i64, items: strings("items") },
            "folderdropdown" => ControlKind::FolderDropdown {
                value: s("value").filter(|v| !v.is_empty()),
                folder: s("folder").unwrap_or_default(),
                filter: s("filter").unwrap_or_else(|| "*".into()),
            },
            "button" => ControlKind::Button { value: s("value").unwrap_or_default() },
            "label" => ControlKind::Label { value: s("value").unwrap_or_default() },
            "color" => ControlKind::Color { value: s("value").unwrap_or_else(|| "#ffffff".into()) },
            "checkbox" => ControlKind::Checkbox { value: o.get("value").and_then(Value::as_bool).unwrap_or(false) },
            _ => return None,
        };
        Some(Control { text: s("text").unwrap_or_default(), help: s("help"), kind })
    }

    /// The value pushed to the wallpaper; `None` for controls without a value (button, label).
    pub fn value(&self) -> Option<Value> {
        Some(match &self.kind {
            ControlKind::Slider { value, .. } => json_f64(*value),
            ControlKind::Textbox { value } | ControlKind::Color { value } => Value::String(value.clone()),
            ControlKind::Dropdown { value, .. } | ControlKind::ScalerDropdown { value, .. } => Value::from(*value),
            ControlKind::FolderDropdown { value, folder, .. } => match value {
                Some(v) => Value::String(join_folder(folder, v)),
                None => Value::Null,
            },
            ControlKind::Checkbox { value } => Value::Bool(*value),
            ControlKind::Button { .. } | ControlKind::Label { .. } => return None,
        })
    }

    pub fn is_interactive_only(&self) -> bool {
        matches!(self.kind, ControlKind::Button { .. } | ControlKind::Label { .. })
    }
}

/// A `LivelyProperties.json` document. Unknown fields and control order are preserved;
/// edits touch only `value`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties {
    raw: Map<String, Value>,
}

impl Properties {
    pub fn from_value(v: Value) -> Result<Properties> {
        match v {
            Value::Object(raw) => Ok(Properties { raw }),
            _ => Err(Error::Invalid("LivelyProperties.json must be a JSON object".into())),
        }
    }

    pub fn load(path: &Path) -> Result<Properties> {
        let text = ctx(std::fs::read_to_string(path), path.display())?;
        Properties::from_value(serde_json::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(&Value::Object(self.raw.clone()))?;
        ctx(std::fs::write(path, text), path.display())
    }

    pub fn controls(&self) -> Vec<(String, Control)> {
        self.raw
            .iter()
            .filter_map(|(k, v)| Control::parse(v).map(|c| (k.clone(), c)))
            .collect()
    }

    /// Keep only the controls `keep` accepts. Returns whether anything was dropped.
    pub fn retain(&mut self, keep: impl Fn(&str) -> bool) -> bool {
        let before = self.raw.len();
        self.raw.retain(|k, _| keep(k));
        self.raw.len() != before
    }

    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&Value::Object(self.raw.clone()))?)
    }

    pub fn get(&self, name: &str) -> Option<Control> {
        self.raw.get(name).and_then(Control::parse)
    }

    /// Coerce `incoming` to the control's type, clamp it, store it, and return the
    /// value as it should be sent to the wallpaper. Sliders and dropdowns accept
    /// `++n` / `--n` for relative changes.
    pub fn set(&mut self, name: &str, incoming: &Value) -> Result<Value> {
        let control = self.get(name).ok_or_else(|| Error::NotFound(format!("no control named '{name}'")))?;
        let stored = match &control.kind {
            ControlKind::Slider { value, min, max, .. } => {
                let v = relative(incoming, *value).ok_or_else(|| Error::Invalid(format!("'{name}' expects a number")))?;
                json_f64(v.clamp(min.min(*max), max.max(*min)))
            }
            ControlKind::Dropdown { value, items } | ControlKind::ScalerDropdown { value, items } => {
                let v = relative(incoming, *value as f64).ok_or_else(|| Error::Invalid(format!("'{name}' expects an index")))?;
                Value::from((v.round() as i64).clamp(0, items.len().saturating_sub(1) as i64))
            }
            ControlKind::Checkbox { .. } => Value::Bool(match incoming {
                Value::Bool(b) => *b,
                Value::String(s) => matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"),
                Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
                _ => return Err(Error::Invalid(format!("'{name}' expects true or false"))),
            }),
            ControlKind::Textbox { .. } | ControlKind::Color { .. } => Value::String(as_text(incoming)),
            ControlKind::FolderDropdown { .. } => match incoming {
                Value::Null => Value::Null,
                v => Value::String(Path::new(&as_text(v)).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()),
            },
            ControlKind::Button { .. } | ControlKind::Label { .. } => {
                return Err(Error::Invalid(format!("'{name}' has no value to set")));
            }
        };
        if let Some(Value::Object(o)) = self.raw.get_mut(name) {
            o.insert("value".into(), stored);
        }
        Ok(self.get(name).and_then(|c| c.value()).unwrap_or(Value::Null))
    }
}

fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn relative(v: &Value, current: f64) -> Option<f64> {
    if let Value::String(s) = v {
        let s = s.trim();
        if let Some(rest) = s.strip_prefix("++") {
            return rest.trim().parse::<f64>().ok().map(|d| current + d);
        }
        if let Some(rest) = s.strip_prefix("--") {
            return rest.trim().parse::<f64>().ok().map(|d| current - d);
        }
    }
    number(v)
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_f64(v: f64) -> Value {
    serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::from(0))
}

/// Folder dropdown values travel to the wallpaper as `folder/file` with forward slashes.
pub fn join_folder(folder: &str, file: &str) -> String {
    let folder = folder.trim_matches(['/', '\\']);
    if folder.is_empty() { file.to_string() } else { format!("{folder}/{file}") }
}

/// Built-in controls for media wallpapers (libmpv properties), matching Lively's mpv defaults.
pub const MEDIA_DEFAULTS: &str = r##"{
  "saturation": { "type": "slider", "text": "Saturation", "value": 0, "min": -100, "max": 100, "step": 1 },
  "hue": { "type": "slider", "text": "Hue", "value": 0, "min": -100, "max": 100, "step": 1 },
  "brightness": { "type": "slider", "text": "Brightness", "value": 0, "min": -100, "max": 100, "step": 1 },
  "contrast": { "type": "slider", "text": "Contrast", "value": 0, "min": -100, "max": 100, "step": 1 },
  "gamma": { "type": "slider", "text": "Gamma", "value": 0, "min": -100, "max": 100, "step": 1 },
  "speed": { "type": "slider", "text": "Speed", "value": 1, "min": 0.25, "max": 5, "step": 0.01 },
  "scaler": { "type": "scalerDropdown", "text": "Choose a fit", "help": "Wallpaper scaling", "value": 1, "items": ["None", "Fill", "Uniform", "Uniform Fill"] },
  "mute": { "type": "checkbox", "text": "Mute", "value": false }
}"##;

/// The built-in controls that apply to `kind`: pictures have neither speed nor sound, GIFs
/// have no sound.
pub fn media_defaults(kind: Kind) -> Properties {
    let all: Value = serde_json::from_str(MEDIA_DEFAULTS).expect("MEDIA_DEFAULTS is valid JSON");
    let mut p = Properties::from_value(all).expect("MEDIA_DEFAULTS is an object");
    p.retain(|name| match name {
        "speed" => kind.has_timeline(),
        "mute" => kind.has_audio(),
        _ => true,
    });
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props() -> Properties {
        Properties::from_value(serde_json::from_str(MEDIA_DEFAULTS).unwrap()).unwrap()
    }

    #[test]
    fn media_defaults_follow_the_kind() {
        let names = |k: Kind| media_defaults(k).controls().into_iter().map(|(n, _)| n).collect::<Vec<_>>();
        let has = |k: Kind, n: &str| names(k).iter().any(|x| x == n);
        assert!(has(Kind::Video, "mute") && has(Kind::Video, "speed"));
        assert!(has(Kind::VideoStream, "mute") && has(Kind::VideoStream, "speed"));
        assert!(!has(Kind::Gif, "mute") && has(Kind::Gif, "speed"));
        assert!(!has(Kind::Picture, "mute") && !has(Kind::Picture, "speed"));
        assert!(has(Kind::Picture, "scaler") && has(Kind::Picture, "brightness"));
    }

    #[test]
    fn parses_in_file_order_and_preserves_unknown_fields() {
        let mut p = Properties::from_value(serde_json::json!({
            "b": {"type": "checkbox", "text": "B", "value": true, "extra": 5},
            "a": {"type": "Slider", "text": "A", "value": 3, "min": 0, "max": 10},
            "weird": {"type": "gizmo", "value": 1}
        }))
        .unwrap();
        let names: Vec<_> = p.controls().into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["b", "a"]);
        p.set("b", &Value::String("false".into())).unwrap();
        assert_eq!(p.raw["b"]["extra"], 5);
        assert_eq!(p.raw["b"]["value"], false);
        assert!(p.set("weird", &Value::from(1)).is_err());
    }

    #[test]
    fn clamps_and_applies_relative_changes() {
        let mut p = props();
        assert_eq!(p.set("saturation", &Value::from(500)).unwrap(), 100.0);
        assert_eq!(p.set("saturation", &Value::String("--30".into())).unwrap(), 70.0);
        assert_eq!(p.set("scaler", &Value::String("++9".into())).unwrap(), 3);
        assert_eq!(p.set("mute", &Value::String("true".into())).unwrap(), true);
        assert!(p.set("nope", &Value::Null).is_err());
    }

    #[test]
    fn folder_dropdown_stores_basename_and_sends_relative_path() {
        let mut p = Properties::from_value(serde_json::json!({
            "img": {"type": "folderDropdown", "text": "Image", "value": "a.png", "folder": "media/", "filter": "*.png"}
        }))
        .unwrap();
        assert_eq!(p.get("img").unwrap().value().unwrap(), "media/a.png");
        assert_eq!(p.set("img", &Value::String("media/b.png".into())).unwrap(), "media/b.png");
        assert_eq!(p.raw["img"]["value"], "b.png");
        assert_eq!(p.set("img", &Value::Null).unwrap(), Value::Null);
    }
}
