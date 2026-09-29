use log::{Level, LevelFilter, Log, Metadata, Record};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

struct Logger {
    file: Option<Mutex<File>>,
    stderr: bool,
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        let level = if m.target().starts_with("deadlywp") {
            log::max_level()
        } else {
            LevelFilter::Warn
        };
        m.level() <= level
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let line = format!(
            "{:.3} {:5} [{}] {}\n",
            now,
            r.level(),
            r.module_path()
                .unwrap_or("?")
                .trim_start_matches("deadlywp::"),
            r.args()
        );
        if self.stderr || r.level() <= Level::Warn {
            let _ = std::io::stderr().write_all(line.as_bytes());
        }
        if let Some(f) = &self.file {
            if let Ok(mut f) = f.lock() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }

    fn flush(&self) {}
}

/// Install the process logger. `file` receives every record; stderr receives warnings,
/// or everything when `stderr` is set.
pub fn init(file: Option<&Path>, stderr: bool) {
    let level = std::env::var("DEADLYWP_LOG")
        .ok()
        .and_then(|v| v.parse::<LevelFilter>().ok())
        .unwrap_or(LevelFilter::Info);
    let file = file.and_then(|p| {
        if let Ok(meta) = std::fs::metadata(p) {
            if meta.len() > 4 * 1024 * 1024 {
                let _ = std::fs::rename(p, p.with_extension("log.old"));
            }
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
            .map(Mutex::new)
    });
    let _ = log::set_boxed_logger(Box::new(Logger { file, stderr }));
    log::set_max_level(level);
}
