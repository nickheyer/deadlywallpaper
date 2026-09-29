use crate::error::{Result, ctx};
use crate::model::Kind;
use serde::{Deserialize, Deserializer, Serialize};
use std::path::Path;

pub const FILE_NAME: &str = "LivelyInfo.json";
pub const PROPERTIES_FILE_NAME: &str = "LivelyProperties.json";

/// LivelyInfo.json metadata.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", default)]
pub struct Info {
    #[serde(deserialize_with = "null_string")]
    pub app_version: String,
    #[serde(deserialize_with = "null_string")]
    pub title: String,
    pub thumbnail: Option<String>,
    pub preview: Option<String>,
    pub desc: Option<String>,
    pub author: Option<String>,
    pub license: Option<String>,
    pub contact: Option<String>,
    #[serde(rename = "Type", with = "crate::model::kind::lively")]
    pub kind: Kind,
    #[serde(deserialize_with = "null_string")]
    pub file_name: String,
    pub arguments: Option<String>,
    pub is_absolute_path: bool,
    pub id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub version: i64,
}

impl Default for Info {
    fn default() -> Self {
        Info {
            app_version: format!("deadlywp {}", env!("CARGO_PKG_VERSION")),
            title: String::new(),
            thumbnail: None,
            preview: None,
            desc: None,
            author: None,
            license: None,
            contact: None,
            kind: Kind::Web,
            file_name: String::new(),
            arguments: None,
            is_absolute_path: false,
            id: None,
            tags: None,
            version: 0,
        }
    }
}

impl Info {
    pub fn load(path: &Path) -> Result<Info> {
        let text = ctx(std::fs::read_to_string(path), path.display())?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        crate::paths::write(path, text)
    }

    /// Program arguments split shell-style on whitespace with simple quoting.
    #[cfg(any(target_os = "linux", windows, test))]
    pub fn args(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut quote: Option<char> = None;
        for c in self.arguments.as_deref().unwrap_or("").chars() {
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), c) => cur.push(c),
                (None, '"' | '\'') => quote = Some(c),
                (None, c) if c.is_whitespace() => {
                    if !cur.is_empty() {
                        out.push(std::mem::take(&mut cur));
                    }
                }
                (None, c) => cur.push(c),
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }
}

fn null_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_lively_numeric_types_and_nulls() {
        let text = r#"{"AppVersion":"2.0.6.0","Title":"Rain","Thumbnail":"t.jpg","Preview":null,"Desc":null,
            "Author":"a","License":null,"Contact":"https://x","Type":7,"FileName":"rain.mp4","Arguments":null,
            "IsAbsolutePath":false,"Id":null,"Tags":null,"Version":0}"#;
        let info: Info = serde_json::from_str(text).unwrap();
        assert_eq!(info.kind, Kind::Video);
        assert_eq!(info.title, "Rain");
        assert_eq!(info.desc, None);
        let back = serde_json::to_value(&info).unwrap();
        assert_eq!(back["Type"], 7);
        assert_eq!(back["Thumbnail"], "t.jpg");
    }

    #[test]
    fn tolerates_missing_and_string_types() {
        let info: Info =
            serde_json::from_str(r#"{"Title":"x","Type":"godot","FileName":"g.exe"}"#).unwrap();
        assert_eq!(info.kind, Kind::Program);
        let info: Info = serde_json::from_str(r#"{"FileName":"index.html"}"#).unwrap();
        assert_eq!(info.kind, Kind::Web);
    }

    #[test]
    fn splits_arguments() {
        let info = Info {
            arguments: Some(r#"--a "two words" -b 'c d'"#.into()),
            ..Info::default()
        };
        assert_eq!(info.args(), vec!["--a", "two words", "-b", "c d"]);
    }
}
