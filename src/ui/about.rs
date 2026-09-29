use crate::ipc::Status;
use crate::model::Settings;
use crate::paths::Paths;
use crate::ui::{theme, widgets};
use eframe::egui::{self, RichText};

pub enum Action {
    OpenLibraryFolder,
    OpenLogFile,
    OpenSource,
}

pub struct View<'a> {
    pub status: Option<&'a Status>,
    pub settings: Option<&'a Settings>,
    pub paths: &'a Paths,
}

pub fn page(ui: &mut egui::Ui, v: &View<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let p = theme::palette(ui);
    theme::page_header(ui, "About", None, |_| {});
    ui.add_space(14.0);
    egui::ScrollArea::vertical().id_salt("about").auto_shrink([false; 2]).show(ui, |ui| {
        ui.set_max_width(760.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(theme::logo().fit_to_exact_size(egui::vec2(72.0, 72.0)));
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.label(RichText::new("Deadly Wallpaper").size(20.0).strong().color(p.text_strong));
                    ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(p.text_weak));
                });
            });
        });
        ui.add_space(14.0);
        theme::section_title(ui, "Desktop");
        ui.add_space(6.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match v.status {
                Some(s) => {
                    egui::Grid::new("about-desktop").num_columns(2).spacing([18.0, 8.0]).show(ui, |ui| {
                        let rows: [(&str, String); 5] = [
                            ("Presenter", presenter_label(&s.capabilities.presenter)),
                            ("Session", format!("{} on {}", s.session, s.platform)),
                            ("Window monitor", s.window_monitor.clone()),
                            ("Daemon", format!("deadlywp {}", s.version)),
                            ("Displays", s.displays.iter().enumerate().map(|(i, d)| format!("{}. {} ({}×{})", i + 1, d.name, d.rect.w, d.rect.h)).collect::<Vec<_>>().join("\n")),
                        ];
                        for (k, val) in rows {
                            ui.label(RichText::new(k).color(p.text_weak));
                            ui.add(egui::Label::new(RichText::new(val).color(p.text)).wrap());
                            ui.end_row();
                        }
                    });
                }
                None => {
                    theme::weak(ui, "Connecting…");
                }
            }
        });
        ui.add_space(14.0);
        theme::section_title(ui, "Files");
        ui.add_space(6.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let library = v.settings.map(|s| s.library_dir.clone()).unwrap_or_else(|| v.paths.default_library_dir());
            if file_row(ui, "Library", &library.display().to_string(), Some("Open")) {
                actions.push(Action::OpenLibraryFolder);
            }
            widgets::divider(ui);
            if file_row(ui, "Log", &v.paths.log_file().display().to_string(), Some("Open")) {
                actions.push(Action::OpenLogFile);
            }
            widgets::divider(ui);
            file_row(ui, "Configuration", &v.paths.config_dir.display().to_string(), None);
        });
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            if ui.link("GitHub").clicked() {
                actions.push(Action::OpenSource);
            }
            theme::weak(ui, "·");
            theme::weak(ui, "MIT license");
        });
        ui.add_space(12.0);
    });
    actions
}

fn presenter_label(presenter: &str) -> String {
    match presenter {
        "plasma" => "KDE Plasma wallpaper plugin".into(),
        "layer-shell" => "Wayland layer shell".into(),
        "x11" => "X11 keep-below windows".into(),
        "win32" => "Explorer WorkerW".into(),
        "quartz" => "macOS desktop-level windows".into(),
        other => other.to_string(),
    }
}

fn file_row(ui: &mut egui::Ui, name: &str, path: &str, button: Option<&str>) -> bool {
    let p = theme::palette(ui);
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        ui.add_sized([110.0, 20.0], egui::Label::new(RichText::new(name).color(p.text_weak)).halign(egui::Align::LEFT));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(b) = button {
                clicked = ui.add(theme::secondary_button(b)).clicked();
                ui.add_space(6.0);
            }
            ui.add(egui::Label::new(RichText::new(path).monospace().color(p.text)).truncate());
        });
    });
    clicked
}
