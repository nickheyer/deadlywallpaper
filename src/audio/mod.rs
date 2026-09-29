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

pub const BINS: usize = 128;
pub const WINDOW: usize = 1024;

/// Rolling FFT over mono samples. Output matches Lively's visualizer feed: 128 magnitude bins
/// from the low end of the spectrum, averaged over two frames and smoothed across neighbors.
pub struct Analyzer {
    fft: Arc<dyn realfft::RealToComplex<f32>>,
    input: Vec<f32>,
    spectrum: Vec<realfft::num_complex::Complex<f32>>,
    pending: Vec<f32>,
    previous: Vec<f32>,
}

impl Analyzer {
    pub fn new() -> Analyzer {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        Analyzer {
            input: fft.make_input_vec(),
            spectrum: fft.make_output_vec(),
            fft,
            pending: Vec::with_capacity(WINDOW * 2),
            previous: vec![0.0; BINS],
        }
    }

    /// Feed mono samples; returns a frame whenever a full window has accumulated.
    pub fn push(&mut self, mono: &[f32]) -> Option<Vec<f32>> {
        self.pending.extend_from_slice(mono);
        if self.pending.len() < WINDOW {
            return None;
        }
        self.input.copy_from_slice(&self.pending[..WINDOW]);
        self.pending.drain(..WINDOW);
        if self
            .fft
            .process(&mut self.input, &mut self.spectrum)
            .is_err()
        {
            return None;
        }
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
}

pub struct Capture {
    stop: Arc<AtomicBool>,
}

impl Capture {
    /// Start capturing from `device` (`None` = system output) and stream spectra to the engine.
    pub fn start(device: Option<String>, tx: MsgSender) -> Result<Capture> {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let sink = move |bins: Vec<f32>| tx.send(Msg::Audio(bins));
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

pub type Sink = Box<dyn FnMut(Vec<f32>) + Send + 'static>;

/// Interleaved stereo (or mono) f32 frames to mono.
pub fn downmix(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks_exact(channels)
        .map(|f| f.iter().sum::<f32>() / channels as f32)
        .collect()
}
