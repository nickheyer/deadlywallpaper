use crate::error::{Error, Result};
use crate::model::info::{FILE_NAME, PROPERTIES_FILE_NAME};
use crate::model::{Info, Kind};
use crate::we::project::Project;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where a wallpaper's customization controls come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertySource {
    None,
    /// The wallpaper ships its own `LivelyProperties.json`.
    File(PathBuf),
    /// Media wallpapers get the built-in libmpv controls.
    BuiltinMedia,
    /// A Wallpaper Engine wallpaper's `general.properties`, read from this `project.json`.
    WallpaperEngine(PathBuf),
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
    /// The Wallpaper Engine project this entry was made from, when it is one.
    pub we: Option<Project>,
}

/// Where a Wallpaper Engine entry came from on the Steam Workshop.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkshopOrigin {
    pub id: u64,
    /// Steam's `timeupdated` of the download this entry was made from.
    pub updated: Option<u64>,
    /// The folder Steam downloaded the item to.
    pub source: Option<PathBuf>,
}

impl WorkshopOrigin {
    pub const FILE_NAME: &str = "workshop.json";

    pub fn load(dir: &Path) -> Option<WorkshopOrigin> {
        let text = std::fs::read_to_string(dir.join(WorkshopOrigin::FILE_NAME)).ok()?;
        serde_json::from_str(&text).ok().filter(|o: &WorkshopOrigin| o.id > 0)
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        crate::paths::write(
            &dir.join(WorkshopOrigin::FILE_NAME),
            serde_json::to_string_pretty(self)?,
        )
    }
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
    /// Bytes of the content file, or of every file under the project folder for web and
    /// program wallpapers. `None` for online kinds and for content that cannot be read.
    #[serde(default)]
    pub size: Option<u64>,
    /// When the entry was added to the library, seconds since the Unix epoch.
    #[serde(default)]
    pub added: Option<u64>,
    /// Last change to the content (newest file for project folders), seconds since the Unix
    /// epoch. `None` for online kinds and for content that cannot be read.
    #[serde(default)]
    pub modified: Option<u64>,
    /// Folder the content was imported from, for content referenced in place: the parent of a
    /// media file, or the parent of a web or program project folder. `None` for online kinds
    /// and for content copied into the library.
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// Steam Workshop item this wallpaper was made from.
    #[serde(default)]
    pub workshop: Option<u64>,
    /// Wallpaper Engine tags, for Wallpaper Engine wallpapers.
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Seconds since the Unix epoch.
fn epoch_secs(t: SystemTime) -> Option<u64> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

/// Size and newest modification time of one file.
fn file_stats(path: &Path) -> Option<(u64, Option<u64>)> {
    let meta = std::fs::metadata(path).ok().filter(|m| m.is_file())?;
    Some((meta.len(), meta.modified().ok().and_then(epoch_secs)))
}

/// Total size and newest modification time of every regular file under `dir`. Symbolic links
/// are not followed, so the walk cannot loop.
fn tree_stats(dir: &Path) -> Option<(u64, Option<u64>)> {
    if !dir.is_dir() {
        return None;
    }
    let mut total = 0u64;
    let mut newest: Option<u64> = None;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                    let modified = meta.modified().ok().and_then(epoch_secs);
                    newest = newest.max(modified);
                }
            }
        }
    }
    Some((total, newest))
}

impl Wallpaper {
    pub fn load(dir: &Path) -> Result<Wallpaper> {
        let info_path = dir.join(FILE_NAME);
        if !info_path.is_file() {
            return Err(Error::NotFound(format!(
                "{} has no {FILE_NAME}",
                dir.display()
            )));
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
        let project_path = dir.join(crate::we::project::FILE_NAME);
        let we = project_path
            .is_file()
            .then(|| Project::load(&project_path))
            .and_then(|r| match r {
                Ok(p) => Some(p),
                Err(e) => {
                    log::warn!("{e}");
                    None
                }
            });
        let properties = if kind.is_media() {
            PropertySource::BuiltinMedia
        } else if kind == Kind::Url {
            PropertySource::None
        } else if we.as_ref().is_some_and(|p| !p.properties.is_empty()) {
            PropertySource::WallpaperEngine(project_path.clone())
        } else if we.is_some() {
            PropertySource::None
        } else {
            let root = if info.is_absolute_path {
                Path::new(&source)
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| dir.to_path_buf())
            } else {
                dir.to_path_buf()
            };
            let candidate = root.join(PROPERTIES_FILE_NAME);
            if candidate.is_file() {
                PropertySource::File(candidate)
            } else {
                PropertySource::None
            }
        };
        Wallpaper {
            id: dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            dir: dir.to_path_buf(),
            info,
            source,
            thumbnail,
            properties,
            we,
        }
    }

    /// Whether the wallpaper wants the audio spectrum: web visualizers and Wallpaper Engine
    /// wallpapers that declare audio processing.
    pub fn wants_audio(&self) -> bool {
        self.kind() == Kind::WebAudio || self.we.as_ref().is_some_and(|p| p.audio)
    }

    pub fn kind(&self) -> Kind {
        self.info.kind
    }

    pub fn title(&self) -> String {
        if !self.info.title.trim().is_empty() {
            return self.info.title.trim().to_string();
        }
        if self.kind().is_online() {
            return self
                .source
                .split("://")
                .nth(1)
                .unwrap_or(&self.source)
                .trim_start_matches("www.")
                .to_string();
        }
        Path::new(&self.source)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.id.clone())
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

    /// Size and newest change of the content: the file itself, or the whole project folder.
    fn content_stats(&self) -> Option<(u64, Option<u64>)> {
        let path = self.source_path()?;
        if self.kind().is_directory_project() {
            tree_stats(&self.root_dir())
        } else {
            file_stats(&path)
        }
    }

    /// When the library entry was created; its directory's birth time, or its modification
    /// time on filesystems that do not record births.
    fn added(&self) -> Option<u64> {
        let meta = std::fs::metadata(&self.dir).ok()?;
        meta.created()
            .or_else(|_| meta.modified())
            .ok()
            .and_then(epoch_secs)
    }

    /// Folder the content was imported from, when it is referenced in place.
    fn source_folder(&self) -> Option<PathBuf> {
        if !self.info.is_absolute_path {
            return None;
        }
        let path = self.source_path()?;
        let owner = if self.kind().is_directory_project() {
            self.root_dir()
        } else {
            path
        };
        owner.parent().map(Path::to_path_buf)
    }

    pub fn summary(&self) -> Summary {
        let (size, modified) = match self.content_stats() {
            Some((size, modified)) => (Some(size), modified),
            None => (None, None),
        };
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
            size,
            added: self.added(),
            modified,
            folder: self.source_folder(),
            workshop: WorkshopOrigin::load(&self.dir).map(|o| o.id),
            tags: self.we.as_ref().map(|p| p.tags.clone()).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deadlywp-summary-{}", crate::paths::nonce()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn summary_reports_file_size_dates_and_folder() {
        let root = temp();
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let clip = media.join("clip.mp4");
        std::fs::write(&clip, [0u8; 1234]).unwrap();
        let entry = root.join("library").join("clip-000001");
        std::fs::create_dir_all(&entry).unwrap();
        let info = Info {
            title: "Clip".into(),
            kind: Kind::Video,
            file_name: clip.to_string_lossy().into_owned(),
            is_absolute_path: true,
            ..Info::default()
        };
        let s = Wallpaper::from_info(&entry, info).summary();
        assert_eq!(s.size, Some(1234));
        assert_eq!(s.folder.as_deref(), Some(media.as_path()));
        let clip_mtime = epoch_secs(std::fs::metadata(&clip).unwrap().modified().unwrap());
        assert_eq!(s.modified, clip_mtime);
        assert!(s.added.is_some_and(|t| t > 1_600_000_000));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn summary_sums_project_folders_and_skips_copied_and_online_folders() {
        let root = temp();
        let site = root.join("sites").join("aurora");
        std::fs::create_dir_all(site.join("js")).unwrap();
        std::fs::write(site.join("index.html"), [0u8; 100]).unwrap();
        std::fs::write(site.join("js").join("app.js"), [0u8; 50]).unwrap();
        let entry = root.join("library").join("aurora-000001");
        std::fs::create_dir_all(&entry).unwrap();
        let referenced = Wallpaper::from_info(
            &entry,
            Info {
                title: "Aurora".into(),
                kind: Kind::Web,
                file_name: site.join("index.html").to_string_lossy().into_owned(),
                is_absolute_path: true,
                ..Info::default()
            },
        )
        .summary();
        assert_eq!(referenced.size, Some(150));
        assert_eq!(
            referenced.folder.as_deref(),
            Some(root.join("sites").as_path())
        );
        assert!(referenced.modified.is_some());

        let copied_entry = root.join("library").join("copied-000002");
        std::fs::create_dir_all(&copied_entry).unwrap();
        std::fs::write(copied_entry.join("index.html"), [0u8; 20]).unwrap();
        let copied = Wallpaper::from_info(
            &copied_entry,
            Info {
                title: "Copied".into(),
                kind: Kind::Web,
                file_name: "index.html".into(),
                ..Info::default()
            },
        )
        .summary();
        assert_eq!(copied.size, Some(20));
        assert_eq!(copied.folder, None);

        let online_entry = root.join("library").join("site-000003");
        std::fs::create_dir_all(&online_entry).unwrap();
        let online = Wallpaper::from_info(
            &online_entry,
            Info {
                title: "Site".into(),
                kind: Kind::Url,
                file_name: "https://example.org".into(),
                is_absolute_path: true,
                ..Info::default()
            },
        )
        .summary();
        assert_eq!(online.size, None);
        assert_eq!(online.modified, None);
        assert_eq!(online.folder, None);
        assert!(online.added.is_some());

        let missing = Wallpaper::from_info(
            &online_entry,
            Info {
                title: "Gone".into(),
                kind: Kind::Video,
                file_name: root.join("gone.mp4").to_string_lossy().into_owned(),
                is_absolute_path: true,
                ..Info::default()
            },
        )
        .summary();
        assert_eq!(missing.size, None);
        assert_eq!(missing.modified, None);
        assert_eq!(missing.folder.as_deref(), Some(root.as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }
}
