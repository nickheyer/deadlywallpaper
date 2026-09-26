use crate::ipc::{AudioDevice, Capabilities};
use crate::model::Display;
use crate::model::settings::{AudioOutput, PauseScope, Scaler, Settings, StreamQuality, Theme};
use crate::ui::theme;
use crate::ui::widgets::row;
use eframe::egui;

pub struct Context<'a> {
    pub devices: &'a [AudioDevice],
    pub displays: &'a [Display],
    pub capabilities: &'a Capabilities,
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    theme::section_title(ui, title);
    ui.add_space(4.0);
    theme::card(ui).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add(ui);
    });
    ui.add_space(14.0);
}

/// Settings form; returns true when a value changed and should be saved.
pub fn ui(ui: &mut egui::Ui, s: &mut Settings, cx: &Context<'_>) -> bool {
    let before = s.clone();
    let mut slider_active = false;

    section(ui, "General", |ui| {
        row(ui, "Start with the system", "Launch the wallpaper daemon when you log in", |ui| {
            ui.checkbox(&mut s.autostart, "");
        });
        row(ui, "Tray icon", "Pause, shuffle and open the app from the system tray", |ui| {
            ui.checkbox(&mut s.tray, "");
        });
        row(ui, "Appearance", "Colour scheme of this window and of web wallpapers", |ui| {
            egui::ComboBox::from_id_salt("theme").selected_text(theme_label(s.theme)).show_ui(ui, |ui| {
                for t in [Theme::System, Theme::Light, Theme::Dark] {
                    ui.selectable_value(&mut s.theme, t, theme_label(t));
                }
            });
        });
        let library_dir = s.library_dir.to_string_lossy().into_owned();
        row(ui, "Library folder", &library_dir, |ui| {
            if ui.button("Change…").clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_directory(&s.library_dir).pick_folder() {
                    s.library_dir = dir;
                }
            }
        });
        row(ui, "Copy media into the library", "Otherwise imported files are used where they are", |ui| {
            ui.checkbox(&mut s.copy_imports, "");
        });
        row(ui, "Generate thumbnails", "Capture a preview frame for imported and running wallpapers", |ui| {
            ui.checkbox(&mut s.thumbnails, "");
        });
    });

    section(ui, "Playback", |ui| {
        row(ui, "Pause under fullscreen or covering windows", "Saves power while a game or a maximized window hides the wallpaper", |ui| {
            ui.checkbox(&mut s.rules.fullscreen_pause, "");
        });
        row(ui, "Pause whenever an application is focused", "", |ui| {
            ui.add_enabled(s.rules.fullscreen_pause, egui::Checkbox::new(&mut s.rules.focus_pause, ""));
        });
        row(ui, "When one display is covered", "", |ui| {
            egui::ComboBox::from_id_salt("scope")
                .selected_text(match s.rules.scope {
                    PauseScope::Display => "Pause only that display",
                    PauseScope::All => "Pause every display",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.rules.scope, PauseScope::Display, "Pause only that display");
                    ui.selectable_value(&mut s.rules.scope, PauseScope::All, "Pause every display");
                });
        });
        row(ui, "Covered when windows hide", "Share of the work area that counts as covered", |ui| {
            let mut percent = (s.rules.coverage * 100.0).round();
            let r = ui.add(egui::Slider::new(&mut percent, 50.0..=100.0).suffix("%").fixed_decimals(0));
            slider_active |= r.dragged();
            s.rules.coverage = percent / 100.0;
        });
        row(ui, "Pause on battery", "", |ui| {
            ui.checkbox(&mut s.rules.battery_pause, "");
        });
        row(ui, "Pause while the session is locked", "", |ui| {
            ui.checkbox(&mut s.rules.lock_pause, "");
        });
        row(ui, "Window check interval", "How often window positions are re-evaluated", |ui| {
            let r = ui.add(egui::Slider::new(&mut s.rules.interval_ms, 100..=5000).suffix(" ms").logarithmic(true));
            slider_active |= r.dragged();
        });
        ui.add_space(6.0);
        ui.label("Pause while these applications run");
        theme::hint(ui, "Match on the window's application id or process name, for example steam or firefox");
        let mut remove = None;
        for (i, app) in s.rules.app_pause.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(app).desired_width(240.0).hint_text("application"));
                if ui.small_button("✖").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            s.rules.app_pause.remove(i);
        }
        if ui.small_button("＋ Add application").clicked() {
            s.rules.app_pause.push(String::new());
        }
    });

    section(ui, "Audio", |ui| {
        row(ui, "Volume", "", |ui| {
            let r = ui.add(egui::Slider::new(&mut s.volume, 0..=100).suffix("%"));
            slider_active |= r.dragged();
        });
        row(ui, "Sound only while on the desktop", "Mute while another application is focused", |ui| {
            ui.checkbox(&mut s.audio_only_on_desktop, "");
        });
        row(ui, "Play sound on", "", |ui| {
            let label = match &s.audio_output {
                AudioOutput::All => "Every display".to_string(),
                AudioOutput::Primary => "Primary display".to_string(),
                AudioOutput::Display(id) => cx.displays.iter().find(|d| &d.id == id).map(|d| d.name.clone()).unwrap_or_else(|| id.clone()),
            };
            egui::ComboBox::from_id_salt("audio_out").selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut s.audio_output, AudioOutput::All, "Every display");
                ui.selectable_value(&mut s.audio_output, AudioOutput::Primary, "Primary display");
                for d in cx.displays {
                    ui.selectable_value(&mut s.audio_output, AudioOutput::Display(d.id.clone()), &d.name);
                }
            });
        });
        row(ui, "Visualizer input", "Audio source analysed for web audio wallpapers", |ui| {
            let current = s.audio_capture_device.clone().unwrap_or_default();
            let label = cx.devices.iter().find(|d| d.id == current).map(|d| d.name.clone()).unwrap_or_else(|| if current.is_empty() { "System output".into() } else { current.clone() });
            egui::ComboBox::from_id_salt("audio_in").selected_text(label).width(240.0).show_ui(ui, |ui| {
                for d in cx.devices {
                    let value = if d.id.is_empty() { None } else { Some(d.id.clone()) };
                    ui.selectable_value(&mut s.audio_capture_device, value, &d.name);
                }
            });
        });
    });

    section(ui, "Video", |ui| {
        row(ui, "Hardware decoding", "", |ui| {
            ui.checkbox(&mut s.video.hw_accel, "");
        });
        row(ui, "Default fit", "How videos, GIFs and pictures fill the screen; adjustable per wallpaper", |ui| {
            egui::ComboBox::from_id_salt("scaler").selected_text(s.video.scaler.label()).show_ui(ui, |ui| {
                for sc in Scaler::ALL {
                    ui.selectable_value(&mut s.video.scaler, sc, sc.label());
                }
            });
        });
        row(ui, "Stream quality", "Maximum resolution for online video streams", |ui| {
            egui::ComboBox::from_id_salt("quality").selected_text(s.video.stream_quality.label()).show_ui(ui, |ui| {
                for q in StreamQuality::ALL {
                    ui.selectable_value(&mut s.video.stream_quality, q, q.label());
                }
            });
        });
        row(ui, "Start timeout", "Wallpapers that do not start in time are stopped", |ui| {
            let r = ui.add(egui::Slider::new(&mut s.video.load_timeout_secs, 5..=120).suffix(" s"));
            slider_active |= r.dragged();
        });
    });

    if cx.capabilities.web_devtools {
        section(ui, "Web", |ui| {
            row(ui, "Developer tools", "Allow inspecting web wallpapers", |ui| {
                ui.checkbox(&mut s.web.devtools, "");
            });
        });
    }

    if cx.capabilities.pointer_motion || cx.capabilities.pointer_clicks {
        section(ui, "Input", |ui| {
            let what = match (cx.capabilities.pointer_motion, cx.capabilities.pointer_clicks) {
                (true, true) => "Interactive wallpapers react to pointer movement and clicks on the desktop",
                (true, false) => "Interactive wallpapers react to pointer movement over the desktop",
                _ => "Interactive wallpapers react to clicks on the desktop",
            };
            row(ui, "Forward the pointer to wallpapers", what, |ui| {
                ui.checkbox(&mut s.input.forward_mouse, "");
            });
            if cx.capabilities.global_pointer {
                row(ui, "Keep tracking movement while another application is focused", "", |ui| {
                    ui.add_enabled(s.input.forward_mouse, egui::Checkbox::new(&mut s.input.always_move, ""));
                });
            }
        });
    }

    let editing_text = ui.memory(|m| m.focused()).is_some();
    *s != before && !slider_active && !editing_text
}

fn theme_label(t: Theme) -> &'static str {
    match t {
        Theme::System => "Follow the system",
        Theme::Light => "Light",
        Theme::Dark => "Dark",
    }
}
