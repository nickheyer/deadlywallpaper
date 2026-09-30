//! Windows' global media session (System Media Transport Controls), the same source
//! Wallpaper Engine reads.

use crate::error::{Error, Result};
use crate::nowplaying::{Art, NowPlaying, POLL, Playback, Properties, Sink};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::MediaPlaybackType;
use windows::Storage::Streams::DataReader;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

pub fn start(stop: Arc<AtomicBool>, mut sink: Sink) -> Result<()> {
    std::thread::Builder::new()
        .name("nowplaying".into())
        .spawn(move || {
            // SAFETY: initialising the WinRT apartment for this thread has no preconditions;
            // a mode mismatch only means the thread was initialised already.
            let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
            let manager = match Manager::RequestAsync().and_then(|op| op.join()) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("media session manager: {e}");
                    return;
                }
            };
            while !stop.load(Ordering::Relaxed) {
                match read(&manager) {
                    Ok(reading) => sink(reading),
                    Err(e) => {
                        log::warn!("media session: {e}");
                        sink(None);
                    }
                }
                std::thread::sleep(POLL);
            }
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(())
}

fn read(manager: &Manager) -> windows::core::Result<Option<NowPlaying>> {
    let Ok(session) = manager.GetCurrentSession() else {
        return Ok(None);
    };
    Ok(Some(read_session(&session)?))
}

fn read_session(session: &Session) -> windows::core::Result<NowPlaying> {
    let info = session.GetPlaybackInfo()?;
    let playback = match info.PlaybackStatus()? {
        Status::Playing | Status::Changing => Playback::Playing,
        Status::Paused => Playback::Paused,
        _ => Playback::Stopped,
    };
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let genres: Vec<String> = props
        .Genres()
        .map(|g| g.into_iter().map(|s| s.to_string()).collect())
        .unwrap_or_default();
    let content_type = match props.PlaybackType().and_then(|r| r.Value()) {
        Ok(MediaPlaybackType::Video) => "video",
        Ok(MediaPlaybackType::Image) => "image",
        _ => "music",
    };
    let art = props.Thumbnail().ok().and_then(|reference| {
        let stream = reference.OpenReadAsync().ok()?.join().ok()?;
        let size = stream.Size().ok()?;
        let reader = DataReader::CreateDataReader(&stream).ok()?;
        reader.LoadAsync(size as u32).ok()?.join().ok()?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        Some(Art::Bytes(bytes))
    });
    let timeline = session.GetTimelineProperties()?;
    let seconds = |t: windows::Foundation::TimeSpan| t.Duration as f64 / 10_000_000.0;
    let start = seconds(timeline.StartTime()?);
    let end = seconds(timeline.EndTime()?);
    let position = seconds(timeline.Position()?);
    let duration = (end - start).max(0.0);
    Ok(NowPlaying {
        properties: Properties {
            title: props.Title()?.to_string(),
            artist: props.Artist()?.to_string(),
            sub_title: props.Subtitle()?.to_string(),
            album_title: props.AlbumTitle()?.to_string(),
            album_artist: props.AlbumArtist()?.to_string(),
            genres: genres.join(", "),
            content_type: content_type.into(),
        },
        art,
        playback,
        position: (duration > 0.0).then_some((position - start).max(0.0)),
        duration: (duration > 0.0).then_some(duration),
    })
}
