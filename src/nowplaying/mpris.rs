//! Media players on the session bus (MPRIS): the playing one wins, then the one last chosen,
//! then any paused one.

use crate::error::{Error, Result};
use crate::nowplaying::{Art, NowPlaying, POLL, Playback, Properties, Sink};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zbus::blocking::{Connection, Proxy, fdo::DBusProxy};
use zbus::zvariant::OwnedValue;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const ROOT: &str = "org.mpris.MediaPlayer2";

/// Desktop entries of players whose content is video.
const VIDEO_PLAYERS: &[&str] = &[
    "mpv",
    "vlc",
    "celluloid",
    "totem",
    "haruna",
    "smplayer",
    "dragon",
    "kodi",
    "jellyfin",
    "plex",
    "mplayer",
    "kaffeine",
    "gnome-videos",
];

pub fn start(stop: Arc<AtomicBool>, mut sink: Sink) -> Result<()> {
    let conn = Connection::session()
        .map_err(|e| Error::Platform(format!("session bus for media players: {e}")))?;
    std::thread::Builder::new()
        .name("nowplaying".into())
        .spawn(move || {
            let mut chosen: Option<String> = None;
            while !stop.load(Ordering::Relaxed) {
                match read(&conn, &mut chosen) {
                    Ok(reading) => sink(reading),
                    Err(e) => {
                        log::warn!("media players: {e}");
                        sink(None);
                    }
                }
                std::thread::sleep(POLL);
            }
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(())
}

fn read(conn: &Connection, chosen: &mut Option<String>) -> zbus::Result<Option<NowPlaying>> {
    let names: Vec<String> = DBusProxy::new(conn)?
        .list_names()?
        .into_iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with(PREFIX))
        .collect();
    let mut readings: Vec<(String, Playback, NowPlaying)> = Vec::new();
    for name in names {
        match read_player(conn, &name) {
            Ok(np) => readings.push((name, np.playback, np)),
            Err(e) => log::debug!("{name}: {e}"),
        }
    }
    let pick = readings
        .iter()
        .position(|(_, p, _)| *p == Playback::Playing)
        .or_else(|| {
            chosen.as_ref().and_then(|c| {
                readings
                    .iter()
                    .position(|(n, p, _)| n == c && *p != Playback::Stopped)
            })
        })
        .or_else(|| readings.iter().position(|(_, p, _)| *p == Playback::Paused));
    Ok(match pick {
        Some(i) => {
            let (name, _, np) = readings.swap_remove(i);
            *chosen = Some(name);
            Some(np)
        }
        None => {
            *chosen = None;
            None
        }
    })
}

fn read_player(conn: &Connection, name: &str) -> zbus::Result<NowPlaying> {
    let player = Proxy::new(conn, name.to_string(), PATH, PLAYER)?;
    let status: String = player.get_property("PlaybackStatus")?;
    let playback = match status.as_str() {
        "Playing" => Playback::Playing,
        "Paused" => Playback::Paused,
        _ => Playback::Stopped,
    };
    let metadata: HashMap<String, OwnedValue> = player.get_property("Metadata")?;
    let text = |key: &str| -> String {
        metadata
            .get(key)
            .and_then(|v| {
                String::try_from(v.clone()).ok().or_else(|| {
                    Vec::<String>::try_from(v.clone())
                        .ok()
                        .map(|l| l.join(", "))
                })
            })
            .unwrap_or_default()
    };
    let length_us = metadata
        .get("mpris:length")
        .and_then(|v| {
            i64::try_from(v.clone())
                .ok()
                .or_else(|| u64::try_from(v.clone()).ok().map(|u| u as i64))
        })
        .filter(|l| *l > 0);
    let position_us: Option<i64> = player.get_property("Position").ok();
    let root = Proxy::new(conn, name.to_string(), PATH, ROOT)?;
    let desktop: String = root.get_property("DesktopEntry").unwrap_or_default();
    let desktop = desktop.to_ascii_lowercase();
    let content_type = if VIDEO_PLAYERS.iter().any(|p| desktop.contains(p)) {
        "video"
    } else {
        "music"
    };
    let art_url = text("mpris:artUrl");
    Ok(NowPlaying {
        properties: Properties {
            title: text("xesam:title"),
            artist: text("xesam:artist"),
            sub_title: String::new(),
            album_title: text("xesam:album"),
            album_artist: text("xesam:albumArtist"),
            genres: text("xesam:genre"),
            content_type: content_type.into(),
        },
        art: (!art_url.is_empty()).then_some(Art::Url(art_url)),
        playback,
        position: position_us.map(|p| p.max(0) as f64 / 1_000_000.0),
        duration: length_us.map(|l| l as f64 / 1_000_000.0),
    })
}
