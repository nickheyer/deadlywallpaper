//! System fonts for the scripts the bundled fonts lack: Chinese, Japanese, Korean, Arabic,
//! Hebrew, Thai, the Indic scripts and more. The desktop's own face for each language is
//! appended to egui's font families, so Latin text keeps the bundled look and everything else
//! falls back to the same faces the rest of the desktop shows.

use crate::error::ctx;
#[cfg(any(target_os = "linux", test))]
use crate::error::{Error, Result};
use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// One face on disk: the file and the face index inside it, since collections hold several.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Face {
    pub file: PathBuf,
    pub index: u32,
}

/// Replace the context's fonts with the bundled set plus every system face found.
pub fn install(context: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut files: HashMap<PathBuf, &'static [u8]> = HashMap::new();
    let mut loaded = Vec::new();
    for face in system_faces() {
        let bytes = match files.get(&face.file) {
            Some(bytes) => *bytes,
            None => match ctx(std::fs::read(&face.file), face.file.display()) {
                Ok(bytes) => {
                    // Fonts live as long as the process; the leak lets every face of a
                    // collection share one copy of the file.
                    let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                    files.insert(face.file.clone(), bytes);
                    bytes
                }
                Err(e) => {
                    log::warn!("{e}");
                    continue;
                }
            },
        };
        if let Err(e) = ab_glyph::FontRef::try_from_slice_and_index(bytes, face.index) {
            log::warn!("font {} face {}: {e}", face.file.display(), face.index);
            continue;
        }
        let name = format!("{}#{}", face.file.display(), face.index);
        let mut data = FontData::from_static(bytes);
        data.index = face.index;
        defs.font_data.insert(name.clone(), Arc::new(data));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            defs.families.entry(family).or_default().push(name.clone());
        }
        loaded.push(name);
    }
    log::info!("system fallback fonts: {}", loaded.join(", "));
    context.set_fonts(defs);
}

/// Languages whose scripts the bundled fonts do not cover, as fontconfig language tags.
#[cfg(target_os = "linux")]
const LANGUAGES: &[&str] = &[
    "zh-cn", "zh-tw", "zh-hk", "ja", "ko", "ar", "fa", "he", "th", "vi", "hi", "bn", "ta", "te",
    "ka", "hy", "am", "km", "my",
];

/// The faces fontconfig picks for each language, in language order, without repeats.
#[cfg(target_os = "linux")]
fn system_faces() -> Vec<Face> {
    use std::process::{Command, Stdio};
    let children: Vec<_> = LANGUAGES
        .iter()
        .map(|lang| {
            let child = Command::new("fc-match")
                .arg("-f")
                .arg("%{file}\n%{index}\n")
                .arg(format!("sans-serif:lang={lang}"))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn();
            (lang, child)
        })
        .collect();
    let mut faces = Vec::new();
    for (lang, child) in children {
        let face = child
            .map_err(|e| Error::Platform(format!("fc-match: {e}")))
            .and_then(|c| {
                c.wait_with_output()
                    .map_err(|e| Error::Platform(format!("fc-match: {e}")))
            })
            .and_then(|out| {
                if out.status.success() {
                    parse_match(&String::from_utf8_lossy(&out.stdout))
                } else {
                    Err(Error::Platform(format!(
                        "fc-match for {lang}: {}",
                        String::from_utf8_lossy(&out.stderr).trim()
                    )))
                }
            });
        match face {
            Ok(face) => {
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
            Err(e) => log::warn!("{e}"),
        }
    }
    faces
}

/// The file and index lines `fc-match -f "%{file}\n%{index}\n"` prints.
#[cfg(any(target_os = "linux", test))]
fn parse_match(output: &str) -> Result<Face> {
    let mut lines = output.lines();
    let file = lines
        .next()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .ok_or_else(|| Error::Platform("fc-match printed no file".into()))?;
    let index = lines
        .next()
        .map(str::trim)
        .unwrap_or("0")
        .parse()
        .map_err(|e| Error::Platform(format!("fc-match index: {e}")))?;
    Ok(Face {
        file: PathBuf::from(file),
        index,
    })
}

/// Windows' own fonts for each script, under the Fonts folder of the Windows directory.
#[cfg(windows)]
fn system_faces() -> Vec<Face> {
    const FILES: &[&str] = &[
        "msyh.ttc",     // Microsoft YaHei: Simplified Chinese
        "msjh.ttc",     // Microsoft JhengHei: Traditional Chinese
        "YuGothM.ttc",  // Yu Gothic: Japanese
        "meiryo.ttc",   // Meiryo: Japanese, before Yu Gothic
        "malgun.ttf",   // Malgun Gothic: Korean
        "segoeui.ttf",  // Segoe UI: Arabic, Hebrew, Armenian, Georgian
        "Nirmala.ttf",  // Nirmala UI: the Indic scripts
        "leelawui.ttf", // Leelawadee UI: Thai, Lao, Khmer
        "ebrima.ttf",   // Ebrima: Ethiopic, N'Ko, Tifinagh, Vai
        "mmrtext.ttf",  // Myanmar Text
        "simsun.ttc",   // SimSun: Chinese on older installs
    ];
    let dir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("Fonts");
    existing(FILES.iter().map(|f| dir.join(f)))
}

/// macOS's own fonts for each script.
#[cfg(target_os = "macos")]
fn system_faces() -> Vec<Face> {
    const FILES: &[&str] = &[
        "/System/Library/Fonts/PingFang.ttc",              // Chinese
        "/System/Library/Fonts/Hiragino Sans GB.ttc",      // Simplified Chinese
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", // Hiragino Sans: Japanese
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",      // Korean
        "/System/Library/Fonts/GeezaPro.ttc",              // Arabic
        "/System/Library/Fonts/Thonburi.ttc",              // Thai
        "/System/Library/Fonts/KohinoorDevanagari.ttc",    // Hindi, Marathi
        "/System/Library/Fonts/KohinoorBangla.ttc",        // Bengali
        "/System/Library/Fonts/KohinoorTelugu.ttc",        // Telugu
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf", // everything else
    ];
    existing(FILES.iter().map(PathBuf::from))
}

/// Face 0 of every file that exists.
#[cfg(any(windows, target_os = "macos"))]
fn existing(files: impl Iterator<Item = PathBuf>) -> Vec<Face> {
    files
        .filter(|f| f.is_file())
        .map(|file| Face { file, index: 0 })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fc_match_output_names_a_face() {
        let face = parse_match("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc\n2\n").unwrap();
        assert_eq!(
            face,
            Face {
                file: PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
                index: 2
            }
        );
        assert_eq!(parse_match("/a/b.ttf\n").unwrap().index, 0);
        assert!(parse_match("\n0\n").is_err());
        assert!(parse_match("/a/b.ttf\nx\n").is_err());
    }

    #[test]
    fn every_face_found_parses() {
        for face in system_faces() {
            let bytes = std::fs::read(&face.file).unwrap();
            ab_glyph::FontRef::try_from_slice_and_index(&bytes, face.index)
                .unwrap_or_else(|e| panic!("{}: {e}", face.file.display()));
        }
    }
}
