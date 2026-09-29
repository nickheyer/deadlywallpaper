//! Online video detection through yt-dlp.

use crate::error::{Error, Result};
use std::io::{Read, Seek};
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

/// Resolve a page URL to one directly playable media URL (video with audio in one stream),
/// for players that cannot merge separate video and audio streams.
#[cfg(target_os = "linux")]
pub fn direct_url(url: &str, quality: crate::model::settings::StreamQuality) -> Result<String> {
    let format = quality.single_stream_format();
    let out = run(Command::new("yt-dlp").args(["--no-warnings", "--no-playlist", "-g", "-f", &format, url]), 40)?;
    if !out.status.success() {
        return Err(Error::Media(format!("yt-dlp: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("http"))
        .map(str::to_string)
        .ok_or_else(|| Error::Media("yt-dlp found no playable stream".into()))
}

/// YouTube watch links play in the web view through the embed player when yt-dlp is missing.
pub fn youtube_embed(url: &str) -> Option<String> {
    let uri: http::Uri = url.split('#').next()?.parse().ok()?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return None;
    }
    let id = match uri.host()?.to_ascii_lowercase().as_str() {
        "youtu.be" | "www.youtu.be" => uri.path().strip_prefix('/')?,
        "youtube.com" | "www.youtube.com" | "m.youtube.com" if uri.path() == "/watch" => {
            uri.query()?.split('&').find_map(|p| p.strip_prefix("v="))?
        }
        _ => return None,
    };
    (id.len() == 11 && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')))
        .then(|| format!("https://www.youtube.com/embed/{id}?autoplay=1&loop=1&controls=0&mute=0&playlist={id}"))
}

fn run(cmd: &mut Command, timeout_secs: u64) -> Result<std::process::Output> {
    // Files avoid a full stdout/stderr pipe blocking the child before it exits.
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    let mut child = cmd.stdin(Stdio::null()).stdout(stdout.try_clone()?).stderr(stderr.try_clone()?).spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Error::Unsupported("yt-dlp is not installed".into()),
        _ => Error::Io(e),
    })?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                stdout.rewind()?;
                stderr.rewind()?;
                let mut output = std::process::Output { status, stdout: Vec::new(), stderr: Vec::new() };
                stdout.read_to_end(&mut output.stdout)?;
                stderr.read_to_end(&mut output.stderr)?;
                return Ok(output);
            }
            Ok(None) => {},
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.into());
            }
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Media("yt-dlp timed out".into()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_links_require_a_youtube_host_and_video_id() {
        for url in ["https://youtu.be/abcdefghijk?t=5", "https://www.youtube.com/watch?list=x&v=abcdefghijk#t=5"] {
            assert!(youtube_embed(url).unwrap().starts_with("https://www.youtube.com/embed/abcdefghijk?"));
        }
        for url in ["https://example.com/youtu.be/abcdefghijk", "https://notyoutube.com/watch?v=abcdefghijk", "https://youtube.com/watch?notv=abcdefghijk", "https://youtu.be/abc", "https://youtu.be/abc%20defgh"] {
            assert_eq!(youtube_embed(url), None, "{url}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn collects_output_larger_than_pipe_buffers() {
        let output = run(Command::new("sh").args(["-c", "head -c 131072 /dev/zero; head -c 131072 /dev/zero >&2"]), 5).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 131072);
        assert_eq!(output.stderr.len(), 131072);
    }

    #[cfg(unix)]
    #[test]
    fn stops_a_command_at_its_deadline() {
        let result = run(Command::new("sleep").arg("10"), 0);
        assert!(matches!(result, Err(Error::Media(message)) if message.contains("timed out")));
    }
}
