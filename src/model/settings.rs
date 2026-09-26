use crate::error::{Result, ctx};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub library_dir: PathBuf,
    pub autostart: bool,
    pub tray: bool,
    pub theme: Theme,
    /// 0..=100, applied to every wallpaper that carries audio.
    pub volume: u8,
    pub audio_only_on_desktop: bool,
    pub audio_output: AudioOutput,
    /// Capture device for web audio visualizers; `None` selects the system output monitor.
    pub audio_capture_device: Option<String>,
    pub rules: Rules,
    pub video: Video,
    pub web: Web,
    pub input: Input,
    /// Copy imported media into the library instead of referencing it in place.
    pub copy_imports: bool,
    pub thumbnails: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    /// Pause a display's wallpaper when a window covers it or runs fullscreen there.
    pub fullscreen_pause: bool,
    /// Pause whenever any application window is focused on the display.
    pub focus_pause: bool,
    pub scope: PauseScope,
    /// Fraction of the work area that must be covered to count as covered.
    pub coverage: f64,
    pub battery_pause: bool,
    pub lock_pause: bool,
    pub interval_ms: u64,
    /// Process names that pause all wallpapers while running.
    pub app_pause: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Video {
    pub hw_accel: bool,
    pub scaler: Scaler,
    pub stream_quality: StreamQuality,
    pub load_timeout_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Web {
    pub devtools: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Input {
    /// Deliver pointer motion and clicks to interactive wallpapers.
    pub forward_mouse: bool,
    /// Keep delivering motion while another application is focused.
    pub always_move: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AudioOutput {
    #[default]
    All,
    Primary,
    Display(String),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PauseScope {
    #[default]
    Display,
    All,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scaler {
    None,
    #[default]
    Fill,
    Uniform,
    UniformFill,
}

impl Scaler {
    pub const ALL: [Scaler; 4] = [Scaler::None, Scaler::Fill, Scaler::Uniform, Scaler::UniformFill];

    pub fn label(self) -> &'static str {
        match self {
            Scaler::None => "None",
            Scaler::Fill => "Fill",
            Scaler::Uniform => "Uniform",
            Scaler::UniformFill => "Uniform fill",
        }
    }

    pub fn from_index(i: i64) -> Scaler {
        Scaler::ALL[(i.clamp(0, 3)) as usize]
    }

    /// libmpv property assignments realizing this fit.
    pub fn mpv_properties(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Scaler::None => &[("keepaspect", "yes"), ("video-unscaled", "yes"), ("panscan", "0.0")],
            Scaler::Fill => &[("video-unscaled", "no"), ("keepaspect", "no"), ("panscan", "0.0")],
            Scaler::Uniform => &[("video-unscaled", "no"), ("keepaspect", "yes"), ("panscan", "0.0")],
            Scaler::UniformFill => &[("video-unscaled", "no"), ("keepaspect", "yes"), ("panscan", "1.0")],
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StreamQuality {
    P144,
    P240,
    P360,
    P480,
    P720,
    #[default]
    P1080,
    Best,
}

impl StreamQuality {
    pub const ALL: [StreamQuality; 7] = [
        StreamQuality::P144,
        StreamQuality::P240,
        StreamQuality::P360,
        StreamQuality::P480,
        StreamQuality::P720,
        StreamQuality::P1080,
        StreamQuality::Best,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StreamQuality::P144 => "144p",
            StreamQuality::P240 => "240p",
            StreamQuality::P360 => "360p",
            StreamQuality::P480 => "480p",
            StreamQuality::P720 => "720p",
            StreamQuality::P1080 => "1080p",
            StreamQuality::Best => "Best available",
        }
    }

    pub fn ytdl_format(self) -> String {
        let cap = match self {
            StreamQuality::P144 => 144,
            StreamQuality::P240 => 240,
            StreamQuality::P360 => 360,
            StreamQuality::P480 => 480,
            StreamQuality::P720 => 720,
            StreamQuality::P1080 => 1080,
            StreamQuality::Best => return "bestvideo+bestaudio/best".into(),
        };
        format!("bestvideo[height<={cap}]+bestaudio/best")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            library_dir: PathBuf::new(),
            autostart: true,
            tray: true,
            theme: Theme::System,
            volume: 75,
            audio_only_on_desktop: true,
            audio_output: AudioOutput::All,
            audio_capture_device: None,
            rules: Rules::default(),
            video: Video::default(),
            web: Web::default(),
            input: Input::default(),
            copy_imports: false,
            thumbnails: true,
        }
    }
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            fullscreen_pause: true,
            focus_pause: false,
            scope: PauseScope::Display,
            coverage: 0.95,
            battery_pause: false,
            lock_pause: true,
            interval_ms: 500,
            app_pause: Vec::new(),
        }
    }
}

impl Default for Video {
    fn default() -> Self {
        Video { hw_accel: true, scaler: Scaler::Fill, stream_quality: StreamQuality::P1080, load_timeout_secs: 20 }
    }
}

impl Default for Input {
    fn default() -> Self {
        Input { forward_mouse: true, always_move: true }
    }
}

impl Settings {
    /// Load settings, or defaults when the file does not exist. An unreadable file is
    /// set aside as `settings.json.bad` so the daemon still starts.
    pub fn load(path: &Path, default_library: &Path) -> Settings {
        let mut s = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s,
                Err(e) => {
                    log::warn!("{}: {e}; using defaults", path.display());
                    let _ = std::fs::rename(path, path.with_extension("json.bad"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        if s.library_dir.as_os_str().is_empty() {
            s.library_dir = default_library.to_path_buf();
        }
        s.normalize();
        s
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        ctx(std::fs::write(path, text), path.display())
    }

    pub fn normalize(&mut self) {
        self.volume = self.volume.min(100);
        self.rules.coverage = if self.rules.coverage.is_finite() { self.rules.coverage.clamp(0.5, 1.0) } else { 0.95 };
        self.rules.interval_ms = self.rules.interval_ms.clamp(100, 5000);
        self.video.load_timeout_secs = self.video.load_timeout_secs.clamp(5, 120);
        for a in &mut self.rules.app_pause {
            *a = a.trim().to_string();
        }
        self.rules.app_pause.retain(|a| !a.is_empty());
        self.rules.app_pause.dedup();
    }
}
