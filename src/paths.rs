use crate::error::{Error, Result, ctx};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "deadlywp";
pub const APP_NAME: &str = "Deadly Wallpaper";

#[derive(Clone, Debug)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Result<Paths> {
        let base = |d: Option<PathBuf>, what: &str| -> Result<PathBuf> {
            d.map(|p| p.join(APP_ID))
                .ok_or_else(|| Error::Platform(format!("no {what} directory for this user")))
        };
        let paths = Paths {
            config_dir: base(dirs::config_dir(), "config")?,
            data_dir: base(dirs::data_dir(), "data")?,
            cache_dir: base(dirs::cache_dir(), "cache")?,
        };
        for d in [
            &paths.config_dir,
            &paths.data_dir,
            &paths.cache_dir,
            &paths.temp_dir(),
            &paths.properties_dir(),
        ] {
            ctx(std::fs::create_dir_all(d), d.display())?;
        }
        Ok(paths)
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    pub fn layout_file(&self) -> PathBuf {
        self.config_dir.join("layout.json")
    }

    pub fn log_file(&self) -> PathBuf {
        self.cache_dir.join("deadlywp.log")
    }

    pub fn default_library_dir(&self) -> PathBuf {
        self.data_dir.join("library")
    }

    /// Per-slot copies of LivelyProperties.json live here: `<wallpaper>/<slot>.json`.
    pub fn properties_dir(&self) -> PathBuf {
        self.data_dir.join("properties")
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.cache_dir.join("tmp")
    }
}

/// Name of the daemon's local socket, unique per user.
pub fn socket_path() -> PathBuf {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "user".into());
        PathBuf::from(format!("deadlywp-{user}"))
    }
    #[cfg(not(windows))]
    {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        dir.join(format!("deadlywp-{}.sock", uid()))
    }
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Replace a file only after its complete contents have been written.
pub fn write(path: &Path, contents: impl AsRef<[u8]>) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = ctx(tempfile::NamedTempFile::new_in(parent), path.display())?;
    ctx(file.write_all(contents.as_ref()), path.display())?;
    ctx(file.as_file().sync_all(), path.display())?;
    ctx(
        file.persist(path).map(|_| ()).map_err(|e| e.error),
        path.display(),
    )
}

/// Filesystem-safe, ASCII-only slug of a title, used for library directory names.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "wallpaper".into()
    } else {
        out
    }
}

/// Hand a URL or path to the desktop's default handler.
pub fn open_external(target: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    let program = "xdg-open";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(windows)]
    let program = "explorer";
    std::process::Command::new(program)
        .arg(target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| Error::Platform(format!("open {target}: {e}")))
}

/// Random suffix for temporary files and library entries.
pub fn nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::hash::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0),
    );
    h.write_u32(std::process::id());
    format!("{:06x}", h.finish() & 0xff_ffff)
}
