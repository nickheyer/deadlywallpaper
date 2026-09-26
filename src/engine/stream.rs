//! Online video detection through yt-dlp.

use crate::error::{Error, Result};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub struct Probe {
    pub title: String,
}

/// `Ok(Some)` when yt-dlp can play the URL, `Ok(None)` when it is a plain web page.
pub fn probe(url: &str) -> Result<Option<Probe>> {
    let out = run(Command::new("yt-dlp").args(["--no-warnings", "--no-playlist", "--skip-download", "--print", "title", url]), 25)?;
    if !out.status.success() {
        return Ok(None);
    }
    let title = String::from_utf8_lossy(&out.stdout).lines().next().unwrap_or("").trim().to_string();
    Ok(Some(Probe { title }))
}

/// Download the stream's cover image as `thumbnail.<ext>` into `dir`; returns the file name.
pub fn thumbnail(url: &str, dir: &Path) -> Result<String> {
    let template = dir.join("thumbnail.%(ext)s");
    let out = run(
        Command::new("yt-dlp").args(["--no-warnings", "--no-playlist", "--skip-download", "--write-thumbnail", "--convert-thumbnails", "jpg", "-o"]).arg(&template).arg(url),
        40,
    )?;
    if !out.status.success() {
        return Err(Error::Media(format!("yt-dlp thumbnail: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("thumbnail."))
        .min_by_key(|n| if n.ends_with(".jpg") { 0 } else { 1 })
        .ok_or_else(|| Error::Media("yt-dlp produced no thumbnail".into()))
}

/// YouTube watch links play in the web view through the embed player when yt-dlp is missing.
pub fn youtube_embed(url: &str) -> Option<String> {
    let id = if let Some(rest) = url.split("youtu.be/").nth(1) {
        rest.split(['?', '&', '/']).next()
    } else if url.contains("youtube.com/") {
        url.split("v=").nth(1).and_then(|s| s.split('&').next())
    } else {
        None
    }?;
    (id.len() == 11).then(|| format!("https://www.youtube.com/embed/{id}?autoplay=1&loop=1&controls=0&mute=0&playlist={id}"))
}

fn run(cmd: &mut Command, timeout_secs: u64) -> Result<std::process::Output> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::Unsupported("yt-dlp is not installed".into()),
        _ => Error::Io(e),
    })?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Media("yt-dlp timed out".into()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
