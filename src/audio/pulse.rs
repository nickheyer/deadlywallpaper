//! PulseAudio / PipeWire capture of the default sink monitor (or a chosen source).

use crate::audio::{Analyzer, Sink, WINDOW};
use crate::error::{Error, Result};
use crate::ipc::AudioDevice;
use libpulse_binding::context::{Context, FlagSet as ContextFlags, State};
use libpulse_binding::def::BufferAttr;
use libpulse_binding::mainloop::standard::{IterateResult, Mainloop};
use libpulse_binding::sample::{Format, Spec};
use libpulse_binding::stream::Direction;
use libpulse_simple_binding::Simple;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const RATE: u32 = 48_000;

pub fn start(device: Option<String>, stop: Arc<AtomicBool>, mut sink: Sink) -> Result<()> {
    let spec = Spec {
        format: Format::F32le,
        channels: 2,
        rate: RATE,
    };
    let source = device.unwrap_or_else(|| "@DEFAULT_MONITOR@".into());
    let attr = BufferAttr {
        maxlength: u32::MAX,
        tlength: u32::MAX,
        prebuf: u32::MAX,
        minreq: u32::MAX,
        fragsize: (WINDOW * 2 * 4) as u32,
    };
    let simple = Simple::new(
        None,
        crate::paths::APP_NAME,
        Direction::Record,
        Some(&source),
        "visualizer",
        &spec,
        None,
        Some(&attr),
    )
    .map_err(|e| Error::Platform(format!("audio capture on {source}: {e}")))?;
    std::thread::Builder::new()
        .name("audio-capture".into())
        .spawn(move || {
            let mut analyzer = Analyzer::new(2);
            let mut bytes = vec![0u8; WINDOW * 2 * 4];
            while !stop.load(Ordering::Relaxed) {
                if let Err(e) = simple.read(&mut bytes) {
                    log::warn!("audio read: {e}");
                    break;
                }
                let samples: Vec<f32> = bytes
                    .chunks_exact(4)
                    .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .collect();
                if let Some(spectrum) = analyzer.push(&samples) {
                    sink(spectrum);
                }
            }
        })
        .map_err(|e| Error::Platform(e.to_string()))?;
    Ok(())
}

pub fn devices() -> Vec<AudioDevice> {
    let mut out = vec![AudioDevice {
        id: String::new(),
        name: "System output (default monitor)".into(),
    }];
    let Some(mut mainloop) = Mainloop::new() else {
        return out;
    };
    let Some(mut context) = Context::new(&mainloop, crate::paths::APP_ID) else {
        return out;
    };
    if context.connect(None, ContextFlags::NOFLAGS, None).is_err() {
        return out;
    }
    loop {
        match mainloop.iterate(true) {
            IterateResult::Quit(_) | IterateResult::Err(_) => return out,
            IterateResult::Success(_) => {}
        }
        match context.get_state() {
            State::Ready => break,
            State::Failed | State::Terminated => return out,
            _ => {}
        }
    }
    let found: Arc<std::sync::Mutex<Vec<AudioDevice>>> = Arc::default();
    let done = Arc::new(AtomicBool::new(false));
    let (f2, d2) = (found.clone(), done.clone());
    let op = context
        .introspect()
        .get_source_info_list(move |res| match res {
            libpulse_binding::callbacks::ListResult::Item(info) => {
                let id = info.name.as_deref().unwrap_or("").to_string();
                let name = info.description.as_deref().unwrap_or(&id).to_string();
                if !id.is_empty() {
                    if let Ok(mut v) = f2.lock() {
                        v.push(AudioDevice { id, name });
                    }
                }
            }
            _ => d2.store(true, Ordering::Relaxed),
        });
    while !done.load(Ordering::Relaxed) {
        if let IterateResult::Quit(_) | IterateResult::Err(_) = mainloop.iterate(true) {
            break;
        }
    }
    drop(op);
    context.disconnect();
    if let Ok(v) = found.lock() {
        out.extend(v.iter().cloned());
    }
    out
}
