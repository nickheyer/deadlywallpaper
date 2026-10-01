//! The wallpaper library: a directory of wallpaper folders, each holding `LivelyInfo.json`.

use crate::error::{Error, Result, ctx};
use crate::media::thumb;
use crate::model::info::FILE_NAME;
use crate::model::kind::PACKAGE_EXTENSIONS;
use crate::model::wallpaper::WorkshopOrigin;
use crate::model::{Info, Kind, Wallpaper};
use crate::paths::{file_name, nonce, slug};
use crate::we::project::{self, Project, ProjectType};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

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
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .filter_map(|e| Wallpaper::load(&e.path()).ok())
                    .collect()
            })
            .unwrap_or_default();
        out.sort_by_key(|w| w.title().to_lowercase());
        out
    }

    pub fn get(&self, id: &str) -> Result<Wallpaper> {
        if id.is_empty() || id.contains(['/', '\\', ':']) || id == "." || id == ".." {
            return Err(Error::NotFound(format!("no wallpaper '{id}'")));
        }
        Wallpaper::load(&self.dir.join(id))
            .map_err(|_| Error::NotFound(format!("no wallpaper '{id}'")))
    }

    fn new_dir(&self, title: &str) -> Result<PathBuf> {
        for _ in 0..8 {
            let dir = self.dir.join(format!("{}-{}", slug(title), nonce()));
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(dir),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return ctx(Err(e), dir.display()),
            }
        }
        Err(Error::Io(std::io::Error::other(
            "could not allocate a library directory",
        )))
    }

    fn create(&self, title: &str, populate: impl FnOnce(&Path) -> Result<()>) -> Result<Wallpaper> {
        let dir = self.new_dir(title)?;
        let result = populate(&dir).and_then(|_| Wallpaper::load(&dir));
        if result.is_err() {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                log::warn!("remove incomplete import {}: {e}", dir.display());
            }
        }
        result
    }

    pub fn import(
        &self,
        source: &str,
        opts: &ImportOptions<'_>,
        progress: &mut dyn FnMut(&Wallpaper),
    ) -> Result<Imported> {
        ctx(std::fs::create_dir_all(&self.dir), self.dir.display())?;
        let mut out = Imported::default();
        let path = PathBuf::from(source);
        let is_url = source.starts_with("http://") || source.starts_with("https://");
        if !is_url && path.is_dir() {
            let root = ctx(std::fs::canonicalize(&path), path.display())?;
            let library = ctx(std::fs::canonicalize(&self.dir), self.dir.display())?;
            let mut seen = HashSet::new();
            if root != library {
                seen.insert(library);
            }
            self.import_tree(&root, opts, &mut seen, progress, &mut out);
            return match (out.wallpapers.is_empty(), out.problems.is_empty()) {
                (true, true) => Err(Error::Unsupported(format!(
                    "{} contains no wallpapers",
                    root.display()
                ))),
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

    fn import_file(&self, path: &Path, opts: &ImportOptions<'_>) -> Result<Wallpaper> {
        let ext = extension(path);
        if PACKAGE_EXTENSIONS.contains(&ext.as_str()) {
            return self.import_zip(path);
        }
        if file_name(path).eq_ignore_ascii_case(project::FILE_NAME) {
            let dir = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or_else(|| {
                    Error::Invalid(format!("{} has no parent folder", path.display()))
                })?;
            return self.import_project(dir, opts, None, None);
        }
        let kind = Kind::from_extension(&ext)
            .or_else(|| is_executable(path).then_some(Kind::Program))
            .ok_or_else(|| {
                Error::Unsupported(format!(
                    "{} is not a supported wallpaper format",
                    if ext.is_empty() {
                        path.display().to_string()
                    } else {
                        format!(".{ext} files")
                    }
                ))
            })?;
        let source_abs = std::path::absolute(path)?;
        if kind == Kind::Web {
            return self.import_web(&source_abs, opts.copy);
        }
        let title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Wallpaper".into());
        self.create(&title, |dir| {
            let mut info = Info {
                title: title.clone(),
                kind,
                ..Info::default()
            };
            if kind.is_media() && opts.copy {
                let name = file_name(&source_abs);
                ctx(
                    std::fs::copy(&source_abs, dir.join(&name)),
                    source_abs.display(),
                )?;
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
            info.save(&dir.join(FILE_NAME))
        })
    }

    fn import_tree(
        &self,
        dir: &Path,
        opts: &ImportOptions<'_>,
        seen: &mut HashSet<PathBuf>,
        progress: &mut dyn FnMut(&Wallpaper),
        out: &mut Imported,
    ) {
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
            out.record(
                self.import_lively_dir(dir)
                    .map_err(|e| Error::Invalid(format!("{}: {e}", dir.display()))),
                progress,
            );
            return;
        }
        if dir.join(project::FILE_NAME).is_file() {
            out.record(
                self.import_project(dir, opts, None, None)
                    .map_err(|e| Error::Invalid(format!("{}: {e}", dir.display()))),
                progress,
            );
            return;
        }
        if let Some(index) = find_index(dir) {
            out.record(
                self.import_web(&index, opts.copy)
                    .map_err(|e| Error::Invalid(format!("{}: {e}", dir.display()))),
                progress,
            );
            return;
        }
        let entries = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) => {
                out.problems.push(format!("{}: {e}", dir.display()));
                return;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(e) if !file_name(&e.path()).starts_with('.') => paths.push(e.path()),
                Ok(_) => {}
                Err(e) => out.problems.push(format!("{}: {e}", dir.display())),
            }
        }
        paths.sort();
        for p in paths {
            if p.is_dir() {
                self.import_tree(&p, opts, seen, progress, out);
            } else if p.is_file() {
                let ext = extension(&p);
                if PACKAGE_EXTENSIONS.contains(&ext.as_str())
                    || Kind::from_extension(&ext).is_some()
                {
                    out.record(
                        self.import_file(&p, opts)
                            .map_err(|e| Error::Invalid(format!("{}: {e}", p.display()))),
                        progress,
                    );
                }
            }
        }
    }

    fn import_url(&self, url: &str, want_thumbnail: bool) -> Result<Wallpaper> {
        let (kind, title, file_name) = match super::stream::probe(url) {
            Ok(Some(p)) => (
                Kind::VideoStream,
                if p.title.is_empty() {
                    url.to_string()
                } else {
                    p.title
                },
                url.to_string(),
            ),
            Ok(None) => (Kind::Url, host(url), url.to_string()),
            Err(Error::Unsupported(_)) => match super::stream::youtube_embed(url) {
                Some(embed) => (Kind::Url, host(url), embed),
                None => (Kind::Url, host(url), url.to_string()),
            },
            Err(e) => return Err(e),
        };
        self.create(&title, |dir| {
            let mut info = Info {
                title: title.clone(),
                kind,
                file_name,
                contact: Some(url.to_string()),
                is_absolute_path: true,
                ..Info::default()
            };
            if kind == Kind::VideoStream && want_thumbnail {
                match super::stream::thumbnail(url, dir) {
                    Ok(name) => info.thumbnail = Some(name),
                    Err(e) => log::warn!("stream thumbnail: {e}"),
                }
            }
            info.save(&dir.join(FILE_NAME))
        })
    }

    fn import_lively_dir(&self, path: &Path) -> Result<Wallpaper> {
        if path
            .parent()
            .is_some_and(|p| std::fs::canonicalize(&self.dir).is_ok_and(|library| p == library))
        {
            return Wallpaper::load(path);
        }
        let info = Info::load(&path.join(FILE_NAME))?;
        self.create(&info.title, |dir| copy_tree(path, dir))
    }

    fn import_web(&self, index: &Path, copy: bool) -> Result<Wallpaper> {
        let root = index
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| Error::Invalid(format!("{} has no parent folder", index.display())))?;
        if root.join(FILE_NAME).is_file() {
            return self.import_lively_dir(root);
        }
        let stem = index
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = if stem.eq_ignore_ascii_case("index") {
            file_name(root)
        } else {
            stem
        };
        self.create(&title, |dir| {
            let mut info = Info {
                title: title.clone(),
                kind: Kind::Web,
                ..Info::default()
            };
            if copy {
                copy_tree(root, dir)?;
                info.file_name = file_name(index);
            } else {
                info.file_name = index.to_string_lossy().into_owned();
                info.is_absolute_path = true;
            }
            info.save(&dir.join(FILE_NAME))
        })
    }

    fn import_zip(&self, path: &Path) -> Result<Wallpaper> {
        let file = ctx(std::fs::File::open(path), path.display())?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))?;
        let info_entry = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|n| n.rsplit('/').next() == Some(FILE_NAME))
            .min_by_key(|n| n.len())
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{} is not a Lively wallpaper package (no {FILE_NAME})",
                    path.display()
                ))
            })?;
        let prefix = info_entry.trim_end_matches(FILE_NAME).to_string();
        let mut info_text = String::new();
        archive
            .by_name(&info_entry)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .read_to_string(&mut info_text)?;
        let info: Info = serde_json::from_str(&info_text)?;
        let title = if info.title.trim().is_empty() {
            file_name(path)
        } else {
            info.title.clone()
        };
        self.create(&title, |dir| {
            for i in 0..archive.len() {
                let mut entry = archive
                    .by_index(i)
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                let name = entry.name().to_string();
                let Some(rel) = name.strip_prefix(&prefix) else {
                    continue;
                };
                if rel.is_empty() {
                    continue;
                }
                if entry.enclosed_name().is_none()
                    || rel.contains(['\\', ':'])
                    || Path::new(rel)
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
                {
                    return Err(Error::Invalid(format!(
                        "{} contains an unsafe path",
                        path.display()
                    )));
                }
                let target = dir.join(rel);
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
            Ok(())
        })
    }

    /// Import a Wallpaper Engine project folder: the one holding `project.json`. Packed
    /// scenes are unpacked into the entry; other content follows `opts.copy`. `origin` records
    /// the workshop item the folder came from and `author` its creator's name.
    pub fn import_project(
        &self,
        dir: &Path,
        opts: &ImportOptions<'_>,
        origin: Option<WorkshopOrigin>,
        author: Option<String>,
    ) -> Result<Wallpaper> {
        ctx(std::fs::create_dir_all(&self.dir), self.dir.display())?;
        let dir = ctx(std::fs::canonicalize(dir), dir.display())?;
        let project = Project::load(&dir.join(project::FILE_NAME))?;
        let content = dir.join(&project.file);
        if !content.is_file() {
            return Err(Error::NotFound(format!(
                "{} names {} but the file is missing",
                project::FILE_NAME,
                project.file
            )));
        }
        let kind = match project.kind {
            ProjectType::Scene => Kind::Scene,
            ProjectType::Video => Kind::Video,
            ProjectType::Web => Kind::Web,
            ProjectType::Application if cfg!(windows) => Kind::Program,
            ProjectType::Application => {
                return Err(Error::Unsupported(format!(
                    "'{}' is a Wallpaper Engine application wallpaper: a Windows program that runs only on Windows",
                    project.title
                )));
            }
        };
        let packed = project.is_packed_scene();
        let title = project.title.clone();
        let origin = origin.or_else(|| {
            crate::we::steam::item_at(&dir).map(|item| WorkshopOrigin {
                id: item.id,
                updated: item.updated,
                source: Some(item.dir),
            })
        });
        let workshop_id = origin.as_ref().map(|o| o.id).or(project.workshop_id);
        self.create(&title, |entry| {
            ctx(
                std::fs::copy(dir.join(project::FILE_NAME), entry.join(project::FILE_NAME))
                    .map(|_| ()),
                dir.display(),
            )?;
            let mut info = Info {
                title: title.clone(),
                kind,
                desc: project.description.clone(),
                author: author.clone().filter(|a| !a.trim().is_empty()),
                contact: workshop_id.map(project::workshop_url),
                id: workshop_id.map(|id| id.to_string()),
                tags: (!project.tags.is_empty()).then(|| project.tags.clone()),
                ..Info::default()
            };
            if let Some(preview) = project
                .preview
                .as_deref()
                .map(|p| dir.join(p))
                .filter(|p| p.is_file())
            {
                let name = file_name(&preview);
                ctx(
                    std::fs::copy(&preview, entry.join(&name)).map(|_| ()),
                    preview.display(),
                )?;
                info.thumbnail = Some(name);
            }
            if packed {
                let scene_dir = entry.join("scene");
                let mut pkg = crate::we::pkg::Package::open(&content)?;
                if !pkg.contains("scene.json") {
                    return Err(Error::Invalid(format!(
                        "{} holds no scene.json",
                        content.display()
                    )));
                }
                let scene = pkg.read_entry("scene.json")?;
                if let Err(e) = serde_json::from_slice::<serde_json::Value>(&scene) {
                    return Err(Error::Invalid(format!(
                        "{}: scene.json is not valid JSON: {e}",
                        content.display()
                    )));
                }
                log::info!(
                    "unpacking {} files from {}",
                    pkg.entries().len(),
                    content.display()
                );
                pkg.extract_to(&scene_dir)?;
                copy_loose_files(&dir, &content, &scene_dir)?;
                info.file_name = "scene/scene.json".into();
            } else if opts.copy {
                copy_tree(&dir, &entry.join("content"))?;
                info.file_name = format!("content/{}", project.file);
            } else {
                info.file_name = content.to_string_lossy().into_owned();
                info.is_absolute_path = true;
            }
            if kind == Kind::Video && opts.thumbnails && info.thumbnail.is_none() {
                match thumb::capture(&content, kind, &entry.join(THUMBNAIL), opts.temp_dir) {
                    Ok(()) => info.thumbnail = Some(THUMBNAIL.into()),
                    Err(e) => log::warn!("thumbnail for {}: {e}", content.display()),
                }
            }
            if let Some(o) = &origin {
                o.save(entry)?;
            }
            info.save(&entry.join(FILE_NAME))
        })
    }

    /// Put a freshly imported entry in place of entry `id`, keeping the id so layouts and
    /// property copies still point at it. The fresh entry's directory is consumed.
    pub fn replace(&self, id: &str, fresh: &Wallpaper) -> Result<Wallpaper> {
        let old = self.get(id)?;
        let backup = self.dir.join(format!("{id}.old-{}", nonce()));
        ctx(std::fs::rename(&old.dir, &backup), old.dir.display())?;
        if let Err(e) = std::fs::rename(&fresh.dir, &old.dir) {
            let _ = std::fs::rename(&backup, &old.dir);
            return ctx(Err(e), fresh.dir.display());
        }
        if let Err(e) = std::fs::remove_dir_all(&backup) {
            log::warn!("remove {}: {e}", backup.display());
        }
        Wallpaper::load(&old.dir)
    }

    /// The entry made from workshop item `id`, if any.
    pub fn find_workshop(&self, id: u64) -> Option<Wallpaper> {
        self.scan()
            .into_iter()
            .find(|w| WorkshopOrigin::load(&w.dir).is_some_and(|o| o.id == id))
    }

    /// Every entry's workshop origin, for keeping imports in step with Steam.
    pub fn workshop_entries(&self) -> Vec<(Wallpaper, WorkshopOrigin)> {
        self.scan()
            .into_iter()
            .filter_map(|w| WorkshopOrigin::load(&w.dir).map(|o| (w, o)))
            .collect()
    }

    /// Remove a wallpaper directory and its property copies. Only directories inside the
    /// library are deleted; referenced media outside it stays untouched.
    pub fn delete(&self, id: &str, properties_dir: &Path) -> Result<()> {
        let wp = self.get(id)?;
        let dir = std::path::absolute(&wp.dir)?;
        if dir.parent() != Some(std::path::absolute(&self.dir)?.as_path()) {
            return Err(Error::Invalid(format!(
                "{} is outside the library",
                dir.display()
            )));
        }
        ctx(std::fs::remove_dir_all(&dir), dir.display())?;
        let _ = std::fs::remove_dir_all(properties_dir.join(id));
        Ok(())
    }

    /// Write a Lively package: the wallpaper folder, plus referenced files when the wallpaper
    /// points outside the library, with paths rewritten relative.
    pub fn export(&self, wp: &Wallpaper, file: &Path) -> Result<()> {
        let library_entry = wp.dir.canonicalize()?;
        let parent = file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .canonicalize()?;
        let destination = file
            .canonicalize()
            .unwrap_or_else(|_| parent.join(file_name(file)));
        let inside_source = wp.info.is_absolute_path
            && !wp.kind().is_online()
            && if wp.kind().is_directory_project() {
                destination.starts_with(wp.root_dir().canonicalize()?)
            } else {
                destination == Path::new(&wp.source).canonicalize()?
            };
        if inside_source || destination.starts_with(&library_entry) {
            return Err(Error::Invalid(
                "export outside the wallpaper's source and library folders".into(),
            ));
        }
        let out = ctx(tempfile::NamedTempFile::new_in(&parent), file.display())?;
        let mut zip = zip::ZipWriter::new(out);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut info = wp.info.clone();
        add_tree(&mut zip, &wp.dir, "", &opts)?;
        if wp.info.is_absolute_path && !wp.kind().is_online() {
            let src = PathBuf::from(&wp.source);
            if wp.kind().is_directory_project() {
                let root = wp.root_dir();
                add_tree(&mut zip, &root, "content/", &opts)?;
                let rel = src
                    .strip_prefix(&root)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_else(|_| file_name(&src));
                info.file_name = format!("content/{rel}");
            } else if src.is_file() {
                let name = file_name(&src);
                zip.start_file(&name, opts)
                    .map_err(|e| Error::Io(std::io::Error::other(e)))?;
                let mut f = ctx(std::fs::File::open(&src), src.display())?;
                std::io::copy(&mut f, &mut zip)?;
                info.file_name = name;
            }
            info.is_absolute_path = false;
        }
        info.thumbnail = wp.thumbnail.as_ref().map(|t| file_name(t));
        zip.start_file(FILE_NAME, opts)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        zip.write_all(serde_json::to_string_pretty(&info)?.as_bytes())?;
        let out = zip
            .finish()
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        ctx(out.as_file().sync_all(), file.display())?;
        ctx(out.persist(file).map_err(|e| e.error), file.display())?;
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
    path.extension().is_some_and(|e| {
        e.eq_ignore_ascii_case("exe")
            || e.eq_ignore_ascii_case("bat")
            || e.eq_ignore_ascii_case("cmd")
    })
}

fn host(url: &str) -> String {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .trim_start_matches("www.")
        .to_string()
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn find_index(dir: &Path) -> Option<PathBuf> {
    ["index.html", "index.htm"]
        .into_iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// Files a packed scene keeps next to its package (anything but the project's own
/// bookkeeping) join the unpacked scene, without replacing what the package held.
fn copy_loose_files(dir: &Path, package: &Path, scene_dir: &Path) -> Result<()> {
    let skip = |p: &Path| {
        p == package
            || file_name(p).eq_ignore_ascii_case(project::FILE_NAME)
            || file_name(p).to_ascii_lowercase().starts_with("preview.")
    };
    walk_tree(dir, &mut HashSet::new(), &mut |src, directory| {
        if skip(src) {
            return Ok(());
        }
        let rel = src
            .strip_prefix(dir)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        let dst = scene_dir.join(rel);
        if directory {
            ctx(std::fs::create_dir_all(&dst), dst.display())
        } else if dst.exists() {
            Ok(())
        } else {
            if let Some(parent) = dst.parent() {
                ctx(std::fs::create_dir_all(parent), parent.display())?;
            }
            ctx(std::fs::copy(src, &dst).map(|_| ()), src.display())
        }
    })
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    let from = ctx(std::fs::canonicalize(from), from.display())?;
    ctx(std::fs::create_dir_all(to), to.display())?;
    let to = ctx(std::fs::canonicalize(to), to.display())?;
    if to.starts_with(&from) {
        return Err(Error::Invalid(
            "cannot copy a wallpaper into its own source folder".into(),
        ));
    }
    walk_tree(&from, &mut HashSet::new(), &mut |src, directory| {
        if directory && ctx(src.canonicalize(), src.display())? == to {
            return Err(Error::Invalid(
                "cannot copy a wallpaper into its own source folder".into(),
            ));
        }
        let dst = to.join(
            src.strip_prefix(&from)
                .map_err(|e| Error::Invalid(e.to_string()))?,
        );
        if directory {
            ctx(std::fs::create_dir_all(&dst), dst.display())
        } else {
            ctx(std::fs::copy(src, &dst).map(|_| ()), src.display())
        }
    })
}

fn add_tree<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    prefix: &str,
    opts: &zip::write::SimpleFileOptions,
) -> Result<()> {
    walk_tree(root, &mut HashSet::new(), &mut |path, directory| {
        let rel = path
            .strip_prefix(root)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        if directory || (prefix.is_empty() && rel == FILE_NAME) {
            return Ok(());
        }
        zip.start_file(format!("{prefix}{rel}"), *opts)
            .map_err(|e| Error::Io(std::io::Error::other(e)))?;
        let mut f = ctx(std::fs::File::open(path), path.display())?;
        std::io::copy(&mut f, zip)?;
        Ok(())
    })
}

fn walk_tree(
    dir: &Path,
    ancestors: &mut HashSet<PathBuf>,
    visit: &mut dyn FnMut(&Path, bool) -> Result<()>,
) -> Result<()> {
    let real = ctx(std::fs::canonicalize(dir), dir.display())?;
    if !ancestors.insert(real.clone()) {
        return Err(Error::Invalid(format!(
            "{} contains a directory cycle",
            dir.display()
        )));
    }
    for entry in ctx(std::fs::read_dir(dir), dir.display())? {
        let path = ctx(entry, dir.display())?.path();
        let metadata = ctx(std::fs::metadata(&path), path.display())?;
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(Error::Invalid(format!(
                "{} is not a regular file or directory",
                path.display()
            )));
        }
        visit(&path, metadata.is_dir())?;
        if metadata.is_dir() {
            walk_tree(&path, ancestors, visit)?;
        }
    }
    ancestors.remove(&real);
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

    fn one(lib: &Library, source: &Path, copy: bool, root: &Path) -> Result<Wallpaper> {
        let opts = ImportOptions {
            copy,
            thumbnails: false,
            temp_dir: root,
        };
        let mut imported = lib.import(source.to_str().unwrap(), &opts, &mut |_| {})?;
        assert_eq!(imported.wallpapers.len(), 1, "{}", source.display());
        assert!(imported.problems.is_empty(), "{:?}", imported.problems);
        Ok(imported.wallpapers.remove(0))
    }

    #[test]
    fn imports_media_by_reference_and_scans() {
        let root = temp();
        let lib = Library {
            dir: root.join("library"),
        };
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
        let lib = Library {
            dir: root.join("library"),
        };
        let site = root.join("site");
        std::fs::create_dir_all(site.join("assets")).unwrap();
        std::fs::write(site.join("index.html"), "<html></html>").unwrap();
        std::fs::write(site.join("assets/a.js"), "1").unwrap();
        std::fs::write(
            site.join("LivelyProperties.json"),
            r#"{"hue":{"type":"slider","text":"Hue","value":1,"min":0,"max":9}}"#,
        )
        .unwrap();
        let copied = one(&lib, &site, true, &root).unwrap();
        assert_eq!(copied.kind(), Kind::Web);
        assert!(!copied.info.is_absolute_path);
        assert!(copied.dir.join("assets/a.js").is_file());
        assert!(matches!(
            copied.properties,
            crate::model::wallpaper::PropertySource::File(_)
        ));

        let package = root.join("pkg.zip");
        lib.export(&copied, &package).unwrap();
        let other = Library {
            dir: root.join("other"),
        };
        let restored = one(&other, &package, false, &root).unwrap();
        assert_eq!(restored.title(), copied.title());
        assert!(restored.dir.join("index.html").is_file());
        assert!(restored.dir.join("assets/a.js").is_file());
        assert!(matches!(
            restored.properties,
            crate::model::wallpaper::PropertySource::File(_)
        ));

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
        let lib = Library {
            dir: root.join("library"),
        };
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
        std::fs::write(
            lively.join("LivelyInfo.json"),
            r#"{"Title":"Packaged","Type":2,"FileName":"index.html"}"#,
        )
        .unwrap();
        let via_html = one(&lib, &lively.join("index.html"), false, &root).unwrap();
        assert_eq!(via_html.kind(), Kind::WebAudio);
        assert_eq!(via_html.title(), "Packaged");
        assert!(via_html.dir.join("index.html").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_imports_everything_inside_flat_and_reports_problems() {
        let root = temp();
        let lib = Library {
            dir: root.join("library"),
        };
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
        std::fs::write(
            pack.join("lively/LivelyInfo.json"),
            r#"{"Title":"Packed","Type":7,"FileName":"clip.mp4"}"#,
        )
        .unwrap();
        std::fs::write(pack.join("lively/clip.mp4"), "v").unwrap();
        std::fs::write(pack.join(".hidden/d.mp4"), "v").unwrap();
        std::fs::write(pack.join("broken.zip"), b"PK\x03\x04junk").unwrap();

        let opts = ImportOptions {
            copy: false,
            thumbnails: false,
            temp_dir: &root,
        };
        let mut seen = Vec::new();
        let imported = lib
            .import(pack.to_str().unwrap(), &opts, &mut |w| seen.push(w.title()))
            .unwrap();
        let mut titles: Vec<String> = imported.wallpapers.iter().map(|w| w.title()).collect();
        titles.sort();
        assert_eq!(titles, ["Packed", "a", "b", "c", "site"]);
        assert_eq!(seen.len(), 5, "progress runs once per wallpaper");
        assert_eq!(imported.problems.len(), 1, "{:?}", imported.problems);
        assert!(imported.problems[0].contains("broken.zip"));
        assert_eq!(lib.scan().len(), 5);

        let nothing = root.join("nothing");
        std::fs::create_dir_all(&nothing).unwrap();
        assert!(matches!(
            lib.import(nothing.to_str().unwrap(), &opts, &mut |_| {}),
            Err(Error::Unsupported(_))
        ));
        let only_bad = root.join("only-bad");
        std::fs::create_dir_all(&only_bad).unwrap();
        std::fs::write(only_bad.join("x.zip"), b"PK\x03\x04junk").unwrap();
        assert!(matches!(
            lib.import(only_bad.to_str().unwrap(), &opts, &mut |_| {}),
            Err(Error::Invalid(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    fn we_project(dir: &Path, kind: &str, file: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("project.json"),
            format!(
                r#"{{"title":"Nebula","type":"{kind}","file":"{file}","preview":"preview.gif","tags":["Space"],"workshopid":"42",
                "general":{{"supportsaudioprocessing":true,"properties":{{"speed":{{"type":"slider","value":1,"min":0,"max":2,"text":"Speed","order":1}}}}}}}}"#
            ),
        )
        .unwrap();
        std::fs::write(dir.join("preview.gif"), b"GIF89a").unwrap();
    }

    #[test]
    fn imports_packed_scenes_and_video_projects() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            dir: root.path().join("library"),
        };
        let scene = root.path().join("123");
        we_project(&scene, "scene", "scene.pkg");
        let mut pkg = Vec::new();
        crate::we::pkg::write(
            &mut pkg,
            "PKGV0018",
            &[
                ("scene.json", br#"{"objects":[]}"#),
                ("materials/a.tex", b"TEXV"),
            ],
        )
        .unwrap();
        std::fs::write(scene.join("scene.pkg"), pkg).unwrap();
        std::fs::create_dir_all(scene.join("extras")).unwrap();
        std::fs::write(scene.join("extras/loose.json"), "{}").unwrap();
        let opts = ImportOptions {
            copy: false,
            thumbnails: false,
            temp_dir: root.path(),
        };
        let origin = WorkshopOrigin {
            id: 42,
            updated: Some(5),
            source: Some(scene.clone()),
        };
        let w = lib
            .import_project(&scene, &opts, Some(origin.clone()), Some("Ada".into()))
            .unwrap();
        assert_eq!(w.kind(), Kind::Scene);
        assert_eq!(w.title(), "Nebula");
        assert!(!w.info.is_absolute_path);
        assert!(w.dir.join("scene/scene.json").is_file());
        assert!(w.dir.join("scene/materials/a.tex").is_file());
        assert!(w.dir.join("scene/extras/loose.json").is_file());
        assert!(!w.dir.join("scene/scene.pkg").exists());
        assert_eq!(w.root_dir(), w.dir.join("scene"));
        assert_eq!(w.thumbnail, Some(w.dir.join("preview.gif")));
        assert_eq!(w.info.author.as_deref(), Some("Ada"));
        assert_eq!(w.info.id.as_deref(), Some("42"));
        assert!(w.we.as_ref().is_some_and(|p| p.audio));
        assert!(w.wants_audio());
        assert!(matches!(
            w.properties,
            crate::model::wallpaper::PropertySource::WallpaperEngine(_)
        ));
        assert_eq!(WorkshopOrigin::load(&w.dir), Some(origin));
        assert_eq!(lib.find_workshop(42).map(|f| f.id), Some(w.id.clone()));
        assert!(lib.find_workshop(43).is_none());
        let s = w.summary();
        assert_eq!(s.workshop, Some(42));
        assert_eq!(s.tags, ["Space"]);
        assert!(s.customizable);

        let video = root.path().join("video");
        we_project(&video, "video", "clip.mp4");
        std::fs::write(video.join("clip.mp4"), "v").unwrap();
        let v = lib.import_project(&video, &opts, None, None).unwrap();
        assert_eq!(v.kind(), Kind::Video);
        assert!(v.info.is_absolute_path);
        assert_eq!(
            PathBuf::from(&v.source),
            video.canonicalize().unwrap().join("clip.mp4")
        );
        assert!(matches!(
            v.properties,
            crate::model::wallpaper::PropertySource::BuiltinMedia
        ));
        assert_eq!(
            v.info.contact.as_deref(),
            Some("https://steamcommunity.com/sharedfiles/filedetails/?id=42")
        );

        let copied = lib
            .import_project(&video, &ImportOptions { copy: true, ..opts }, None, None)
            .unwrap();
        assert!(!copied.info.is_absolute_path);
        assert!(copied.dir.join("content/clip.mp4").is_file());

        let imported = lib
            .import(root.path().to_str().unwrap(), &opts, &mut |_| {})
            .unwrap();
        assert_eq!(imported.wallpapers.len(), 2, "{:?}", imported.problems);
        assert!(imported.problems.is_empty(), "{:?}", imported.problems);
        assert_eq!(lib.scan().len(), 5);
    }

    #[test]
    fn refuses_broken_projects() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            dir: root.path().join("library"),
        };
        let opts = ImportOptions {
            copy: false,
            thumbnails: false,
            temp_dir: root.path(),
        };
        let missing = root.path().join("missing");
        we_project(&missing, "scene", "scene.pkg");
        assert!(matches!(
            lib.import_project(&missing, &opts, None, None),
            Err(Error::NotFound(_))
        ));
        let bad = root.path().join("bad");
        we_project(&bad, "scene", "scene.pkg");
        std::fs::write(bad.join("scene.pkg"), b"nope").unwrap();
        assert!(lib.import_project(&bad, &opts, None, None).is_err());
        if !cfg!(windows) {
            let app = root.path().join("app");
            we_project(&app, "application", "wp.exe");
            std::fs::write(app.join("wp.exe"), b"MZ").unwrap();
            assert!(matches!(
                lib.import_project(&app, &opts, None, None),
                Err(Error::Unsupported(_))
            ));
        }
        assert_eq!(std::fs::read_dir(&lib.dir).unwrap().count(), 0);
    }

    #[test]
    fn rejects_escaping_archive_paths_and_removes_partial_imports() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            dir: root.path().join("library"),
        };
        for name in [
            "../escape",
            "/escape",
            "C:/escape",
            "..\\escape",
            "folder/../../escape",
            "folder\\..\\..\\escape",
        ] {
            let package = root.path().join("bad.zip");
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file(FILE_NAME, opts).unwrap();
            zip.write_all(br#"{"Title":"Bad","Type":7,"FileName":"clip.mp4"}"#)
                .unwrap();
            zip.start_file(name, opts).unwrap();
            zip.write_all(b"escape").unwrap();
            zip.finish().unwrap();
            assert!(one(&lib, &package, true, root.path()).is_err(), "{name}");
            assert_eq!(std::fs::read_dir(&lib.dir).unwrap().count(), 0, "{name}");
            assert!(!root.path().join("escape").exists());
        }
    }

    #[test]
    fn rejects_copying_a_project_into_itself() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            dir: root.path().join("library"),
        };
        std::fs::write(root.path().join("index.html"), "site").unwrap();
        assert!(one(&lib, root.path(), true, root.path()).is_err());
        assert_eq!(std::fs::read_dir(&lib.dir).unwrap().count(), 0);
    }

    #[test]
    fn recursive_import_skips_its_library() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            dir: root.path().join("library"),
        };
        std::fs::write(root.path().join("clip.mp4"), "video").unwrap();
        one(&lib, root.path(), true, root.path()).unwrap();
        assert_eq!(lib.scan().len(), 1);
        one(&lib, root.path(), true, root.path()).unwrap();
        assert_eq!(lib.scan().len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn directory_cycles_fail_without_overwriting_an_export() {
        let root = tempfile::tempdir().unwrap();
        let lib = Library {
            dir: root.path().join("library"),
        };
        let site = root.path().join("site");
        std::fs::create_dir(&site).unwrap();
        std::fs::write(site.join("index.html"), "site").unwrap();
        std::os::unix::fs::symlink(&site, site.join("loop")).unwrap();
        assert!(one(&lib, &site, true, root.path()).is_err());
        assert_eq!(std::fs::read_dir(&lib.dir).unwrap().count(), 0);
        let wp = one(&lib, &site, false, root.path()).unwrap();
        let package = root.path().join("existing.zip");
        std::fs::write(&package, "keep this").unwrap();
        assert!(lib.export(&wp, &package).is_err());
        assert_eq!(std::fs::read_to_string(package).unwrap(), "keep this");
    }

    #[test]
    fn rejects_unsupported_and_unsafe_input() {
        let root = temp();
        let lib = Library {
            dir: root.join("library"),
        };
        let odd = root.join("notes.txt");
        std::fs::write(&odd, "x").unwrap();
        assert!(matches!(
            one(&lib, &odd, false, &root),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            one(&lib, &root.join("missing.mp4"), false, &root),
            Err(Error::NotFound(_))
        ));
        let bad_zip = root.join("bad.zip");
        std::fs::write(&bad_zip, b"PK\x03\x04junk").unwrap();
        assert!(one(&lib, &bad_zip, false, &root).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
