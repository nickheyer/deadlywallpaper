//! Wallpaper Engine `scene.pkg` archives: a flat table of named files followed by their bytes.
//!
//! Layout, little endian throughout:
//!
//! ```text
//! u32 len, len bytes        version string, "PKGV0001" .. "PKGV0018"
//! u32 count
//! count × { u32 len, len bytes name; u32 offset; u32 size }
//! data                      every entry's bytes, `offset` relative to the end of the table
//! ```

use crate::error::{Error, Result, ctx};
use std::io::{Read, Seek, SeekFrom};
#[cfg(test)]
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const MAX_NAME: u32 = 4096;
const MAX_ENTRIES: u32 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Forward-slash separated path inside the package.
    pub name: String,
    pub offset: u64,
    pub size: u64,
}

pub struct Package<R> {
    reader: R,
    entries: Vec<Entry>,
    data_start: u64,
}

impl Package<std::fs::File> {
    pub fn open(path: &Path) -> Result<Package<std::fs::File>> {
        let file = ctx(std::fs::File::open(path), path.display())?;
        Package::read(file).map_err(|e| Error::Invalid(format!("{}: {e}", path.display())))
    }
}

impl<R: Read + Seek> Package<R> {
    pub fn read(mut reader: R) -> Result<Package<R>> {
        let version = read_string(&mut reader)?;
        if !version.starts_with("PKGV") {
            return Err(Error::Invalid(format!(
                "not a Wallpaper Engine package (magic '{version}')"
            )));
        }
        let count = read_u32(&mut reader)?;
        if count > MAX_ENTRIES {
            return Err(Error::Invalid(format!(
                "package claims {count} entries"
            )));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name = read_string(&mut reader)?;
            let offset = read_u32(&mut reader)? as u64;
            let size = read_u32(&mut reader)? as u64;
            entries.push(Entry { name, offset, size });
        }
        let data_start = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        for e in &entries {
            if data_start + e.offset + e.size > end {
                return Err(Error::Invalid(format!(
                    "entry '{}' runs past the end of the package",
                    e.name
                )));
            }
        }
        Ok(Package {
            reader,
            entries,
            data_start,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    /// The bytes of one entry.
    pub fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .iter()
            .find(|e| e.name == name)
            .cloned()
            .ok_or_else(|| Error::NotFound(format!("package has no '{name}'")))?;
        self.reader
            .seek(SeekFrom::Start(self.data_start + entry.offset))?;
        let mut buf = vec![0u8; entry.size as usize];
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Write every entry under `dir`, rejecting names that would leave it.
    pub fn extract_to(&mut self, dir: &Path) -> Result<()> {
        ctx(std::fs::create_dir_all(dir), dir.display())?;
        let entries = self.entries.clone();
        for entry in &entries {
            let rel = safe_relative(&entry.name)?;
            let target = dir.join(&rel);
            if let Some(parent) = target.parent() {
                ctx(std::fs::create_dir_all(parent), parent.display())?;
            }
            self.reader
                .seek(SeekFrom::Start(self.data_start + entry.offset))?;
            let mut out = ctx(std::fs::File::create(&target), target.display())?;
            let mut limited = (&mut self.reader).take(entry.size);
            ctx(std::io::copy(&mut limited, &mut out), target.display())?;
        }
        Ok(())
    }
}

/// A package name as a relative path that stays inside the extraction folder.
pub fn safe_relative(name: &str) -> Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.is_empty()
        || normalized.contains(['\0', ':'])
        || normalized.starts_with('/')
        || Path::new(&normalized)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(Error::Invalid(format!(
            "package entry '{name}' has an unsafe path"
        )));
    }
    Ok(PathBuf::from(normalized))
}

fn read_u32<R: Read>(r: &mut R) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_string<R: Read>(r: &mut R) -> Result<String> {
    let len = read_u32(r)?;
    if len > MAX_NAME {
        return Err(Error::Invalid(format!("string of {len} bytes")));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| Error::Invalid("string is not UTF-8".into()))
}

/// Write a package; the inverse of [`Package::read`], used by tests and by exports.
#[cfg(test)]
pub fn write<W: Write>(mut out: W, version: &str, files: &[(&str, &[u8])]) -> Result<()> {
    let put_string = |out: &mut W, s: &str| -> Result<()> {
        out.write_all(&(s.len() as u32).to_le_bytes())?;
        out.write_all(s.as_bytes())?;
        Ok(())
    };
    put_string(&mut out, version)?;
    out.write_all(&(files.len() as u32).to_le_bytes())?;
    let mut offset = 0u32;
    for (name, data) in files {
        put_string(&mut out, name)?;
        out.write_all(&offset.to_le_bytes())?;
        out.write_all(&(data.len() as u32).to_le_bytes())?;
        offset += data.len() as u32;
    }
    for (_, data) in files {
        out.write_all(data)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample() -> Vec<u8> {
        let mut buf = Vec::new();
        write(
            &mut buf,
            "PKGV0018",
            &[
                ("scene.json", br#"{"objects":[]}"#),
                ("materials/a.json", b"{}"),
                ("materials/a.tex", &[0u8, 1, 2, 3, 4, 5, 6, 7]),
            ],
        )
        .unwrap();
        buf
    }

    #[test]
    fn reads_what_it_writes() {
        let mut pkg = Package::read(Cursor::new(sample())).unwrap();
        let names: Vec<&str> = pkg.entries().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["scene.json", "materials/a.json", "materials/a.tex"]);
        assert_eq!(pkg.read_entry("materials/a.tex").unwrap(), [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(pkg.read_entry("scene.json").unwrap(), br#"{"objects":[]}"#);
        assert!(pkg.read_entry("missing").is_err());
        assert!(pkg.contains("materials/a.json"));
        let dir = tempfile::tempdir().unwrap();
        pkg.extract_to(dir.path()).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("materials/a.json")).unwrap(),
            b"{}"
        );
        assert_eq!(
            std::fs::read(dir.path().join("materials/a.tex")).unwrap().len(),
            8
        );
    }

    #[test]
    fn rejects_bad_magic_truncation_and_escapes() {
        assert!(Package::read(Cursor::new(b"\x04\x00\x00\x00ZIPX".to_vec())).is_err());
        let mut truncated = sample();
        truncated.truncate(truncated.len() - 3);
        assert!(Package::read(Cursor::new(truncated)).is_err());
        let mut buf = Vec::new();
        write(&mut buf, "PKGV0001", &[("../escape", b"x")]).unwrap();
        let mut pkg = Package::read(Cursor::new(buf)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(pkg.extract_to(dir.path()).is_err());
        assert!(!dir.path().parent().unwrap().join("escape").exists());
        for name in ["/abs", "C:/x", "a/../../b", "", "nul\0"] {
            assert!(safe_relative(name).is_err(), "{name}");
        }
        assert_eq!(
            safe_relative("materials\\x.tex").unwrap(),
            PathBuf::from("materials/x.tex")
        );
    }
}
