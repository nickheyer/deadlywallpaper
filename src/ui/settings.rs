use eframe::egui;
use crate::ipc::AudioDevice;
use crate::model::Display;
use crate::model::settings::{AudioOutput, PauseScope, Scaler, Settings, StreamQuality, Theme};
use crate::ui::widgets::{row, section};

/// Settings form; returns true when a value changed and should be saved.
pub fn ui(ui: &mut egui::Ui, s: &mut Settings, devices: &[AudioDevice], displays: &[Display]) -> bool {
    let before = s.clone();
    let mut slider_active = false;

    section(ui, "General");
    row(ui, "Start with the system", "Launch the wallpaper daemon at login", |ui| {
        ui.checkbox(&mut s.autostart, "");
    });
    row(ui, "Tray icon", "", |ui| {
        ui.checkbox(&mut s.tray, "");
    });
    row(ui, "Theme", "Control window and web wallpaper color scheme", |ui| {
        egui::ComboBox::from_id_salt("theme").selected_text(format!("{:?}", s.theme)).show_ui(ui, |ui| {
            for t in [Theme::System, Theme::Light, Theme::Dark] {
                ui.selectable_value(&mut s.theme, t, format!("{t:?}"));
            }
        });
    });
    row(ui, "Library folder", "Where imported wallpapers are stored", |ui| {
        if ui.button("Change…").clicked() {
            if let Some(dir) = rfd::FileDialog::new().set_directory(&s.library_dir).pick_folder() {
                s.library_dir = dir;
            }
        }
        ui.label(egui::RichText::new(s.library_dir.to_string_lossy()).weak().small());
    });
    row(ui, "Copy media into the library", "Otherwise imported files are referenced where they are", |ui| {
        ui.checkbox(&mut s.copy_imports, "");
    });
    row(ui, "Generate thumbnails", "", |ui| {
        ui.checkbox(&mut s.thumbnails, "");
    });

    section(ui, "Audio");
    row(ui, "Volume", "", |ui| {
        let r = ui.add(egui::Slider::new(&mut s.volume, 0..=100));
        slider_active |= r.dragged();
    });
    row(ui, "Audio only while on the desktop", "Mute while another application is focused", |ui| {
        ui.checkbox(&mut s.audio_only_on_desktop, "");
    });
    row(ui, "Audio output", "Which display's wallpaper plays sound", |ui| {
        let label = match &s.audio_output {
            AudioOutput::All => "Every display".to_string(),
            AudioOutput::Primary => "Primary display".to_string(),
            AudioOutput::Display(id) => displays.iter().find(|d| &d.id == id).map(|d| d.name.clone()).unwrap_or_else(|| id.clone()),
        };
        egui::ComboBox::from_id_salt("audio_out").selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut s.audio_output, AudioOutput::All, "Every display");
            ui.selectable_value(&mut s.audio_output, AudioOutput::Primary, "Primary display");
            for d in displays {
                ui.selectable_value(&mut s.audio_output, AudioOutput::Display(d.id.clone()), &d.name);
            }
        });
    });
    row(ui, "Visualizer input", "Audio source analysed for web audio wallpapers", |ui| {
        let current = s.audio_capture_device.clone().unwrap_or_default();
        let label = devices.iter().find(|d| d.id == current).map(|d| d.name.clone()).unwrap_or_else(|| if current.is_empty() { "System output".into() } else { current.clone() });
        egui::ComboBox::from_id_salt("audio_in").selected_text(label).width(200.0).show_ui(ui, |ui| {
            for d in devices {
                let value = if d.id.is_empty() { None } else { Some(d.id.clone()) };
                ui.selectable_value(&mut s.audio_capture_device, value, &d.name);
            }
        });
    });

    section(ui, "Playback rules");
    row(ui, "Pause under fullscreen or covering windows", "", |ui| {
        ui.checkbox(&mut s.rules.fullscreen_pause, "");
    });
    row(ui, "Pause whenever an application is focused", "", |ui| {
        ui.add_enabled(s.rules.fullscreen_pause, egui::Checkbox::new(&mut s.rules.focus_pause, ""));
    });
    row(ui, "Scope", "Pause only the covered display, or all of them", |ui| {
        egui::ComboBox::from_id_salt("scope").selected_text(match s.rules.scope {
            PauseScope::Display => "Per display",
            PauseScope::All => "All displays",
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut s.rules.scope, PauseScope::Display, "Per display");
            ui.selectable_value(&mut s.rules.scope, PauseScope::All, "All displays");
        });
    });
    row(ui, "Covered threshold", "Fraction of the work area windows must cover", |ui| {
        let r = ui.add(egui::Slider::new(&mut s.rules.coverage, 0.5..=1.0).fixed_decimals(2));
        slider_active |= r.dragged();
    });
    row(ui, "Pause on battery", "", |ui| {
        ui.checkbox(&mut s.rules.battery_pause, "");
    });
    row(ui, "Pause while the session is locked", "", |ui| {
        ui.checkbox(&mut s.rules.lock_pause, "");
    });
    row(ui, "Check interval (ms)", "How often window state is evaluated", |ui| {
        let r = ui.add(egui::Slider::new(&mut s.rules.interval_ms, 100..=5000).logarithmic(true));
        slider_active |= r.dragged();
    });
    ui.add_space(4.0);
    ui.label("Pause while these applications run:");
    let mut remove = None;
    for (i, app) in s.rules.app_pause.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(app).desired_width(220.0).hint_text("process or app id"));
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

    section(ui, "Video");
    row(ui, "Hardware decoding", "", |ui| {
        ui.checkbox(&mut s.video.hw_accel, "");
    });
    row(ui, "Default fit", "Scaling for video, GIF and picture wallpapers", |ui| {
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
    row(ui, "Load timeout (s)", "Wallpapers that do not start in time are stopped", |ui| {
        let r = ui.add(egui::Slider::new(&mut s.video.load_timeout_secs, 5..=120));
        slider_active |= r.dragged();
    });

    section(ui, "Web");
    row(ui, "Developer tools", "Allow inspecting web wallpapers", |ui| {
        ui.checkbox(&mut s.web.devtools, "");
    });

    section(ui, "Input");
    row(ui, "Forward mouse to wallpapers", "Interactive wallpapers react to the pointer", |ui| {
        ui.checkbox(&mut s.input.forward_mouse, "");
    });
    row(ui, "Keep tracking motion while apps are focused", "", |ui| {
        ui.add_enabled(s.input.forward_mouse, egui::Checkbox::new(&mut s.input.always_move, ""));
    });

    let editing_text = ui.memory(|m| m.focused()).is_some();
    *s != before && !slider_active && !editing_text
}
