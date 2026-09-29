//! The wallpaper library: a directory of wallpaper folders, each holding `LivelyInfo.json`.

use crate::error::{Error, Result, ctx};
use crate::media::thumb;
use crate::model::info::FILE_NAME;
use crate::model::kind::PACKAGE_EXTENSIONS;
use crate::model::{Info, Kind, Wallpaper};
use crate::paths::{file_name, nonce, slug};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const THUMBNAIL: &str = "thumbnail.jpg";

#[derive(Clone, Copy)]
pub struct ImportOptions<'a> {
    pub copy: bool,
    pub thumbnails: bool,
    pub temp_dir: &'a Path,
}

#[derive(Default)]
pub struct Imported {
    pub wallpapers: Vec<Wallpaper>,
    pub problems: Vec<String>,
}

impl Imported {
    fn record(&mut self, result: Result<Wallpaper>, progress: &mut dyn FnMut(&Wallpaper)) {
        match result {
            Ok(w) => {
                progress(&w);
                self.wallpapers.push(w);
            }
            Err(e) => self.problems.push(e.to_string()),
        }
    }
}

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

    /// Bring a file, folder, Lively package, or URL into the library
    pub fn import(&self, source: &str, opts: &ImportOptions<'_>, progress: &mut dyn FnMut(&Wallpaper)) -> Result<Imported> {
        ctx(std::fs::create_dir_all(&self.dir), self.dir.display())?;
        let mut out = Imported::default();
        let path = PathBuf::from(source);
        let is_url = source.starts_with("http://") || source.starts_with("https://");
        if !is_url && path.is_dir() {
            let root = std::path::absolute(&path)?;
            self.import_tree(&root, opts, &mut HashSet::new(), progress, &mut out);
            return match (out.wallpapers.is_empty(), out.problems.is_empty()) {
                (true, true) => Err(Error::Unsupported(format!("{} contains no wallpapers", root.display()))),
                (true, false) => Err(Error::Invalid(out.problems.join("\n"))),
                _ => Ok(out),
            };
        }
        let wallpaper = if is_url {
            self.import_url(source, opts.thumbnails)?
        } else if path.is_file() {
            self.import_file(&path, opts)?
        } else {
            return Err(Error::NotFound(format!("{source} does not exist")));
        };
        progress(&wallpaper);
        out.wallpapers.push(wallpaper);
        Ok(out)
    }

    /// One file: a Lively package, a web page, a program, or media.
    fn import_file(&self, path: &Path, opts: &ImportOptions<'_>) -> Result<Wallpaper> {
        let ext = extension(path);
        if PACKAGE_EXTENSIONS.contains(&ext.as_str()) {
            return self.import_zip(path);
        }
        let kind = Kind::from_extension(&ext)
            .or_else(|| is_executable(path).then_some(Kind::Program))
            .ok_or_else(|| Error::Unsupported(format!("{} is not a supported wallpaper format", if ext.is_empty() { path.display().to_string() } else { format!(".{ext} files") })))?;
        let source_abs = std::path::absolute(path)?;
        if kind == Kind::Web {
            return self.import_web(&source_abs, opts.copy);
        }
        let title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Wallpaper".into());
        let dir = self.new_dir(&title)?;
        let mut info = Info { title: title.clone(), kind, ..Info::default() };
        if kind.is_media() && opts.copy {
            let name = file_name(&source_abs);
            ctx(std::fs::copy(&source_abs, dir.join(&name)), source_abs.display())?;
            info.file_name = name;
        } else {
            info.file_name = source_abs.to_string_lossy().into_owned();
            info.is_absolute_path = true;
        }
        if kind.is_media() && opts.thumbnails {
            match thumb::capture(&source_abs, kind, &dir.join(THUMBNAIL), opts.temp_dir) {
                Ok(()) => info.thumbnail = Some(THUMBNAIL.into()),
                Err(e) => log::warn!("thumbnail for {}: {e}", source_abs.display()),
            }
        }
        info.save(&dir.join(FILE_NAME))?;
        Wallpaper::load(&dir)
    }

    fn import_tree(&self, dir: &Path, opts: &ImportOptions<'_>, seen: &mut HashSet<PathBuf>, progress: &mut dyn FnMut(&Wallpaper), out: &mut Imported) {
        match std::fs::canonicalize(dir) {
            Ok(real) => {
                if !seen.insert(real) {
                    return;
                }
            }
            Err(e) => {
                out.problems.push(format!("{}: {e}", dir.display()));
                return;
            }
        }
        if dir.join(FILE_NAME).is_file() {
            out.record(self.import_lively_dir(dir).map_err(|e| Error::Invalid(format!("{}: {e}", dir.display()))), progress);
            return;
        }
        if let Some(index) = find_index(dir) {
            out.record(self.import_web(&index, opts.copy).map_err(|e| Error::Invalid(format!("{}: {e}", dir.display()))), progress);
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                out.problems.push(format!("{}: {e}", dir.display()));
                return;
            }
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| !file_name(p).starts_with('.')).collect();
        paths.sort();
        for p in paths {
            if p.is_dir() {
                self.import_tree(&p, opts, seen, progress, out);
            } else if p.is_file() {
                let ext = extension(&p);
                if PACKAGE_EXTENSIONS.contains(&ext.as_str()) || Kind::from_extension(&ext).is_some() {
                    out.record(self.import_file(&p, opts).map_err(|e| Error::Invalid(format!("{}: {e}", p.display()))), progress);
                }
            }
        }
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

    /// A folder that already is a Lively wallpaper
    fn import_lively_dir(&self, path: &Path) -> Result<Wallpaper> {
        if path.parent().is_some_and(|p| p == self.dir) {
            return Wallpaper::load(path);
        }
        let info = Info::load(&path.join(FILE_NAME))?;
        let dir = self.new_dir(&info.title)?;
        copy_tree(path, &dir)?;
        Wallpaper::load(&dir)
    }

    fn import_web(&self, index: &Path, copy: bool) -> Result<Wallpaper> {
        let root = index.parent().filter(|p| !p.as_os_str().is_empty()).ok_or_else(|| Error::Invalid(format!("{} has no parent folder", index.display())))?;
        if root.join(FILE_NAME).is_file() {
            return self.import_lively_dir(root);
        }
        let stem = index.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let title = if stem.eq_ignore_ascii_case("index") { file_name(root) } else { stem };
        let dir = self.new_dir(&title)?;
        let mut info = Info { title, kind: Kind::Web, ..Info::default() };
        if copy {
            copy_tree(root, &dir)?;
            info.file_name = file_name(index);
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

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn find_index(dir: &Path) -> Option<PathBuf> {
    ["index.html", "index.htm"].into_iter().map(|n| dir.join(n)).find(|p| p.is_file())
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

    /// Import source for one wallpaper
    fn one(lib: &Library, source: &Path, copy: bool, root: &Path) -> Result<Wallpaper> {
        let opts = ImportOptions { copy, thumbnails: false, temp_dir: root };
        let mut imported = lib.import(source.to_str().unwrap(), &opts, &mut |_| {})?;
        assert_eq!(imported.wallpapers.len(), 1, "{}", source.display());
        assert!(imported.problems.is_empty(), "{:?}", imported.problems);
        Ok(imported.wallpapers.remove(0))
    }

    #[test]
    fn imports_media_by_reference_and_scans() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let video = root.join("clip.mp4");
        std::fs::write(&video, b"not really a video").unwrap();
        let w = one(&lib, &video, false, &root).unwrap();
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
        let copied = one(&lib, &site, true, &root).unwrap();
        assert_eq!(copied.kind(), Kind::Web);
        assert!(!copied.info.is_absolute_path);
        assert!(copied.dir.join("assets/a.js").is_file());
        assert!(matches!(copied.properties, crate::model::wallpaper::PropertySource::File(_)));

        let package = root.join("pkg.zip");
        lib.export(&copied, &package).unwrap();
        let other = Library { dir: root.join("other") };
        let restored = one(&other, &package, false, &root).unwrap();
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
    fn html_file_imports_like_its_folder() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let site = root.join("aurora");
        std::fs::create_dir_all(site.join("js")).unwrap();
        std::fs::write(site.join("index.html"), "<html></html>").unwrap();
        std::fs::write(site.join("js/app.js"), "1").unwrap();
        let index = site.join("index.html");

        let referenced = one(&lib, &index, false, &root).unwrap();
        assert_eq!(referenced.kind(), Kind::Web);
        assert_eq!(referenced.title(), "aurora");
        assert!(referenced.info.is_absolute_path);
        assert_eq!(referenced.root_dir(), site);

        let copied = one(&lib, &index, true, &root).unwrap();
        assert!(!copied.info.is_absolute_path);
        assert!(copied.dir.join("index.html").is_file());
        assert!(copied.dir.join("js/app.js").is_file());
        assert_eq!(copied.root_dir(), copied.dir);

        let page = site.join("nebula.html");
        std::fs::write(&page, "<html></html>").unwrap();
        let named = one(&lib, &page, false, &root).unwrap();
        assert_eq!(named.title(), "nebula");

        let lively = root.join("packaged");
        std::fs::create_dir_all(&lively).unwrap();
        std::fs::write(lively.join("index.html"), "<html></html>").unwrap();
        std::fs::write(lively.join("LivelyInfo.json"), r#"{"Title":"Packaged","Type":2,"FileName":"index.html"}"#).unwrap();
        let via_html = one(&lib, &lively.join("index.html"), false, &root).unwrap();
        assert_eq!(via_html.kind(), Kind::WebAudio);
        assert_eq!(via_html.title(), "Packaged");
        assert!(via_html.dir.join("index.html").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_imports_everything_inside_flat_and_reports_problems() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let pack = root.join("pack");
        for d in ["nested/deeper", "site", "lively", ".hidden"] {
            std::fs::create_dir_all(pack.join(d)).unwrap();
        }
        std::fs::write(pack.join("b.mp4"), "v").unwrap();
        std::fs::write(pack.join("a.png"), "p").unwrap();
        std::fs::write(pack.join("notes.txt"), "not a wallpaper").unwrap();
        std::fs::write(pack.join("nested/deeper/c.gif"), "g").unwrap();
        std::fs::write(pack.join("site/index.html"), "<html></html>").unwrap();
        std::fs::write(pack.join("site/extra.mp4"), "belongs to the site").unwrap();
        std::fs::write(pack.join("lively/LivelyInfo.json"), r#"{"Title":"Packed","Type":7,"FileName":"clip.mp4"}"#).unwrap();
        std::fs::write(pack.join("lively/clip.mp4"), "v").unwrap();
        std::fs::write(pack.join(".hidden/d.mp4"), "v").unwrap();
        std::fs::write(pack.join("broken.zip"), b"PK\x03\x04junk").unwrap();

        let opts = ImportOptions { copy: false, thumbnails: false, temp_dir: &root };
        let mut seen = Vec::new();
        let imported = lib.import(pack.to_str().unwrap(), &opts, &mut |w| seen.push(w.title())).unwrap();
        let mut titles: Vec<String> = imported.wallpapers.iter().map(|w| w.title()).collect();
        titles.sort();
        assert_eq!(titles, ["Packed", "a", "b", "c", "site"]);
        assert_eq!(seen.len(), 5, "progress runs once per wallpaper");
        assert_eq!(imported.problems.len(), 1, "{:?}", imported.problems);
        assert!(imported.problems[0].contains("broken.zip"));
        assert_eq!(lib.scan().len(), 5);

        let nothing = root.join("nothing");
        std::fs::create_dir_all(&nothing).unwrap();
        assert!(matches!(lib.import(nothing.to_str().unwrap(), &opts, &mut |_| {}), Err(Error::Unsupported(_))));
        let only_bad = root.join("only-bad");
        std::fs::create_dir_all(&only_bad).unwrap();
        std::fs::write(only_bad.join("x.zip"), b"PK\x03\x04junk").unwrap();
        assert!(matches!(lib.import(only_bad.to_str().unwrap(), &opts, &mut |_| {}), Err(Error::Invalid(_))));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_unsupported_and_unsafe_input() {
        let root = temp();
        let lib = Library { dir: root.join("library") };
        let odd = root.join("notes.txt");
        std::fs::write(&odd, "x").unwrap();
        assert!(matches!(one(&lib, &odd, false, &root), Err(Error::Unsupported(_))));
        assert!(matches!(one(&lib, &root.join("missing.mp4"), false, &root), Err(Error::NotFound(_))));
        let bad_zip = root.join("bad.zip");
        std::fs::write(&bad_zip, b"PK\x03\x04junk").unwrap();
        assert!(one(&lib, &bad_zip, false, &root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
