//! System audio capture and spectrum analysis for visualizer wallpapers.

use crate::error::Result;
use crate::ipc::AudioDevice;
use crate::msg::Msg;
use crate::platform::{MsgSender, MsgSenderApi};
use realfft::RealFftPlanner;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(target_os = "linux")]
mod pulse;
#[cfg(target_os = "linux")]
use pulse as backend;

#[cfg(not(target_os = "linux"))]
mod cpal_backend;
#[cfg(not(target_os = "linux"))]
use cpal_backend as backend;

/// Bins in Lively's visualizer feed.
pub const BINS: usize = 128;
/// Bands per channel in Wallpaper Engine's feed.
pub const WE_BANDS: usize = 64;
pub const WINDOW: usize = 1024;

/// One analysed window of system audio, in both dialects wallpapers understand.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spectrum {
    /// Lively's feed: 128 mono magnitude bins from the low end of the spectrum.
    pub lively: Vec<f32>,
    /// Wallpaper Engine's feed: 64 left-channel bands followed by 64 right-channel bands,
    /// each in `0..=1`.
    pub we: Vec<f32>,
}

/// Rolling FFT over interleaved samples. The Lively bins average two frames and smooth
/// across neighbours; the Wallpaper Engine bands take every second FFT bin per channel,
/// compress it logarithmically, weight it towards the treble and ease towards each new value.
pub struct Analyzer {
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    /// Interleaved frames not yet analysed.
    pending: Vec<f32>,
    channels: usize,
    previous: Vec<f32>,
    we: Vec<f32>,
}

impl Analyzer {
    pub fn new(channels: usize) -> Analyzer {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        Analyzer {
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            fft,
            pending: Vec::with_capacity(WINDOW * channels.max(1) * 2),
            channels: channels.max(1),
            previous: vec![0.0; BINS],
            we: vec![0.0; WE_BANDS * 2],
        }
    }

    /// Feed interleaved samples; returns a spectrum whenever a full window has accumulated.
    pub fn push(&mut self, samples: &[f32]) -> Option<Spectrum> {
        self.pending.extend_from_slice(samples);
        let frame = WINDOW * self.channels;
        if self.pending.len() < frame {
            return None;
        }
        let window: Vec<f32> = self.pending.drain(..frame).collect();
        let lively = self.lively(&window)?;
        let left = self.we_bands(&window, 0)?;
        let right = self.we_bands(&window, (self.channels > 1) as usize)?;
        for (i, target) in left.iter().chain(right.iter()).enumerate() {
            self.we[i] = move_towards(self.we[i], *target, 0.3);
        }
        Some(Spectrum {
            lively,
            we: self.we.clone(),
        })
    }

    fn lively(&mut self, window: &[f32]) -> Option<Vec<f32>> {
        for (i, dst) in self.input.iter_mut().enumerate() {
            let f = &window[i * self.channels..(i + 1) * self.channels];
            *dst = f.iter().sum::<f32>() / self.channels as f32;
        }
        self.fft.process(&mut self.input, &mut self.spectrum).ok()?;
        let scale = 1.0 / (WINDOW as f32).sqrt();
        let current: Vec<f32> = self
            .spectrum
            .iter()
            .take(BINS)
            .map(|c| c.norm() * scale)
            .collect();
        let vertical: Vec<f32> = current
            .iter()
            .zip(&self.previous)
            .map(|(a, b)| (a + b) / 2.0)
            .collect();
        self.previous = current;
        Some(
            (0..BINS)
                .map(|i| {
                    let lo = i.saturating_sub(1);
                    let hi = (i + 2).min(BINS);
                    vertical[lo..hi].iter().sum::<f32>() / 4.0
                })
                .collect(),
        )
    }

    /// Wallpaper Engine's 64 bands for one channel of the window.
    fn we_bands(&mut self, window: &[f32], channel: usize) -> Option<Vec<f32>> {
        for (i, dst) in self.input.iter_mut().enumerate() {
            *dst = window[i * self.channels + channel];
        }
        self.fft.process(&mut self.input, &mut self.spectrum).ok()?;
        Some(
            (0..WE_BANDS)
                .map(|band| {
                    let c = self.spectrum[band * 2];
                    let power = c.re * c.re + c.im * c.im;
                    let level = if power > 0.0 {
                        0.35 * power.log10()
                    } else {
                        0.0
                    };
                    let weight = 2.0 - ((1.0 - band as f32 / 63.0) - 0.5).exp();
                    (level * weight).clamp(0.0, 1.0)
                })
                .collect(),
        )
    }
}

fn move_towards(current: f32, target: f32, step: f32) -> f32 {
    if (target - current).abs() <= step {
        target
    } else {
        current + step.copysign(target - current)
    }
}

pub struct Capture {
    stop: Arc<AtomicBool>,
}

impl Capture {
    /// Start capturing from `device` (`None` = system output) and stream spectra to the engine.
    pub fn start(device: Option<String>, tx: MsgSender) -> Result<Capture> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let sink = move |spectrum: Spectrum| tx.send(Msg::Audio(spectrum));
        backend::start(device, flag, Box::new(sink))?;
        Ok(Capture { stop })
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

pub fn devices() -> Vec<AudioDevice> {
    backend::devices()
}

pub type Sink = Box<dyn FnMut(Spectrum) + Send + 'static>;

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, amplitude: f32, left: bool) -> Vec<f32> {
        (0..WINDOW)
            .flat_map(|n| {
                let v = amplitude * (2.0 * std::f32::consts::PI * hz * n as f32 / 48_000.0).sin();
                if left { [v, 0.0] } else { [0.0, v] }
            })
            .collect()
    }

    #[test]
    fn wallpaper_engine_bands_follow_the_channel_that_carries_the_tone() {
        let mut a = Analyzer::new(2);
        // Bin 20 (937.5 Hz at 48 kHz) lands in band 10 of the left channel only.
        let mut last = None;
        for _ in 0..12 {
            last = a.push(&tone(937.5, 0.9, true)).or(last);
        }
        let s = last.expect("a full window was analysed");
        assert_eq!(s.lively.len(), BINS);
        assert_eq!(s.we.len(), WE_BANDS * 2);
        assert!(s.we[10] > 0.9, "left band 10 = {}", s.we[10]);
        assert!(
            s.we[WE_BANDS + 10] < 0.01,
            "right band 10 = {}",
            s.we[WE_BANDS + 10]
        );
        assert!(s.we.iter().all(|v| (0.0..=1.0).contains(v)));
        assert!(s.lively[20] > s.lively[60]);

        let mut a = Analyzer::new(2);
        let s = a.push(&tone(937.5, 0.9, false)).expect("first window");
        // The first frame eases towards the target by 0.3 at most.
        assert!(
            (s.we[WE_BANDS + 10] - 0.3).abs() < 1e-6,
            "{}",
            s.we[WE_BANDS + 10]
        );
        assert_eq!(s.we[10], 0.0);
    }

    #[test]
    fn mono_input_feeds_both_channels() {
        let mut a = Analyzer::new(1);
        let mono: Vec<f32> = (0..WINDOW * 3)
            .map(|n| 0.8 * (2.0 * std::f32::consts::PI * 937.5 * n as f32 / 48_000.0).sin())
            .collect();
        let s = a.push(&mono).expect("first window");
        assert_eq!(s.we[10], s.we[WE_BANDS + 10]);
        assert!(s.we[10] > 0.0);
    }
}
