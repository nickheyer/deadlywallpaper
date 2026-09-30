//! Seamless looping for video and GIF wallpapers.
//!
//! Two libmpv cores take turns. While one plays a pass from start to end, the other waits
//! with the first frame already decoded, so the hand-over never stalls on a seek. The last
//! `blend` seconds of a pass cross-fade into the start of the next, picture and sound alike,
//! so a clip whose ends do not match still rolls over without a jolt. Presenters that cannot
//! composite two decoders run a single core that mpv loops itself.

#![cfg_attr(
    windows,
    allow(
        dead_code,
        reason = "mpv draws into its own window on Windows, so passes are never composited"
    )
)]

use crate::content::Seek;
use crate::error::Result;
use crate::media::player::{Looping, Player, PlayerEvent, PlayerOptions};
use crate::model::Control;
use crate::model::props::ControlKind;
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};

/// Name of the built-in control that sets the cross-fade length.
pub const BLEND_CONTROL: &str = "loopblend";

/// Cross-fade when the wallpaper has not set one.
pub const DEFAULT_BLEND_SECS: f64 = 0.5;

/// Which core draws this frame, and which one fades in over it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub active: usize,
    /// The incoming core and how far it has faded in, 0 to 1.
    pub fade: Option<(usize, f32)>,
}

struct State {
    active: usize,
    blend: f64,
    /// The standby is playing and fading in.
    fading: bool,
    /// Engine-wide pause; the standby stays paused regardless.
    paused: bool,
    volume: u8,
    /// Volume factors last pushed to each core.
    gains: [f64; 2],
}

pub struct Loop {
    cores: Vec<Player>,
    state: Mutex<State>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Loop {
    /// Two cores taking turns when the clip loops and the presenter can composite them;
    /// otherwise one core that mpv loops natively. Events from every core reach `on_event`;
    /// only the first core reports `Loaded`, so the wallpaper starts once.
    pub fn spawn(
        opts: PlayerOptions<'_>,
        composited: bool,
        on_event: impl Fn(PlayerEvent) + Send + 'static,
    ) -> Result<Loop> {
        let shared = Arc::new(Mutex::new(on_event));
        let on_event = move |ev: PlayerEvent| lock(&shared)(ev);
        let mut cores = Vec::new();
        if composited && opts.kind.loops() {
            for standby in [false, true] {
                let events = on_event.clone();
                cores.push(Player::new(opts, Looping::Pass { standby }, move |ev| {
                    if !(standby && matches!(ev, PlayerEvent::Loaded)) {
                        events(ev);
                    }
                })?);
            }
        } else {
            cores.push(Player::new(opts, Looping::Native, on_event)?);
        }
        Ok(Loop {
            cores,
            state: Mutex::new(State {
                active: 0,
                blend: DEFAULT_BLEND_SECS,
                fading: false,
                paused: false,
                volume: opts.volume,
                gains: [1.0, 1.0],
            }),
        })
    }

    pub fn players(&self) -> &[Player] {
        &self.cores
    }

    /// Start every core: the first plays, the other waits paused on its first frame.
    pub fn load(&self) -> Result<()> {
        for core in &self.cores {
            core.load()?;
        }
        Ok(())
    }

    fn active(&self) -> &Player {
        &self.cores[lock(&self.state).active]
    }

    /// Advance the hand-over machine; called by the presenter before it draws a frame.
    pub fn tick(&self) -> Frame {
        let mut st = lock(&self.state);
        let only = Frame {
            active: st.active,
            fade: None,
        };
        if self.cores.len() < 2 {
            return only;
        }
        let (ai, bi) = (st.active, 1 - st.active);
        let (a, b) = (&self.cores[ai], &self.cores[bi]);
        let Some(duration) = a.duration() else {
            return only;
        };
        let fade = st.blend.max(2.0 / a.fps()).min(duration / 3.0);
        let start = duration - fade;
        let pos = a.position();
        let eof = a.eof_reached();
        if st.fading && !eof && pos < start - 0.25 {
            // Sought back out of the window: the standby returns to its first frame.
            b.set_paused(true);
            b.rewind();
            st.fading = false;
            self.push_gains(&mut st, [1.0, 1.0]);
        }
        if !st.fading && (eof || pos >= start) {
            st.fading = true;
            if !st.paused {
                b.set_paused(false);
            }
        }
        if !st.fading {
            return only;
        }
        let t = if eof {
            1.0
        } else {
            ((pos - start) / fade).clamp(0.0, 1.0)
        };
        let mut gains = [0.0; 2];
        gains[ai] = 1.0 - t;
        gains[bi] = t;
        self.push_gains(&mut st, gains);
        if t >= 1.0 {
            // Hand over: the incoming core is the pass on screen; the old one rewinds to
            // wait on its first frame for the pass after that.
            a.set_paused(true);
            a.rewind();
            st.active = bi;
            st.fading = false;
            self.push_gains(&mut st, [1.0, 1.0]);
            return Frame {
                active: bi,
                fade: None,
            };
        }
        Frame {
            active: ai,
            fade: Some((bi, t as f32)),
        }
    }

    fn push_gains(&self, st: &mut State, gains: [f64; 2]) {
        for (i, core) in self.cores.iter().enumerate() {
            if (st.gains[i] - gains[i]).abs() > 0.004 {
                st.gains[i] = gains[i];
                core.set_volume_scaled(st.volume, gains[i]);
            }
        }
    }

    pub fn set_paused(&self, paused: bool) {
        let mut st = lock(&self.state);
        st.paused = paused;
        for (i, core) in self.cores.iter().enumerate() {
            if i == st.active || st.fading {
                core.set_paused(paused);
            }
        }
    }

    pub fn set_volume(&self, volume: u8) {
        let mut st = lock(&self.state);
        st.volume = volume;
        for (i, core) in self.cores.iter().enumerate() {
            core.set_volume_scaled(volume, st.gains[i]);
        }
    }

    pub fn set_engine_muted(&self, muted: bool) {
        for core in &self.cores {
            core.set_engine_muted(muted);
        }
    }

    pub fn seek(&self, seek: Seek) {
        self.active().seek(seek);
    }

    /// The blend control is ours; everything else is an mpv property on every core.
    pub fn apply(&self, name: &str, control: &Control, value: Option<&Value>) {
        if name == BLEND_CONTROL {
            if let (ControlKind::Slider { .. }, Some(v)) = (&control.kind, value) {
                if let Some(secs) = v.as_f64() {
                    lock(&self.state).blend = secs.max(0.0);
                }
            }
            return;
        }
        for core in &self.cores {
            core.apply(name, control, value);
        }
    }

    pub fn set_view(&self, view: &crate::content::View) {
        for core in &self.cores {
            core.set_view(view);
        }
    }

    pub fn screenshot(&self, path: &std::path::Path) -> Result<u64> {
        self.active().screenshot(path)
    }

    pub fn slot(&self) -> crate::geom::Size {
        self.cores[0].slot()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::player::Vo;
    use crate::model::Kind;
    use crate::model::settings::{Scaler, StreamQuality};
    use std::io::Write;
    use std::time::{Duration, Instant};

    /// A 16×16 grey clip of `frames` frames at 25 fps in YUV4MPEG2, which mpv demuxes with
    /// a known duration.
    fn clip(dir: &std::path::Path, frames: usize) -> String {
        let path = dir.join("clip.y4m");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"YUV4MPEG2 W16 H16 F25:1 Ip A1:1 C420jpeg\n")
            .unwrap();
        for i in 0..frames {
            f.write_all(b"FRAME\n").unwrap();
            f.write_all(&vec![(i * 4) as u8; 16 * 16 + 2 * 8 * 8])
                .unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    fn spawn(source: &str) -> Loop {
        Loop::spawn(
            PlayerOptions {
                kind: Kind::Video,
                source,
                audio: false,
                volume: 0,
                hw_accel: false,
                scaler: Scaler::Fill,
                stream_quality: StreamQuality::Best,
                vo: Vo::Null,
                slot: crate::geom::Size { w: 16, h: 16 },
            },
            true,
            |_| {},
        )
        .expect("two libmpv cores")
    }

    /// Poll `tick` until `until` says stop or `timeout` passes; every frame seen, in order.
    fn drive(
        looper: &Loop,
        timeout: Duration,
        mut until: impl FnMut(&Frame) -> bool,
    ) -> Vec<Frame> {
        let start = Instant::now();
        let mut frames = Vec::new();
        while start.elapsed() < timeout {
            let f = looper.tick();
            let done = until(&f);
            frames.push(f);
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(8));
        }
        frames
    }

    #[test]
    fn passes_hand_over_through_a_cross_fade() {
        let dir = tempfile::tempdir().unwrap();
        let looper = spawn(&clip(dir.path(), 50));
        assert_eq!(looper.players().len(), 2);
        looper.load().unwrap();
        let blend = Control {
            text: "Loop blend".into(),
            help: None,
            kind: ControlKind::Slider {
                value: 0.5,
                min: 0.0,
                max: 3.0,
                step: 0.05,
            },
        };
        looper.apply(BLEND_CONTROL, &blend, Some(&serde_json::json!(0.6)));

        // First pass: no fade until the window opens 0.6 s before the 2 s end.
        let mut fade_started_at = None;
        let frames = drive(&looper, Duration::from_secs(4), |f| {
            if f.fade.is_some() && fade_started_at.is_none() {
                fade_started_at = Some(looper.players()[0].position());
            }
            f.active == 1
        });
        let first_fade = frames
            .iter()
            .position(|f| f.fade.is_some())
            .expect("a fade began");
        assert!(
            frames[..first_fade]
                .iter()
                .all(|f| f.active == 0 && f.fade.is_none())
        );
        let opened = fade_started_at.unwrap();
        assert!((1.25..=1.6).contains(&opened), "fade opened at {opened}s");
        let fades: Vec<f32> = frames
            .iter()
            .filter_map(|f| f.fade.map(|(_, t)| t))
            .collect();
        assert!(fades.len() >= 5, "fade ran over {} ticks", fades.len());
        assert!(
            fades.windows(2).all(|w| w[0] <= w[1]),
            "fade grows: {fades:?}"
        );
        assert!(frames.iter().all(|f| f.fade.is_none_or(|(i, _)| i == 1)));
        assert_eq!(frames.last().unwrap().active, 1, "the standby took over");
        assert!(frames.last().unwrap().fade.is_none());

        // The retired core waits at the start; the new active pass keeps advancing.
        std::thread::sleep(Duration::from_millis(300));
        assert!(looper.players()[0].position() < 0.2);
        let p1 = looper.players()[1].position();
        assert!(p1 > 0.3, "core 1 is playing on: {p1}");

        // Second hand-over goes the other way.
        let frames = drive(&looper, Duration::from_secs(4), |f| f.active == 0);
        assert!(frames.iter().any(|f| f.fade.is_some_and(|(i, _)| i == 0)));
        assert_eq!(frames.last().unwrap().active, 0);
    }

    #[test]
    fn a_zero_blend_still_hands_over_without_a_gap() {
        let dir = tempfile::tempdir().unwrap();
        let looper = spawn(&clip(dir.path(), 30));
        looper.load().unwrap();
        let blend = Control {
            text: "Loop blend".into(),
            help: None,
            kind: ControlKind::Slider {
                value: 0.5,
                min: 0.0,
                max: 3.0,
                step: 0.05,
            },
        };
        looper.apply(BLEND_CONTROL, &blend, Some(&serde_json::json!(0.0)));
        let frames = drive(&looper, Duration::from_secs(3), |f| f.active == 1);
        assert_eq!(frames.last().unwrap().active, 1, "handed over");
        let fades: Vec<f32> = frames
            .iter()
            .filter_map(|f| f.fade.map(|(_, t)| t))
            .collect();
        assert!(
            !fades.is_empty(),
            "even a cut passes through the two-frame fade"
        );
        let switched = frames.iter().position(|f| f.active == 1).unwrap();
        assert!(
            frames[switched - 1].fade.is_some(),
            "the fade runs right up to the hand-over"
        );
    }
}
