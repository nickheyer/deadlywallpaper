//! Where Steam keeps Wallpaper Engine: its install (the `assets` folder scene wallpapers draw
//! from) and the workshop items the user subscribed to.

use crate::we::vdf;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Wallpaper Engine's Steam application id.
pub const APP_ID: u64 = 431960;

/// Folder name of the Wallpaper Engine install inside `steamapps/common`.
const INSTALL_DIR: &str = "wallpaper_engine";

/// What was found on this machine.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SteamInfo {
    /// The Steam client's own folder, when one was found.
    pub steam_dir: Option<PathBuf>,
    /// Every Steam library folder (the client folder plus `libraryfolders.vdf` entries).
    pub libraries: Vec<PathBuf>,
    /// `steamapps/common/wallpaper_engine`, when Wallpaper Engine is installed.
    pub install_dir: Option<PathBuf>,
    /// The assets folder scene wallpapers need; the settings override or the install's own.
    pub assets_dir: Option<PathBuf>,
    /// Whether `assets_dir` came from the settings rather than the Steam install.
    pub assets_overridden: bool,
    /// Existing `steamapps/workshop/content/431960` folders.
    pub workshop_dirs: Vec<PathBuf>,
}

/// A workshop item Steam has downloaded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledItem {
    pub id: u64,
    pub dir: PathBuf,
    /// `timeupdated` from Steam's workshop manifest, when the item is listed there.
    pub updated: Option<u64>,
}

/// Locate Steam and Wallpaper Engine, honouring the settings' overrides.
pub fn locate(steam_override: Option<&Path>, assets_override: Option<&Path>) -> SteamInfo {
    let steam_dir = steam_override
        .map(Path::to_path_buf)
        .filter(|p| p.is_dir())
        .or_else(|| candidates().into_iter().find(|p| is_steam_dir(p)));
    let libraries = steam_dir
        .as_deref()
        .map(libraries)
        .unwrap_or_default();
    let install_dir = libraries
        .iter()
        .map(|lib| lib.join("steamapps").join("common").join(INSTALL_DIR))
        .find(|p| p.is_dir());
    let (assets_dir, assets_overridden) = match assets_override.filter(|p| p.is_dir()) {
        Some(p) => (Some(p.to_path_buf()), true),
        None => (
            install_dir
                .as_ref()
                .map(|p| p.join("assets"))
                .filter(|p| is_assets_dir(p)),
            false,
        ),
    };
    let workshop_dirs = libraries
        .iter()
        .map(|lib| workshop_content_dir(lib))
        .filter(|p| p.is_dir())
        .collect();
    SteamInfo {
        steam_dir,
        libraries,
        install_dir,
        assets_dir,
        assets_overridden,
        workshop_dirs,
    }
}

/// `steamapps/workshop/content/431960` inside a library folder.
pub fn workshop_content_dir(library: &Path) -> PathBuf {
    library
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join(APP_ID.to_string())
}

/// Steam's manifest of downloaded workshop items for a library folder.
fn workshop_manifest(library: &Path) -> PathBuf {
    library
        .join("steamapps")
        .join("workshop")
        .join(format!("appworkshop_{APP_ID}.acf"))
}

/// The assets folder is recognizable by the shader library every scene depends on.
pub fn is_assets_dir(dir: &Path) -> bool {
    dir.join("shaders").is_dir() && dir.join("effects").is_dir()
}

fn is_steam_dir(dir: &Path) -> bool {
    dir.join("steamapps").is_dir() || dir.join("SteamApps").is_dir()
}

/// `steamapps` with the case Steam used on this filesystem.
fn steamapps(dir: &Path) -> PathBuf {
    let lower = dir.join("steamapps");
    if lower.is_dir() {
        lower
    } else {
        dir.join("SteamApps")
    }
}

/// Library folders, the client's own first, without duplicates.
pub fn libraries(steam_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut push = |p: PathBuf| {
        let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if p.is_dir() && seen.insert(key) {
            out.push(p);
        }
    };
    push(steam_dir.to_path_buf());
    let vdf_path = steamapps(steam_dir).join("libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(&vdf_path) {
        match vdf::parse(&text) {
            Ok(root) => {
                let folders = root
                    .table("libraryfolders")
                    .cloned()
                    .unwrap_or_default();
                for (key, node) in folders.iter() {
                    match node {
                        vdf::Node::Table(t) => {
                            if let Some(p) = t.value("path") {
                                push(PathBuf::from(p));
                            }
                        }
                        // Steam clients before 2021 wrote "1" "path" pairs.
                        vdf::Node::Value(p) if key.parse::<u32>().is_ok() => {
                            push(PathBuf::from(p));
                        }
                        vdf::Node::Value(_) => {}
                    }
                }
            }
            Err(e) => log::warn!("{}: {e}", vdf_path.display()),
        }
    }
    out
}

/// Every workshop item Steam has downloaded, across all libraries.
pub fn installed(info: &SteamInfo) -> Vec<InstalledItem> {
    let mut out = Vec::new();
    for lib in &info.libraries {
        let content = workshop_content_dir(lib);
        let Ok(entries) = std::fs::read_dir(&content) else {
            continue;
        };
        let manifest = manifest_times(lib);
        let mut items: Vec<InstalledItem> = entries
            .flatten()
            .filter_map(|e| {
                let id: u64 = e.file_name().to_str()?.parse().ok()?;
                let dir = e.path();
                dir.join("project.json").is_file().then(|| InstalledItem {
                    id,
                    updated: manifest.iter().find(|(i, _)| *i == id).map(|(_, t)| *t),
                    dir,
                })
            })
            .collect();
        items.sort_by_key(|i| i.id);
        out.extend(items);
    }
    out
}

/// One downloaded item by id, searching every library.
pub fn installed_item(info: &SteamInfo, id: u64) -> Option<InstalledItem> {
    installed(info).into_iter().find(|i| i.id == id)
}

/// The workshop item a folder is, when it sits in a Steam library's
/// `steamapps/workshop/content/431960/<id>`.
pub fn item_at(dir: &Path) -> Option<InstalledItem> {
    let id: u64 = dir.file_name()?.to_str()?.parse().ok()?;
    let app = dir.parent()?;
    let content = app.parent()?;
    let workshop = content.parent()?;
    let steamapps = workshop.parent()?;
    let names = [
        app.file_name()?.to_str()?,
        content.file_name()?.to_str()?,
        workshop.file_name()?.to_str()?,
        steamapps.file_name()?.to_str()?,
    ];
    if names != [APP_ID.to_string().as_str(), "content", "workshop", "steamapps"] {
        return None;
    }
    let library = steamapps.parent()?;
    let updated = manifest_times(library)
        .into_iter()
        .find(|(i, _)| *i == id)
        .map(|(_, t)| t);
    Some(InstalledItem {
        id,
        dir: dir.to_path_buf(),
        updated,
    })
}

/// `(id, timeupdated)` of every item Steam's manifest lists as installed.
fn manifest_times(library: &Path) -> Vec<(u64, u64)> {
    let path = workshop_manifest(library);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let root = match vdf::parse(&text) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            return Vec::new();
        }
    };
    root.table("AppWorkshop")
        .and_then(|a| a.table("WorkshopItemsInstalled"))
        .map(|items| {
            items
                .tables()
                .filter_map(|(id, t)| Some((id.parse().ok()?, t.u64("timeupdated")?)))
                .collect()
        })
        .unwrap_or_default()
}

/// Steam client folders to try, most likely first.
fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        if let Some(p) = windows_registry_path() {
            out.push(p);
        }
        for var in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base).join("Steam"));
            }
        }
    }
    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "linux")]
        {
            for rel in [
                ".steam/steam",
                ".local/share/Steam",
                ".steam/root",
                ".steam/debian-installation",
                ".var/app/com.valvesoftware.Steam/.local/share/Steam",
                ".var/app/com.valvesoftware.Steam/.steam/steam",
                "snap/steam/common/.local/share/Steam",
            ] {
                out.push(home.join(rel));
            }
        }
        #[cfg(target_os = "macos")]
        {
            out.push(home.join("Library/Application Support/Steam"));
        }
        #[cfg(windows)]
        {
            let _ = &home;
        }
    }
    out
}

#[cfg(windows)]
fn windows_registry_path() -> Option<PathBuf> {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::PCWSTR;
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    for (root, key, value) in [
        (HKEY_CURRENT_USER, "Software\\Valve\\Steam", "SteamPath"),
        (
            HKEY_LOCAL_MACHINE,
            "SOFTWARE\\WOW6432Node\\Valve\\Steam",
            "InstallPath",
        ),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Valve\\Steam", "InstallPath"),
    ] {
        let (key, value) = (wide(key), wide(value));
        let mut buf = vec![0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: the buffers outlive the call and `len` holds the byte capacity of `buf`.
        let status = unsafe {
            RegGetValueW(
                root,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if status.is_ok() {
            let chars = (len as usize / 2).saturating_sub(1).min(buf.len());
            let path = PathBuf::from(String::from_utf16_lossy(&buf[..chars]).replace('/', "\\"));
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_with(root: &Path, id: u64, updated: Option<u64>) -> PathBuf {
        let lib = root.join("lib");
        let item = workshop_content_dir(&lib).join(id.to_string());
        std::fs::create_dir_all(&item).unwrap();
        std::fs::write(
            item.join("project.json"),
            r#"{"title":"T","type":"video","file":"v.mp4"}"#,
        )
        .unwrap();
        if let Some(t) = updated {
            std::fs::write(
                workshop_manifest(&lib),
                format!(
                    "\"AppWorkshop\"\n{{\n\t\"appid\"\t\"431960\"\n\t\"WorkshopItemsInstalled\"\n\t{{\n\t\t\"{id}\"\n\t\t{{\n\t\t\t\"size\"\t\"10\"\n\t\t\t\"timeupdated\"\t\"{t}\"\n\t\t\t\"manifest\"\t\"1\"\n\t\t}}\n\t}}\n}}\n"
                ),
            )
            .unwrap();
        }
        lib
    }

    #[test]
    fn finds_libraries_install_and_workshop_items() {
        let root = tempfile::tempdir().unwrap();
        let steam = root.path().join("steam");
        std::fs::create_dir_all(steam.join("steamapps")).unwrap();
        let lib = library_with(root.path(), 123, Some(1700000000));
        let install = lib.join("steamapps/common").join(INSTALL_DIR).join("assets");
        std::fs::create_dir_all(install.join("shaders")).unwrap();
        std::fs::create_dir_all(install.join("effects")).unwrap();
        std::fs::write(
            steam.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n\t\"2\"\n\t{{\n\t\t\"path\"\t\"{}\"\n\t}}\n}}\n",
                steam.display(),
                lib.display(),
                root.path().join("missing").display()
            ),
        )
        .unwrap();
        let info = locate(Some(&steam), None);
        assert_eq!(info.steam_dir.as_deref(), Some(steam.as_path()));
        assert_eq!(info.libraries, vec![steam.clone(), lib.clone()]);
        assert_eq!(
            info.install_dir.as_deref(),
            Some(lib.join("steamapps/common").join(INSTALL_DIR).as_path())
        );
        assert_eq!(info.assets_dir.as_deref(), Some(install.as_path()));
        assert!(!info.assets_overridden);
        assert_eq!(info.workshop_dirs, vec![workshop_content_dir(&lib)]);
        let items = installed(&info);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 123);
        assert_eq!(items[0].updated, Some(1700000000));
        assert_eq!(items[0].dir, workshop_content_dir(&lib).join("123"));
        assert_eq!(installed_item(&info, 123).map(|i| i.id), Some(123));
        assert_eq!(installed_item(&info, 9), None);
        let at = item_at(&items[0].dir).expect("a workshop folder");
        assert_eq!((at.id, at.updated), (123, Some(1700000000)));
        assert_eq!(item_at(&root.path().join("elsewhere").join("123")), None);
        assert_eq!(item_at(&lib.join("steamapps/workshop/content/570/123")), None);
    }

    #[test]
    fn overrides_take_precedence_and_missing_paths_are_ignored() {
        let root = tempfile::tempdir().unwrap();
        let assets = root.path().join("my-assets");
        std::fs::create_dir_all(&assets).unwrap();
        let info = locate(Some(&root.path().join("nope")), Some(&assets));
        assert_eq!(info.assets_dir.as_deref(), Some(assets.as_path()));
        assert!(info.assets_overridden);
        assert!(info.install_dir.is_none());
        let none = locate(Some(&root.path().join("nope")), Some(&root.path().join("gone")));
        assert!(none.assets_dir.is_none() || !none.assets_overridden);
    }

    #[test]
    fn items_without_a_manifest_entry_still_count() {
        let root = tempfile::tempdir().unwrap();
        let lib = library_with(root.path(), 7, None);
        std::fs::create_dir_all(workshop_content_dir(&lib).join("junk")).unwrap();
        std::fs::create_dir_all(workshop_content_dir(&lib).join("8")).unwrap();
        let info = SteamInfo {
            libraries: vec![lib],
            ..SteamInfo::default()
        };
        let items = installed(&info);
        assert_eq!(items.len(), 1, "only folders holding project.json are items");
        assert_eq!(items[0].id, 7);
        assert_eq!(items[0].updated, None);
    }
}
