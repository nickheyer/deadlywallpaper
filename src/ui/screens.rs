//! The Screens page: what plays where, arrangement, and per-display controls.

use crate::ipc::{ActiveInfo, Request, Status};
use crate::model::{Arrangement, Summary};
use crate::ui::theme;
use crate::ui::widgets::{display_diagram, segmented};
use eframe::egui::{self, RichText};

pub enum Action {
    Select(String),
    Send(Request),
    GoToLibrary,
}

pub struct View<'a> {
    pub status: &'a Status,
    pub library: &'a [Summary],
    pub selected: Option<&'a str>,
}

fn thumbnails(status: &Status, library: &[Summary]) -> Vec<(String, Option<std::path::PathBuf>)> {
    status
        .active
        .iter()
        .map(|a| (a.display.clone(), library.iter().find(|w| w.id == a.wallpaper).and_then(|w| w.thumbnail.clone())))
        .collect()
}

pub fn page(ui: &mut egui::Ui, v: &View<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let status = v.status;
    let arrangement = status.layout.arrangement;
    ui.horizontal(|ui| {
        theme::page_title(ui, "Screens");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut a = arrangement;
            if segmented(ui, &mut a, &[(Arrangement::Per, "Per display"), (Arrangement::Span, "Span"), (Arrangement::Duplicate, "Duplicate")]) {
                actions.push(Action::Send(Request::SetArrangement { arrangement: a, display: v.selected.map(str::to_owned) }));
            }
            ui.label(RichText::new("Arrangement").color(ui.visuals().weak_text_color()));
        });
    });
    theme::hint(
        ui,
        match arrangement {
            Arrangement::Per => "Each display shows its own wallpaper. Select a display, then pick a wallpaper in the library.",
            Arrangement::Span => "One wallpaper stretches across every display.",
            Arrangement::Duplicate => "The same wallpaper plays on every display; only the primary display carries its sound.",
        },
    );
    ui.add_space(10.0);

    let thumbs = thumbnails(status, v.library);
    let all = arrangement != Arrangement::Per;
    theme::card(ui).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            let max = egui::vec2((ui.available_width() - 40.0).min(820.0), 250.0);
            if let Some(id) = display_diagram(ui, &status.displays, v.selected, all, &thumbs, max) {
                actions.push(Action::Select(id));
            }
        });
    });
    ui.add_space(12.0);

    let Some(display) = v.selected.and_then(|id| status.displays.iter().find(|d| d.id == id)).or_else(|| crate::model::display::primary(&status.displays)) else {
        return actions;
    };
    let index = crate::model::display::index_of(&status.displays, &display.id).unwrap_or(0);
    let active: Option<&ActiveInfo> = status.active.iter().find(|a| a.display == display.id);
    theme::card(ui).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(if all { "Every display".to_string() } else { format!("Display {index} · {}", display.name) }).size(17.0).strong());
                theme::hint(ui, &format!("{}×{}{}", display.rect.w, display.rect.h, if display.primary { " · primary" } else { "" }));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Choose from library…").clicked() {
                    actions.push(Action::GoToLibrary);
                }
                if ui.button("🔀 Shuffle").on_hover_text("Play a random wallpaper from the library here").clicked() {
                    actions.push(Action::Send(Request::Set { target: "random".into(), display: Some(display.id.clone()) }));
                }
            });
        });
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        match active {
            None => {
                ui.label(RichText::new("Nothing is playing here.").color(ui.visuals().weak_text_color()));
            }
            Some(a) => {
                let summary = v.library.iter().find(|w| w.id == a.wallpaper);
                ui.horizontal(|ui| {
                    let thumb_size = egui::vec2(160.0, 90.0);
                    match summary.and_then(|w| w.thumbnail.clone()) {
                        Some(path) => {
                            ui.add(egui::Image::from_uri(format!("file://{}", path.display())).fit_to_exact_size(thumb_size).corner_radius(egui::CornerRadius::same(8)));
                        }
                        None => {
                            let (rect, _) = ui.allocate_exact_size(thumb_size, egui::Sense::hover());
                            ui.painter().rect_filled(rect, egui::CornerRadius::same(8), theme::kind_color(a.kind));
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, theme::kind_glyph(a.kind), egui::FontId::proportional(34.0), egui::Color32::from_white_alpha(200));
                        }
                    }
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&a.title).size(16.0).strong());
                        let state = if !a.loaded {
                            "starting…"
                        } else if a.paused {
                            "paused"
                        } else {
                            "playing"
                        };
                        theme::hint(ui, &format!("{} · {state}{}", a.kind.label(), if a.volume > 0 { format!(" · volume {}%", a.volume) } else { String::new() }));
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if a.kind.is_media() && ui.button("⏮ Restart").on_hover_text("Play from the beginning").clicked() {
                                actions.push(Action::Send(Request::Seek { display: Some(display.id.clone()), value: "0".into() }));
                            }
                            if ui.button("↻ Reload").on_hover_text("Restart this wallpaper").clicked() {
                                actions.push(Action::Send(Request::Set { target: "reload".into(), display: Some(display.id.clone()) }));
                            }
                            if ui.button("✖ Close").on_hover_text("Stop this wallpaper and give the desktop back its own background").clicked() {
                                actions.push(Action::Send(Request::Close { display: if all { None } else { Some(display.id.clone()) } }));
                            }
                        });
                    });
                });
            }
        }
    });
    actions
}
