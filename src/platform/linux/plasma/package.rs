//! The wallpaper package plasmashell loads, embedded in the binary and installed into the
//! user's Plasma wallpaper directory. plasmashell caches QML components by URL for as long
//! as it runs, so `main.qml` is a permanent trampoline and the implementation files live in a
//! directory named after their content hash: every build gets fresh URLs.

use crate::error::{Error, Result, ctx};
use std::path::PathBuf;

pub const ID: &str = "org.deadlywp.live";

/// Files whose URLs plasmashell may cache: installed once and never rewritten in place.
const STABLE: &[(&str, &[u8])] = &[
    ("metadata.json", include_bytes!("../../../../assets/plasma/metadata.json")),
    ("contents/config/main.xml", include_bytes!("../../../../assets/plasma/contents/config/main.xml")),
    ("contents/ui/main.qml", include_bytes!("../../../../assets/plasma/contents/ui/main.qml")),
    ("contents/ui/config.qml", include_bytes!("../../../../assets/plasma/contents/ui/config.qml")),
];

/// The implementation, installed under `contents/<impl dir>/`.
const IMPL: &[(&str, &[u8])] = &[
    ("Wallpaper.qml", include_bytes!("../../../../assets/plasma/contents/impl/Wallpaper.qml")),
    ("Media.qml", include_bytes!("../../../../assets/plasma/contents/impl/Media.qml")),
    ("Web.qml", include_bytes!("../../../../assets/plasma/contents/impl/Web.qml")),
    ("adjust.frag", include_bytes!("../../../../assets/plasma/contents/impl/adjust.frag")),
    ("adjust.frag.qsb", include_bytes!("../../../../assets/plasma/contents/impl/adjust.frag.qsb")),
];

pub fn dir() -> Result<PathBuf> {
    dirs::data_dir().map(|d| d.join("plasma").join("wallpapers").join(ID)).ok_or_else(|| Error::Platform("no data directory for this user".into()))
}

/// Name of the implementation directory: FNV-1a over every implementation file.
pub fn impl_name() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (name, bytes) in IMPL {
        for b in name.bytes().chain(bytes.iter().copied()).chain(std::iter::once(0)) {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("impl-{hash:016x}")
}

fn write_if_changed(path: &PathBuf, bytes: &[u8]) -> Result<bool> {
    if std::fs::read(path).is_ok_and(|current| current == bytes) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        ctx(std::fs::create_dir_all(parent), parent.display())?;
    }
    ctx(std::fs::write(path, bytes), path.display())?;
    Ok(true)
}

/// Install the package and the current implementation; returns the implementation directory
/// name to write into the `Impl` configuration key. Older implementation directories are
/// removed.
pub fn install() -> Result<String> {
    let root = dir()?;
    let name = impl_name();
    let mut changed = false;
    for (rel, bytes) in STABLE {
        changed |= write_if_changed(&root.join(rel), bytes)?;
    }
    let impl_dir = root.join("contents").join(&name);
    for (rel, bytes) in IMPL {
        changed |= write_if_changed(&impl_dir.join(rel), bytes)?;
    }
    if let Ok(entries) = std::fs::read_dir(root.join("contents")) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let stale = file_name.to_string_lossy();
            if stale.starts_with("impl-") && *stale != *name {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
    if changed {
        log::info!("installed the Plasma wallpaper package at {} ({name})", root.display());
    }
    Ok(name)
}
