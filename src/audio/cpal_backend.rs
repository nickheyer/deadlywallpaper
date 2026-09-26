//! Windows (WASAPI loopback on the default output) and macOS (a chosen input or loopback device).

use crate::audio::{Analyzer, Sink, downmix};
use crate::error::{Error, Result};
use crate::ipc::AudioDevice;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn pick(host: &cpal::Host, device: Option<&str>) -> Result<cpal::Device> {
    if let Some(name) = device.filter(|d| !d.is_empty()) {
        let all = host.input_devices().into_iter().flatten().chain(host.output_devices().into_iter().flatten());
        for d in all {
            if d.description().is_ok_and(|desc| desc.name() == name) {
                return Ok(d);
            }
        }
        return Err(Error::NotFound(format!("audio device '{name}' not found")));
    }
    let default = if cfg!(windows) { host.default_output_device() } else { host.default_input_device() };
    default.ok_or_else(|| Error::Platform("no default audio device".into()))
}

pub fn start(device: Option<String>, stop: Arc<AtomicBool>, sink: Sink) -> Result<()> {
    let host = cpal::default_host();
    let dev = pick(&host, device.as_deref())?;
    let config = dev
        .default_input_config()
        .or_else(|_| dev.default_output_config())
        .map_err(|e| Error::Platform(format!("audio config: {e}")))?;
    let channels = config.channels() as usize;
    let analyzer = Arc::new(Mutex::new(Analyzer::new()));
    let sink = Arc::new(Mutex::new(sink));
    let stream = dev
        .build_input_stream(
            config.config(),
            move |data: &[f32], _| {
                let bins = analyzer.lock().ok().and_then(|mut a| a.push(&downmix(data, channels)));
                if let Some(bins) = bins {
                    if let Ok(mut s) = sink.lock() {
                        s(bins);
                    }
                }
            },
            |e| log::warn!("audio stream: {e}"),
            None,
        )
        .map_err(|e| Error::Platform(format!("audio stream: {e}")))?;
    stream.play().map_err(|e| Error::Platform(format!("audio start: {e}")))?;
    std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            let _keep = stream;
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(())
}

pub fn devices() -> Vec<AudioDevice> {
    let host = cpal::default_host();
    let default_label = if cfg!(windows) { "System output (default loopback)" } else { "Default input device" };
    let mut out = vec![AudioDevice { id: String::new(), name: default_label.into() }];
    let inputs = host.input_devices().into_iter().flatten();
    let outputs: Box<dyn Iterator<Item = cpal::Device>> = if cfg!(windows) { Box::new(host.output_devices().into_iter().flatten()) } else { Box::new(std::iter::empty()) };
    for d in inputs.chain(outputs) {
        if let Ok(desc) = d.description() {
            let name = desc.name().to_string();
            if !out.iter().any(|o| o.id == name) {
                out.push(AudioDevice { id: name.clone(), name });
            }
        }
    }
    out
}
