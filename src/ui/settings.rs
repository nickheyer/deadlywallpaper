use crate::ipc::{AudioDevice, Capabilities};
use crate::model::Display;
use crate::model::settings::{AudioOutput, PauseScope, Scaler, Settings, StreamQuality, Theme};
use crate::ui::widgets::{divider, row, toggle};
use crate::ui::{theme, widgets};
use crate::we::steam::SteamInfo;
use eframe::egui::{self, RichText};

pub struct Context<'a> {
    pub devices: &'a [AudioDevice],
    pub displays: &'a [Display],
    pub capabilities: &'a Capabilities,
    /// Where the daemon found Steam and Wallpaper Engine; `None` until it has answered.
    pub steam: Option<&'a SteamInfo>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    General,
    Playback,
    Audio,
    WallpaperEngine,
    Advanced,
}

#[derive(Default)]
pub struct State {
    tab: Tab,
    pub focus_new_app: bool,
}

impl State {
    /// Open the Wallpaper Engine tab the next time the page shows.
    pub fn show_wallpaper_engine(&mut self) {
        self.tab = Tab::WallpaperEngine;
    }
}

pub fn tabs(ui: &mut egui::Ui, state: &mut State) {
    widgets::segmented(
        ui,
        "settings-tabs",
        &mut state.tab,
        &[
            (Tab::General, "General"),
            (Tab::Playback, "Playback"),
            (Tab::Audio, "Audio"),
            (Tab::WallpaperEngine, "Wallpaper Engine"),
            (Tab::Advanced, "Advanced"),
        ],
    );
    ui.add_space(8.0);
    divider(ui);
    ui.add_space(12.0);
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::section_title(ui, title);
    ui.add_space(6.0);
    ui.push_id(title, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        add(ui);
    });
    ui.add_space(18.0);
}

/// Returns true while editing, to defer saving.
pub fn ui(ui: &mut egui::Ui, s: &mut Settings, state: &mut State, cx: &Context<'_>) -> bool {
    let p = theme::palette(ui);
    let mut busy = false;

    match state.tab {
        Tab::General => {
            section(ui, "Application", |ui| {
                row(ui, "Start at login", "", |ui| {
                    toggle(ui, &mut s.autostart);
                });
                divider(ui);
                row(ui, "Tray icon", "", |ui| {
                    toggle(ui, &mut s.tray);
                });
                divider(ui);
                row(ui, "Theme", "", |ui| {
                    egui::ComboBox::from_id_salt("theme")
                        .selected_text(theme_label(s.theme))
                        .show_ui(ui, |ui| {
                            for t in [Theme::System, Theme::Light, Theme::Dark] {
                                ui.selectable_value(&mut s.theme, t, theme_label(t));
                            }
                        });
                });
            });
            section(ui, "Library", |ui| {
                let library_dir = s.library_dir.to_string_lossy().into_owned();
                row(ui, "Library folder", &library_dir, |ui| {
                    if ui.add(theme::secondary_button("Change…")).clicked() {
                        if let Some(dir) = rfd::FileDialog::new()
                            .set_directory(&s.library_dir)
                            .pick_folder()
                        {
                            s.library_dir = dir;
                        }
                    }
                });
                divider(ui);
                row(ui, "Copy imported files", "", |ui| {
                    toggle(ui, &mut s.copy_imports);
                });
                divider(ui);
                row(ui, "Generate thumbnails", "", |ui| {
                    toggle(ui, &mut s.thumbnails);
                });
            });
        }
        Tab::Playback => {
            section(ui, "Pause wallpapers", |ui| {
                row(ui, "When a window covers the display", "", |ui| {
                    toggle(ui, &mut s.rules.fullscreen_pause);
                });
                divider(ui);
                ui.add_enabled_ui(s.rules.fullscreen_pause, |ui| {
                    row(ui, "Also when a window is focused", "", |ui| {
                        toggle(ui, &mut s.rules.focus_pause);
                    });
                    divider(ui);
                    row(ui, "Pause on", "", |ui| {
                        egui::ComboBox::from_id_salt("scope")
                            .width(210.0)
                            .selected_text(match s.rules.scope {
                                PauseScope::Display => "That display",
                                PauseScope::All => "All displays",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut s.rules.scope,
                                    PauseScope::Display,
                                    "That display",
                                );
                                ui.selectable_value(
                                    &mut s.rules.scope,
                                    PauseScope::All,
                                    "All displays",
                                );
                            });
                    });
                    divider(ui);
                });
                row(ui, "On battery", "", |ui| {
                    toggle(ui, &mut s.rules.battery_pause);
                });
                divider(ui);
                row(ui, "When locked", "", |ui| {
                    toggle(ui, &mut s.rules.lock_pause);
                });
                divider(ui);
                ui.add_space(2.0);
                ui.label(RichText::new("While these apps run").color(p.text));
                ui.add_space(2.0);
                let mut remove = None;
                let count = s.rules.app_pause.len();
                for (i, app) in s.rules.app_pause.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(app)
                                .desired_width(260.0)
                                .hint_text("Process name"),
                        );
                        if state.focus_new_app && i + 1 == count {
                            r.request_focus();
                            state.focus_new_app = false;
                        }
                        busy |= r.has_focus();
                        if widgets::icon_button(ui, "✖", "Remove application").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    s.rules.app_pause.remove(i);
                }
                if ui
                    .add(theme::secondary_button("Add application…"))
                    .clicked()
                {
                    s.rules.app_pause.push(String::new());
                    state.focus_new_app = true;
                    busy = true;
                }
            });

            section(ui, "Video", |ui| {
                row(ui, "Hardware decoding", "", |ui| {
                    toggle(ui, &mut s.video.hw_accel);
                });
                divider(ui);
                row(ui, "Default fit", "", |ui| {
                    egui::ComboBox::from_id_salt("scaler")
                        .selected_text(s.video.scaler.label())
                        .show_ui(ui, |ui| {
                            for sc in Scaler::ALL {
                                ui.selectable_value(&mut s.video.scaler, sc, sc.label());
                            }
                        });
                });
                divider(ui);
                row(ui, "Stream quality", "", |ui| {
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(s.video.stream_quality.label())
                        .show_ui(ui, |ui| {
                            for q in StreamQuality::ALL {
                                ui.selectable_value(&mut s.video.stream_quality, q, q.label());
                            }
                        });
                });
            });
        }
        Tab::Audio => {
            section(ui, "Audio", |ui| {
                row(ui, "Volume", "", |ui| {
                    let r = ui.add(egui::Slider::new(&mut s.volume, 0..=100).suffix("%"));
                    busy |= r.dragged();
                });
                divider(ui);
                row(ui, "Mute when a window is focused", "", |ui| {
                    toggle(ui, &mut s.audio_only_on_desktop);
                });
                divider(ui);
                row(ui, "Play sound on", "", |ui| {
                    let label = match &s.audio_output {
                        AudioOutput::All => "Every display".to_string(),
                        AudioOutput::Primary => "Primary display".to_string(),
                        AudioOutput::Display(id) => cx
                            .displays
                            .iter()
                            .find(|d| &d.id == id)
                            .map(|d| d.name.clone())
                            .unwrap_or_else(|| id.clone()),
                    };
                    egui::ComboBox::from_id_salt("audio_out")
                        .width(210.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut s.audio_output,
                                AudioOutput::All,
                                "Every display",
                            );
                            ui.selectable_value(
                                &mut s.audio_output,
                                AudioOutput::Primary,
                                "Primary display",
                            );
                            for d in cx.displays {
                                ui.selectable_value(
                                    &mut s.audio_output,
                                    AudioOutput::Display(d.id.clone()),
                                    &d.name,
                                );
                            }
                        });
                });
                divider(ui);
                row(ui, "Visualizer input", "", |ui| {
                    let current = s.audio_capture_device.clone().unwrap_or_default();
                    let label = cx
                        .devices
                        .iter()
                        .find(|d| d.id == current)
                        .map(|d| d.name.clone())
                        .unwrap_or_else(|| {
                            if current.is_empty() {
                                "System output".into()
                            } else {
                                current.clone()
                            }
                        });
                    egui::ComboBox::from_id_salt("audio_in")
                        .width(240.0)
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            if cx.devices.is_empty() {
                                ui.selectable_value(
                                    &mut s.audio_capture_device,
                                    None,
                                    "System output",
                                );
                            }
                            for d in cx.devices {
                                let value = if d.id.is_empty() {
                                    None
                                } else {
                                    Some(d.id.clone())
                                };
                                ui.selectable_value(&mut s.audio_capture_device, value, &d.name);
                            }
                        });
                });
            });
        }
        Tab::WallpaperEngine => {
            section(ui, "Steam", |ui| {
                let detected = cx.steam.and_then(|s| s.steam_dir.clone());
                let (label, help) = match (&s.wallpaper_engine.steam_dir, &detected) {
                    (Some(dir), _) => (
                        dir.to_string_lossy().into_owned(),
                        "Set here; the detected folder is not used".to_string(),
                    ),
                    (None, Some(dir)) => (
                        dir.to_string_lossy().into_owned(),
                        "Detected automatically".to_string(),
                    ),
                    (None, None) => (
                        "Not found".to_string(),
                        "Install Steam, or point at its folder (the one holding steamapps)"
                            .to_string(),
                    ),
                };
                row(ui, "Steam folder", &format!("{label}\n{help}"), |ui| {
                    if ui.add(theme::secondary_button("Change…")).clicked() {
                        let mut dialog = rfd::FileDialog::new();
                        if let Some(dir) =
                            s.wallpaper_engine.steam_dir.as_ref().or(detected.as_ref())
                        {
                            dialog = dialog.set_directory(dir);
                        }
                        if let Some(dir) = dialog.pick_folder() {
                            s.wallpaper_engine.steam_dir = Some(dir);
                        }
                    }
                    if s.wallpaper_engine.steam_dir.is_some()
                        && ui
                            .add(theme::secondary_button("Detect"))
                            .on_hover_text("Forget this folder and look for Steam again")
                            .clicked()
                    {
                        s.wallpaper_engine.steam_dir = None;
                    }
                });
                divider(ui);
                let detected_assets = cx
                    .steam
                    .filter(|s| !s.assets_overridden)
                    .and_then(|s| s.assets_dir.clone())
                    .or_else(|| {
                        cx.steam
                            .and_then(|s| s.install_dir.as_ref())
                            .map(|d| d.join("assets"))
                    });
                let (label, help) = match (&s.wallpaper_engine.assets_dir, &detected_assets) {
                    (Some(dir), _) => (
                        dir.to_string_lossy().into_owned(),
                        "Set here; scene wallpapers load their shaders, effects and stock textures from it".to_string(),
                    ),
                    (None, Some(dir)) => (
                        dir.to_string_lossy().into_owned(),
                        "From the Wallpaper Engine install; scene wallpapers load their shaders, effects and stock textures from it".to_string(),
                    ),
                    (None, None) => (
                        "Not found".to_string(),
                        "Scene wallpapers need Wallpaper Engine's assets folder: install Wallpaper Engine through Steam, or point at a copy of its assets folder".to_string(),
                    ),
                };
                row(
                    ui,
                    "Wallpaper Engine assets",
                    &format!("{label}\n{help}"),
                    |ui| {
                        if ui.add(theme::secondary_button("Change…")).clicked() {
                            let mut dialog = rfd::FileDialog::new();
                            if let Some(dir) = s
                                .wallpaper_engine
                                .assets_dir
                                .as_ref()
                                .or(detected_assets.as_ref())
                            {
                                dialog = dialog.set_directory(dir);
                            }
                            if let Some(dir) = dialog.pick_folder() {
                                s.wallpaper_engine.assets_dir = Some(dir);
                            }
                        }
                        if s.wallpaper_engine.assets_dir.is_some()
                            && ui
                                .add(theme::secondary_button("Detect"))
                                .on_hover_text(
                                    "Forget this folder and use the Wallpaper Engine install's",
                                )
                                .clicked()
                        {
                            s.wallpaper_engine.assets_dir = None;
                        }
                    },
                );
            });
            section(ui, "Workshop", |ui| {
                row(
                    ui,
                    "Add Steam downloads automatically",
                    "Every Workshop item Steam downloads, subscriptions made in Steam itself included, joins the library as soon as it lands, and entries are refreshed when Steam updates them",
                    |ui| {
                        toggle(ui, &mut s.wallpaper_engine.auto_import);
                    },
                );
            });
            section(ui, "Playback", |ui| {
                row(
                    ui,
                    "Frame rate limit",
                    "Handed to Wallpaper Engine wallpapers as their general FPS setting",
                    |ui| {
                        let r = ui.add(
                            egui::Slider::new(&mut s.wallpaper_engine.fps, 10..=240).suffix(" fps"),
                        );
                        busy |= r.dragged();
                    },
                );
                divider(ui);
                row(
                    ui,
                    "Media integration",
                    "Tell Wallpaper Engine wallpapers what the system is playing: title, artist, album art, playback state and position",
                    |ui| {
                        toggle(ui, &mut s.wallpaper_engine.media);
                    },
                );
            });
        }
        Tab::Advanced => {
            section(ui, "Window detection", |ui| {
                ui.add_enabled_ui(s.rules.fullscreen_pause, |ui| {
                    row(ui, "Window coverage", "", |ui| {
                        let mut percent = (s.rules.coverage * 100.0).round();
                        let r = ui.add(
                            egui::Slider::new(&mut percent, 50.0..=100.0)
                                .suffix("%")
                                .fixed_decimals(0),
                        );
                        busy |= r.dragged();
                        s.rules.coverage = percent / 100.0;
                    });
                    divider(ui);
                    row(ui, "Check every", "", |ui| {
                        let r = ui.add(
                            egui::Slider::new(&mut s.rules.interval_ms, 100..=5000)
                                .suffix(" ms")
                                .logarithmic(true),
                        );
                        busy |= r.dragged();
                    });
                    divider(ui);
                });
            });
            section(ui, "Loading", |ui| {
                row(ui, "Start timeout", "", |ui| {
                    let r = ui.add(
                        egui::Slider::new(&mut s.video.load_timeout_secs, 5..=120).suffix(" s"),
                    );
                    busy |= r.dragged();
                });
            });
            if cx.capabilities.web_devtools {
                section(ui, "Web", |ui| {
                    row(ui, "Developer tools", "", |ui| {
                        toggle(ui, &mut s.web.devtools);
                    });
                });
            }

            if cx.capabilities.pointer_motion || cx.capabilities.pointer_clicks {
                section(ui, "Input", |ui| {
                    row(ui, "Wallpaper mouse input", "", |ui| {
                        toggle(ui, &mut s.input.forward_mouse);
                    });
                    if cx.capabilities.global_pointer {
                        divider(ui);
                        ui.add_enabled_ui(s.input.forward_mouse, |ui| {
                            row(ui, "Also when a window is focused", "", |ui| {
                                toggle(ui, &mut s.input.always_move);
                            });
                        });
                    }
                });
            }
        }
    }

    busy
}

fn theme_label(t: Theme) -> &'static str {
    match t {
        Theme::System => "System",
        Theme::Light => "Light",
        Theme::Dark => "Dark",
    }
}
