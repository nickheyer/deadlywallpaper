use crate::ipc::Status;
use crate::model::Settings;
use crate::paths::Paths;
use crate::ui::{theme, widgets};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

pub enum Action {
    Open(PathBuf),
    CopiedDetails,
}

pub struct View<'a> {
    pub status: Option<&'a Status>,
    pub settings: Option<&'a Settings>,
    pub paths: &'a Paths,
}

pub fn show(ctx: &egui::Context, v: &View<'_>, open: &mut bool) -> Vec<Action> {
    let mut actions = Vec::new();
    if !*open {
        return actions;
    }
    let modal = egui::Modal::new(egui::Id::new("about"))
        .frame(theme::dialog_frame(ctx))
        .show(ctx, |ui| {
            let p = theme::palette(ui);
            ui.set_width(440.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.horizontal(|ui| {
                theme::logo(ui, 56.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(
                        RichText::new("Deadly Wallpaper")
                            .size(22.0)
                            .strong()
                            .color(p.text_strong),
                    );
                    theme::weak(ui, &format!("Version {}", env!("CARGO_PKG_VERSION")));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::icon_button(ui, "✖", "Close (Esc)").clicked() {
                        *open = false;
                    }
                });
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.hyperlink_to(
                    "Source code",
                    "https://github.com/nickheyer/deadlywallpaper",
                );
                theme::weak(ui, "·");
                ui.hyperlink_to(
                    "MIT license",
                    "https://github.com/nickheyer/deadlywallpaper/blob/HEAD/LICENSE",
                );
            });
            ui.add_space(12.0);
            widgets::divider(ui);
            ui.add_space(12.0);
            let library = v
                .settings
                .map(|s| s.library_dir.clone())
                .unwrap_or_else(|| v.paths.default_library_dir());
            ui.horizontal_wrapped(|ui| {
                for (label, path) in [
                    ("Library folder", library),
                    ("Open log", v.paths.log_file()),
                    ("Settings folder", v.paths.config_dir.clone()),
                ] {
                    if ui
                        .add(theme::secondary_button(label))
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        actions.push(Action::Open(path));
                    }
                }
            });
            ui.add_space(12.0);
            egui::CollapsingHeader::new("Technical details").show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 340.0).max(100.0))
                    .show(ui, |ui| match v.status {
                        Some(s) => {
                            egui::Grid::new("about-details")
                                .num_columns(2)
                                .spacing([16.0, 8.0])
                                .max_col_width(280.0)
                                .show(ui, |ui| {
                                    let mut rows = vec![
                                        ("System", format!("{} / {}", s.platform, s.session)),
                                        ("Renderer", s.capabilities.presenter.clone()),
                                        ("Window tracking", s.window_monitor.clone()),
                                    ];
                                    if s.version != env!("CARGO_PKG_VERSION") {
                                        rows.push(("Daemon version", s.version.clone()));
                                    }
                                    for (label, value) in rows {
                                        theme::weak(ui, label);
                                        ui.add(egui::Label::new(value).selectable(true).wrap());
                                        ui.end_row();
                                    }
                                    for (i, display) in s.displays.iter().enumerate() {
                                        theme::weak(ui, &format!("Display {}", i + 1));
                                        ui.add(
                                            egui::Label::new(format!(
                                                "{}\n{} × {}",
                                                display.name, display.rect.w, display.rect.h
                                            ))
                                            .selectable(true)
                                            .wrap(),
                                        );
                                        ui.end_row();
                                    }
                                });
                        }
                        None => theme::weak(ui, "Not connected"),
                    });
                ui.add_space(10.0);
                if ui.add(theme::secondary_button("Copy details")).clicked() {
                    let mut details = format!("Deadly Wallpaper {}", env!("CARGO_PKG_VERSION"));
                    if let Some(status) = v.status {
                        details.push_str(&format!(
                            "\nDaemon: {}\nSystem: {} / {}\nRenderer: {}\nWindow tracking: {}",
                            status.version,
                            status.platform,
                            status.session,
                            status.capabilities.presenter,
                            status.window_monitor
                        ));
                        for (i, display) in status.displays.iter().enumerate() {
                            details.push_str(&format!(
                                "\nDisplay {}: {} ({} × {})",
                                i + 1,
                                display.name,
                                display.rect.w,
                                display.rect.h
                            ));
                        }
                    }
                    ui.ctx().copy_text(details);
                    actions.push(Action::CopiedDetails);
                }
            });
        });
    if modal.should_close() {
        *open = false;
    }
    actions
}
