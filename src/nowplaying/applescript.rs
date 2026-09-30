//! Music and Spotify on macOS, read through the Apple Events they publish for other
//! applications. Only running players are asked, so none is launched by the query.

use crate::error::{Error, Result};
use crate::nowplaying::{Art, NowPlaying, POLL, Playback, Properties, Sink};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const SEP: &str = "\u{1f}";

pub fn start(stop: Arc<AtomicBool>, mut sink: Sink) -> Result<()> {
    let art_file = std::env::temp_dir().join(format!(
        "deadlywp-art-{}.bin",
        crate::paths::nonce()
    ));
    std::thread::Builder::new()
        .name("nowplaying".into())
        .spawn(move || {
            let mut last_art: Option<(String, Art)> = None;
            while !stop.load(Ordering::Relaxed) {
                match read(&art_file, &mut last_art) {
                    Ok(reading) => sink(reading),
                    Err(e) => {
                        log::warn!("media players: {e}");
                        sink(None);
                    }
                }
                std::thread::sleep(POLL);
            }
            let _ = std::fs::remove_file(&art_file);
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(())
}

fn osascript(script: &str) -> Result<String> {
    let out = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| Error::Platform(format!("osascript: {e}")))?;
    if !out.status.success() {
        return Err(Error::Platform(format!(
            "osascript: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn running(app: &str) -> Result<bool> {
    Ok(osascript(&format!(
        "tell application \"System Events\" to (name of processes) contains \"{app}\""
    ))? == "true")
}

fn read(art_file: &PathBuf, last_art: &mut Option<(String, Art)>) -> Result<Option<NowPlaying>> {
    let mut readings = Vec::new();
    if running("Music")? {
        if let Some(np) = read_music(art_file, last_art)? {
            readings.push(np);
        }
    }
    if running("Spotify")? {
        if let Some(np) = read_spotify()? {
            readings.push(np);
        }
    }
    let pick = readings
        .iter()
        .position(|np| np.playback == Playback::Playing)
        .or_else(|| {
            readings
                .iter()
                .position(|np| np.playback == Playback::Paused)
        });
    Ok(pick.map(|i| readings.swap_remove(i)))
}

fn state(s: &str) -> Playback {
    match s {
        "playing" => Playback::Playing,
        "paused" => Playback::Paused,
        _ => Playback::Stopped,
    }
}

fn read_music(art_file: &PathBuf, last_art: &mut Option<(String, Art)>) -> Result<Option<NowPlaying>> {
    let script = format!(
        r#"tell application "Music"
    if player state is stopped then return "stopped"
    set t to current track
    return (player state as text) & "{SEP}" & (name of t) & "{SEP}" & (artist of t) & "{SEP}" & (album of t) & "{SEP}" & (album artist of t) & "{SEP}" & (genre of t) & "{SEP}" & (player position as text) & "{SEP}" & (duration of t as text) & "{SEP}" & (persistent ID of t) & "{SEP}" & (count of artworks of t) & "{SEP}" & (video kind of t as text)
end tell"#
    );
    let out = osascript(&script)?;
    if out == "stopped" {
        return Ok(None);
    }
    let f: Vec<&str> = out.split(SEP).collect();
    if f.len() < 11 {
        return Err(Error::Platform(format!("Music answered {out:?}")));
    }
    let art_key = format!("music:{}", f[8]);
    let art = if f[9].trim().parse::<u32>().unwrap_or(0) > 0 {
        match last_art {
            Some((k, a)) if *k == art_key => Some(a.clone()),
            _ => {
                let path = art_file.to_string_lossy().replace('"', "\\\"");
                osascript(&format!(
                    r#"tell application "Music"
    set f to open for access POSIX file "{path}" with write permission
    set eof f to 0
    write (raw data of artwork 1 of current track) to f
    close access f
end tell"#
                ))?;
                let bytes = std::fs::read(art_file)?;
                let a = Art::Bytes(bytes);
                *last_art = Some((art_key, a.clone()));
                Some(a)
            }
        }
    } else {
        None
    };
    let content_type = if f[10] == "none" { "music" } else { "video" };
    Ok(Some(NowPlaying {
        properties: Properties {
            title: f[1].into(),
            artist: f[2].into(),
            sub_title: String::new(),
            album_title: f[3].into(),
            album_artist: f[4].into(),
            genres: f[5].into(),
            content_type: content_type.into(),
        },
        art,
        playback: state(f[0]),
        position: f[6].trim().replace(',', ".").parse().ok(),
        duration: f[7].trim().replace(',', ".").parse().ok(),
    }))
}

fn read_spotify() -> Result<Option<NowPlaying>> {
    let script = format!(
        r#"tell application "Spotify"
    if player state is stopped then return "stopped"
    set t to current track
    return (player state as text) & "{SEP}" & (name of t) & "{SEP}" & (artist of t) & "{SEP}" & (album of t) & "{SEP}" & (album artist of t) & "{SEP}" & (player position as text) & "{SEP}" & (duration of t as text) & "{SEP}" & (artwork url of t)
end tell"#
    );
    let out = osascript(&script)?;
    if out == "stopped" {
        return Ok(None);
    }
    let f: Vec<&str> = out.split(SEP).collect();
    if f.len() < 8 {
        return Err(Error::Platform(format!("Spotify answered {out:?}")));
    }
    let art_url = f[7].trim();
    Ok(Some(NowPlaying {
        properties: Properties {
            title: f[1].into(),
            artist: f[2].into(),
            sub_title: String::new(),
            album_title: f[3].into(),
            album_artist: f[4].into(),
            genres: String::new(),
            content_type: "music".into(),
        },
        art: (!art_url.is_empty()).then(|| Art::Url(art_url.to_string())),
        playback: state(f[0]),
        position: f[5].trim().replace(',', ".").parse().ok(),
        duration: f[6]
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .map(|ms| ms / 1000.0),
    }))
}
