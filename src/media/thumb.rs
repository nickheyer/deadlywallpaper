use crate::error::{Error, Result, ctx};
use crate::media::mpv::{self, Handle};
use crate::model::Kind;
use std::path::Path;
use std::time::{Duration, Instant};

pub const WIDTH: u32 = 480;

/// Render one frame of a local media file to a JPEG of [`WIDTH`] pixels.
pub fn capture(source: &Path, kind: Kind, out: &Path, temp_dir: &Path) -> Result<()> {
    let work = temp_dir.join(format!("thumb-{}", crate::paths::nonce()));
    ctx(std::fs::create_dir_all(&work), work.display())?;
    let result = render(source, kind, &work, out);
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn render(source: &Path, kind: Kind, work: &Path, out: &Path) -> Result<()> {
    let handle = Handle::new(mpv::lib()?)?;
    let options = [
        ("config", "no".to_string()),
        ("terminal", "no".into()),
        ("msg-level", "all=error".into()),
        ("vo", "image".into()),
        ("vo-image-format", "jpg".into()),
        ("vo-image-jpeg-quality", "85".into()),
        ("vo-image-outdir", work.to_string_lossy().into_owned()),
        ("frames", "1".into()),
        ("hwdec", "no".into()),
        ("audio", "no".into()),
        ("ytdl", "no".into()),
        ("vf", format!("scale=w={WIDTH}:h=-2")),
        (
            "start",
            if kind == Kind::Video { "10%" } else { "0" }.into(),
        ),
        ("idle", "once".into()),
    ];
    for (k, v) in &options {
        handle.set_option(k, v)?;
    }
    handle.initialize()?;
    handle.command(&["loadfile", &source.to_string_lossy()])?;
    let deadline = Instant::now() + Duration::from_secs(45);
    let mut failure: Option<String> = None;
    loop {
        match handle.wait_event(0.5) {
            mpv::Event::EndFile { reason, error } => {
                if reason == 4 {
                    failure = Some(handle.error_string(error));
                }
                break;
            }
            mpv::Event::Shutdown => break,
            _ => {}
        }
        if Instant::now() > deadline {
            return Err(Error::Media(format!(
                "thumbnail timed out for {}",
                source.display()
            )));
        }
    }
    let produced = std::fs::read_dir(work)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "jpg"))
        .ok_or_else(|| {
            Error::Media(
                failure
                    .unwrap_or_else(|| format!("mpv produced no frame for {}", source.display())),
            )
        })?;
    if std::fs::rename(&produced, out).is_err() {
        ctx(std::fs::copy(&produced, out).map(|_| ()), out.display())?;
    }
    Ok(())
}
