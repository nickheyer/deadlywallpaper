//! The wallpaper library: a directory of wallpaper folders, each holding `LivelyInfo.json`.

use crate::error::{Error, Result, ctx};
use crate::media::thumb;
use crate::model::info::FILE_NAME;
use crate::model::{Info, Kind, Wallpaper};
use crate::paths::{file_name, nonce, slug};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const THUMBNAIL: &str = "thumbnail.jpg";

#[derive(Clone)]
pub struct Library {
    pub dir: PathBuf,
}

impl Library {
    pub fn scan(&self) -> Vec<Wallpaper> {
        let mut out: Vec<Wallpaper> = std::fs::read_dir(&self.dir)
            .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).filter_map(|e| Wallpaper::load(&e.path()).ok()).collect())
            .unwrap_or_default();
        out.sort_by_key(|w| w.title().to_lowercase());
        out
    }

    pub fn get(&self, id: &str) -> Result<Wallpaper> {
        if id.is_empty() || id.contains(['/', '\\']) || id == ".." {
            return Err(Error::NotFound(format!("no wallpaper '{id}'")));
        }
        Wallpaper::load(&self.dir.join(id)).map_err(|_| Error::NotFound(format!("no wallpaper '{id}'")))
    }

    fn new_dir(&self, title: &str) -> Result<PathBuf> {
        for _ in 0..8 {
            let dir = self.dir.join(format!("{}-{}", slug(title), nonce()));
            if !dir.exists() {
                ctx(std::fs::create_dir_all(&dir), dir.display())?;
                return Ok(dir);
            }
        }
        Err(Error::Io(std::io::Error::other("could not allocate a library directory")))
    }

    /// Bring a file, folder, Lively package, or URL into the library.
    pub fn import(&self, source: &str, copy_media: bool, temp_dir: &Path, want_thumbnail: bool) -> Result<Wallpaper> {
        ctx(std::fs::create_dir_all(&self.dir), self.dir.display())?;
        if source.starts_with("http://") || source.starts_with("https://") {
            return self.import_url(source, want_thumbnail);
        }
        let path = PathBuf::from(source);
        if path.is_dir() {
            return self.import_dir(&path, copy_media);
        }
        if !path.is_file() {
            return Err(Error::NotFound(format!("{source} does not exist")));
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == "zip" {
            return self.import_zip(&path);
        }
        let kind = Kind::from_extension(&ext)
            .or_else(|| is_executable(&path).then_some(Kind::Program))
            .ok_or_else(|| Error::Unsupported(format!("{} is not a supported wallpaper format", if ext.is_empty() { path.display().to_string() } else { format!(".{ext} files") })))?;
        let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Wallpaper".into());
        let dir = self.new_dir(&title)?;
        let mut info = Info { title: title.clone(), kind, ..Info::default() };
        let source_abs = std::path::absolute(&path)?;
        if kind.is_media() && copy_media {
            let name = file_name(&source_abs);
            ctx(std::fs::copy(&source_abs, dir.join(&name)), source_abs.display())?;
            info.file_name = name;
        } else {
            info.file_name = source_abs.to_string_lossy().into_owned();
            info.is_absolute_path = true;
        }
        if kind.is_media() && want_thumbnail {
            match thumb::capture(&source_abs, kind, &dir.join(THUMBNAIL), temp_dir) {
                Ok(()) => info.thumbnail = Some(THUMBNAIL.into()),
                Err(e) => log::warn!("thumbnail for {}: {e}", source_abs.display()),
            }
        }
        info.save(&dir.join(FILE_NAME))?;
        Wallpaper::load(&dir)
    }

    fn import_url(&self, url: &str, want_thumbnail: bool) -> Result<Wallpaper> {
        let (kind, title, file_name) = match super::stream::probe(url) {
            Ok(Some(p)) => (Kind::VideoStream, if p.title.is_empty() { url.to_string() } else { p.title }, url.to_string()),
            Ok(None) => (Kind::Url, host(url), url.to_string()),
            Err(Error::Unsupported(_)) => match super::stream::youtube_embed(url) {
                Some(embed) => (Kind::Url, host(url), embed),
                None => (Kind::Url, host(url), url.to_string()),
            },
            Err(e) => return Err(e),
        };
        let dir = self.new_dir(&title)?;
        let mut info = Info { title, kind, file_name, contact: Some(url.to_string()), is_absolute_path: true, ..Info::default() };
        if kind == Kind::VideoStream && want_thumbnail {
            match super::stream::thumbnail(url, &dir) {
                Ok(name) => info.thumbnail = Some(name),
                Err(e) => log::warn!("stream thumbnail: {e}"),
            }
        }
        info.save(&dir.join(FILE_NAME))?;
        Wallpaper::load(&dir)
    }

    fn import_dir(&self, path: &Path, copy: bool) -> Result<Wallpaper> {
        let path = std::path::absolute(path)?;
        if path.join(FILE_NAME).is_file() {
            if path.parent().is_some_and(|p| p == self.dir) {
                return Wallpaper::load(&path);
            }
            let info = Info::load(&path.join(FILE_NAME))?;
            let dir = self.new_dir(&info.title)?;
            copy_tree(&path, &dir)?;
            return Wallpaper::load(&dir);
        }
        let index = ["index.html", "index.htm"].into_iter().map(|n| path.join(n)).find(|p| p.is_file()).ok_or_else(|| {
            Error::Unsupported(format!("{} has neither {FILE_NAME} nor index.html", path.display()))
        })?;
        let title = file_name(&path);
        let dir = self.new_dir(&title)?;
        let mut info = Info { title, kind: Kind::Web, ..Info::default() };
        if copy {
            copy_tree(&path, &dir)?;
            info.file_name = file_name(&index);
        } else {
            info.file_name = index.to_string_lossy().into_owned();
            info.is_absolute_path = true;
        }
        info.save(&dir.join(FILE_NAME))?;
        Wallpaper::load(&dir)
    }

    fn import_zip(&self, path: &Path) -> Result<Wallpaper> {
        let file = ctx(std::fs::File::open(path), path.display())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
        let info_entry = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|n| n.rsplit('/').next() == Some(FILE_NAME))
            .min_by_key(|n| n.len())
            .ok_or_else(|| Error::Invalid(format!("{} is not a Lively wallpaper package (no {FILE_NAME})", path.display())))?;
        let prefix = info_entry.trim_end_matches(FILE_NAME).to_string();
        let mut info_text = String::new();
        archive.by_name(&info_entry).map_err(|e| Error::Invalid(e.to_string()))?.read_to_string(&mut info_text)?;
        let info: Info = serde_json::from_str(&info_text)?;
        let title = if info.title.trim().is_empty() { file_name(path) } else { info.title.clone() };
        let dir = self.new_dir(&title)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| Error::Invalid(e.to_string()))?;
            let name = entry.name().to_string();
            let Some(rel) = name.strip_prefix(&prefix) else { continue };
            if rel.is_empty() {
                continue;
            }
            let mut target = dir.clone();
            for part in rel.split('/') {
                if part.is_empty() || part == "." {
                    continue;
                }
                if part == ".." {
                    return Err(Error::Invalid(format!("{} contains an unsafe path", path.display())));
                }
                target.push(part);
            }
            if entry.is_dir() {
                ctx(std::fs::create_dir_all(&target), target.display())?;
            } else {
                if let Some(parent) = target.parent() {
                    ctx(std::fs::create_dir_all(parent), parent.display())?;
                }
                let mut out = ctx(std::fs::File::create(&target), target.display())?;
                std::io::copy(&mut entry, &mut out)?;
            }
        }
        Wallpaper::load(&dir)
    }

    /// Remove a wallpaper directory and its property copies. Only directories inside the
    /// library are deleted; referenced media outside it stays untouched.
    pub fn delete(&self, id: &str, properties_dir: &Path) -> Result<()> {
        let wp = self.get(id)?;
        let dir = std::path::absolute(&wp.dir)?;
        if dir.parent() != Some(std::path::absolute(&self.dir)?.as_path()) {
            return Err(Error::Invalid(format!("{} is outside the library", dir.display())));
        }
        ctx(std::fs::remove_dir_all(&dir), dir.display())?;
        let _ = std::fs::remove_dir_all(properties_dir.join(id));
        Ok(())
    }

    /// Write a Lively package: the wallpaper folder, plus referenced files when the wallpaper
    /// points outside the library, with paths rewritten relative.
    pub fn export(&self, wp: &Wallpaper, file: &Path) -> Result<()> {
        let out = ctx(std::fs::File::create(file), file.display())?;
        let mut zip = zip::ZipWriter::new(out);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut info = wp.info.clone();
        let mut skip_info = false;
        add_tree(&mut zip, &wp.dir, "", &opts, &mut |rel| {
            if rel == FILE_NAME {
                skip_info = true;
                false
            } else {
                true
            }
        })?;
        let _ = skip_info;
        if wp.info.is_absolute_path && !wp.kind().is_online() {
            let src = PathBuf::from(&wp.source);
            if wp.kind().is_directory_project() {
                let root = wp.root_dir();
                add_tree(&mut zip, &root, "content/", &opts, &mut |_| true)?;
                let rel = src.strip_prefix(&root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| file_name(&src));
                info.file_name = format!("content/{rel}");
            } else if src.is_file() {
                let name = file_name(&src);
                zip.start_file(&name, opts).map_err(|e| Error::Io(std::io::Error::other(e)))?;
                let mut f = ctx(std::fs::File::open(&src), src.display())?;
                std::io::copy(&mut f, &mut zip)?;
                info.file_name = name;
            }
            info.is_absolute_path = false;
        }
        info.thumbnail = wp.thumbnail.as_ref().map(|t| file_name(t));
        zip.start_file(FILE_NAME, opts).map_err(|e| Error::Io(std::io::Error::other(e)))?;
        zip.write_all(serde_json::to_string_pretty(&info)?.as_bytes())?;
        zip.finish().map_err(|e| Error::Io(std::io::Error::other(e)))?;
        Ok(())
    }
}

/// Executable files without a media extension are program wallpapers.
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("bat") || e.eq_ignore_ascii_case("cmd"))
}

fn host(url: &str) -> String {
    url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or(url).trim_start_matches("www.").to_string()
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    ctx(std::fs::create_dir_all(to), to.display())?;
    for entry in ctx(std::fs::read_dir(from), from.display())?.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            ctx(std::fs::copy(&src, &dst).map(|_| ()), src.display())?;
        }
    }
    Ok(())
}

fn add_tree<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    prefix: &str,
    opts: &zip::write::SimpleFileOptions,
    keep: &mut dyn FnMut(&str) -> bool,
) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in ctx(std::fs::read_dir(&dir), dir.display())?.flatten() {
            let path = entry.path();
            let rel = path.strip_prefix(root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
            if path.is_dir() {
                stack.push(path);
            } else if keep(&rel) {
                zip.start_file(format!("{prefix}{rel}"), *opts).map_err(|e| Error::Io(std::io::Error::other(e)))?;
                let mut f = ctx(std::fs::File::open(&path), path.display())?;
                std::io::copy(&mut f, zip)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deadlywp-test-{}", nonce()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn imports_media_by_reference_and_scans() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let video = root.join("clip.mp4");
        std::fs::write(&video, b"not really a video").unwrap();
        let w = lib.import(video.to_str().unwrap(), false, &root, false).unwrap();
        assert_eq!(w.kind(), Kind::Video);
        assert!(w.info.is_absolute_path);
        assert_eq!(PathBuf::from(&w.source), video);
        assert_eq!(w.title(), "clip");
        assert_eq!(lib.scan().len(), 1);
        assert_eq!(lib.get(&w.id).unwrap().id, w.id);
        assert!(lib.get("../etc").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn imports_web_folder_and_lively_package_round_trip() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let site = root.join("site");
        std::fs::create_dir_all(site.join("assets")).unwrap();
        std::fs::write(site.join("index.html"), "<html></html>").unwrap();
        std::fs::write(site.join("assets/a.js"), "1").unwrap();
        std::fs::write(site.join("LivelyProperties.json"), r#"{"hue":{"type":"slider","text":"Hue","value":1,"min":0,"max":9}}"#).unwrap();
        let copied = lib.import(site.to_str().unwrap(), true, &root, false).unwrap();
        assert_eq!(copied.kind(), Kind::Web);
        assert!(!copied.info.is_absolute_path);
        assert!(copied.dir.join("assets/a.js").is_file());
        assert!(matches!(copied.properties, crate::model::wallpaper::PropertySource::File(_)));

        let package = root.join("pkg.zip");
        lib.export(&copied, &package).unwrap();
        let other = Library { dir: root.join("other") };
        let restored = other.import(package.to_str().unwrap(), false, &root, false).unwrap();
        assert_eq!(restored.title(), copied.title());
        assert!(restored.dir.join("index.html").is_file());
        assert!(restored.dir.join("assets/a.js").is_file());
        assert!(matches!(restored.properties, crate::model::wallpaper::PropertySource::File(_)));

        let props_dir = root.join("props");
        std::fs::create_dir_all(props_dir.join(&copied.id)).unwrap();
        lib.delete(&copied.id, &props_dir).unwrap();
        assert!(lib.scan().is_empty());
        assert!(!props_dir.join(&copied.id).exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_unsupported_and_unsafe_input() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let odd = root.join("notes.txt");
        std::fs::write(&odd, "x").unwrap();
        assert!(matches!(lib.import(odd.to_str().unwrap(), false, &root, false), Err(Error::Unsupported(_))));
        assert!(matches!(lib.import(root.join("missing.mp4").to_str().unwrap(), false, &root, false), Err(Error::NotFound(_))));
        let bad_zip = root.join("bad.zip");
        std::fs::write(&bad_zip, b"PK\x03\x04junk").unwrap();
        assert!(lib.import(bad_zip.to_str().unwrap(), false, &root, false).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
