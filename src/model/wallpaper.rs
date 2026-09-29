use crate::error::{Error, Result};
use crate::model::info::{FILE_NAME, PROPERTIES_FILE_NAME};
use crate::model::{Info, Kind};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where a wallpaper's customization controls come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertySource {
    None,
    /// The wallpaper ships its own `LivelyProperties.json`.
    File(PathBuf),
    /// Media wallpapers get the built-in libmpv controls.
    BuiltinMedia,
}

/// A library entry: a directory holding `LivelyInfo.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Wallpaper {
    pub id: String,
    pub dir: PathBuf,
    pub info: Info,
    /// Absolute path of the content file, or the URL for online kinds.
    pub source: String,
    pub thumbnail: Option<PathBuf>,
    pub properties: PropertySource,
}

/// Wire form of a wallpaper for the UI and CLI.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub kind: Kind,
    pub author: Option<String>,
    pub desc: Option<String>,
    pub contact: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Command line arguments of program wallpapers.
    #[serde(default)]
    pub arguments: Option<String>,
    pub thumbnail: Option<PathBuf>,
    pub customizable: bool,
    pub source: String,
    pub dir: PathBuf,
    pub absolute: bool,
}

impl Wallpaper {
    pub fn load(dir: &Path) -> Result<Wallpaper> {
        let info_path = dir.join(FILE_NAME);
        if !info_path.is_file() {
            return Err(Error::NotFound(format!("{} has no {FILE_NAME}", dir.display())));
        }
        let info = Info::load(&info_path)?;
        Ok(Wallpaper::from_info(dir, info))
    }

    pub fn from_info(dir: &Path, info: Info) -> Wallpaper {
        let kind = info.kind;
        let source = if kind.is_online() || info.is_absolute_path {
            info.file_name.clone()
        } else {
            dir.join(&info.file_name).to_string_lossy().into_owned()
        };
        let thumbnail = info
            .thumbnail
            .as_deref()
            .filter(|t| !t.is_empty())
            .map(|t| dir.join(Path::new(t).file_name().unwrap_or_default()))
            .filter(|p| p.is_file());
        let properties = if kind.is_media() {
            PropertySource::BuiltinMedia
        } else if kind == Kind::Url {
            PropertySource::None
        } else {
            let root = if info.is_absolute_path {
                Path::new(&source).parent().map(Path::to_path_buf).unwrap_or_else(|| dir.to_path_buf())
            } else {
                dir.to_path_buf()
            };
            let candidate = root.join(PROPERTIES_FILE_NAME);
            if candidate.is_file() { PropertySource::File(candidate) } else { PropertySource::None }
        };
        Wallpaper {
            id: dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            dir: dir.to_path_buf(),
            info,
            source,
            thumbnail,
            properties,
        }
    }

    pub fn kind(&self) -> Kind {
        self.info.kind
    }

    pub fn title(&self) -> String {
        if !self.info.title.trim().is_empty() {
            return self.info.title.trim().to_string();
        }
        if self.kind().is_online() {
            return self.source.split("://").nth(1).unwrap_or(&self.source).trim_start_matches("www.").to_string();
        }
        Path::new(&self.source).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| self.id.clone())
    }

    /// Local content file, when the wallpaper is not online.
    pub fn source_path(&self) -> Option<PathBuf> {
        (!self.kind().is_online()).then(|| PathBuf::from(&self.source))
    }

    /// Directory from which web and program content is served.
    pub fn root_dir(&self) -> PathBuf {
        self.source_path()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| self.dir.clone())
    }

    pub fn info_path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    pub fn save_info(&self) -> Result<()> {
        self.info.save(&self.info_path())
    }

    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            title: self.title(),
            kind: self.kind(),
            author: self.info.author.clone().filter(|s| !s.trim().is_empty()),
            desc: self.info.desc.clone().filter(|s| !s.trim().is_empty()),
            contact: self.info.contact.clone().filter(|s| !s.trim().is_empty()),
            license: self.info.license.clone().filter(|s| !s.trim().is_empty()),
            arguments: self.info.arguments.clone().filter(|s| !s.trim().is_empty()),
            thumbnail: self.thumbnail.clone(),
            customizable: self.properties != PropertySource::None,
            source: self.source.clone(),
            dir: self.dir.clone(),
            absolute: self.info.is_absolute_path,
        }
    }
}
