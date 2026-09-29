use crate::ipc::{ActiveInfo, Request, Status};
use crate::model::{Arrangement, Kind, Summary};
use crate::ui::{align, theme, widgets};
use eframe::egui::{self, CornerRadius, RichText, Sense};

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

fn summary<'a>(library: &'a [Summary], id: &str) -> Option<&'a Summary> {
    library.iter().find(|w| w.id == id)
}

pub fn page(
    ui: &mut egui::Ui,
    v: &View<'_>,
    adjustments: impl FnOnce(&mut egui::Ui),
) -> Vec<Action> {
    let mut actions = Vec::new();
    theme::page_header(ui, "Screens", None, |_| {});
    ui.add_space(12.0);
    if v.status.displays.is_empty() {
        theme::weak(ui, "No displays connected");
        return actions;
    }
    if v.status.displays.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            ui.label("Wallpaper mode");
            let mut arrangement = v.status.layout.arrangement;
            if widgets::segmented(
                ui,
                "arrangement",
                &mut arrangement,
                &[
                    (Arrangement::Per, "Per display"),
                    (Arrangement::Span, "Span"),
                    (Arrangement::Duplicate, "Duplicate"),
                ],
            ) {
                actions.push(Action::Send(Request::SetArrangement {
                    arrangement,
                    display: v.selected.map(str::to_owned),
                }));
            }
        });
        ui.add_space(14.0);
    }
    let width = ui.available_width();
    let height = ui.available_height();
    if width >= 900.0 {
        let inspector_width = (width * 0.3).clamp(320.0, 400.0);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 24.0;
            ui.allocate_ui_with_layout(
                egui::vec2(width - inspector_width - 24.0, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("screen-layout")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            layout(ui, v, (height - 150.0).clamp(240.0, 680.0), &mut actions);
                        });
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(inspector_width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("screen-inspector")
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            wallpaper(ui, v, &mut actions);
                            adjustments(ui);
                        });
                },
            );
        });
    } else {
        egui::ScrollArea::vertical()
            .id_salt("screens-stacked")
            .auto_shrink([false, true])
            .show(ui, |ui| {
                layout(ui, v, (width * 0.5).clamp(220.0, 360.0), &mut actions);
                ui.add_space(18.0);
                wallpaper(ui, v, &mut actions);
                adjustments(ui);
            });
    }
    actions
}

fn layout(ui: &mut egui::Ui, v: &View<'_>, height: f32, actions: &mut Vec<Action>) {
    let status = v.status;
    let spanning = status.layout.arrangement == Arrangement::Span;
    theme::card_compact(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            theme::section_title(ui, "Display layout");
            if spanning {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            status.layout.is_aligned(&status.displays),
                            theme::secondary_button("Reset alignment"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Send(Request::ResetAlignment));
                    }
                });
            }
        });
        ui.add_space(8.0);
        ui.vertical_centered(|ui| {
            let max = egui::vec2(ui.available_width() - 8.0, height);
            if spanning {
                let shared = status
                    .layout
                    .shared
                    .as_deref()
                    .and_then(|id| summary(v.library, id));
                let kind = shared.map(|s| s.kind);
                let thumbnail = shared
                    .and_then(|s| s.thumbnail.as_deref())
                    .map(widgets::thumbnail_uri);
                let scene = align::Scene {
                    displays: &status.displays,
                    layout: &status.layout,
                    thumbnail: thumbnail.as_deref(),
                    kind,
                    selected: v.selected,
                    scalable: kind != Some(Kind::Program),
                    rotatable: match kind {
                        Some(Kind::Program) => false,
                        Some(k) if k.is_web() => status.capabilities.rotate_web,
                        _ => true,
                    },
                };
                match align::editor(ui, &scene, max) {
                    Some(align::Event::Clicked(id)) => actions.push(Action::Select(id)),
                    Some(align::Event::Image(pose)) => {
                        actions.push(Action::Send(Request::AlignImage { pose }))
                    }
                    Some(align::Event::Display { id, pose }) => {
                        actions.push(Action::Send(Request::AlignDisplay { display: id, pose }))
                    }
                    None => {}
                }
            } else {
                let items: Vec<_> = status
                    .displays
                    .iter()
                    .map(|d| {
                        let active = status.active.iter().find(|a| a.display == d.id);
                        let s = active.and_then(|a| summary(v.library, &a.wallpaper));
                        widgets::DiagramItem {
                            display: d,
                            thumbnail: s.and_then(|s| s.thumbnail.as_deref()),
                            kind: active.map(|a| a.kind),
                            paused: active.is_some_and(|a| a.paused && a.kind != Kind::Picture),
                        }
                    })
                    .collect();
                if let Some(id) = widgets::display_diagram(ui, &items, v.selected, false, max) {
                    actions.push(Action::Select(id));
                }
            }
        });
        if spanning {
            theme::hint(
                ui,
                "Drag a display or the wallpaper to align it. Use the handles to resize or rotate.",
            );
        }
    });
    ui.add_space(8.0);
    for (i, display) in status.displays.iter().enumerate() {
        let label = format!("{}  {}", i + 1, display.name);
        ui.horizontal(|ui| {
            let selected = v.selected == Some(display.id.as_str());
            if ui.selectable_label(selected, label).clicked() {
                actions.push(Action::Select(display.id.clone()));
            }
            theme::hint(
                ui,
                &format!(
                    "{} × {}{}",
                    display.rect.w,
                    display.rect.h,
                    if display.primary { " · Primary" } else { "" }
                ),
            );
        });
    }
}

fn wallpaper(ui: &mut egui::Ui, v: &View<'_>, actions: &mut Vec<Action>) {
    let shared = v.status.layout.arrangement != Arrangement::Per;
    let display = v
        .status
        .displays
        .iter()
        .find(|d| Some(d.id.as_str()) == v.selected);
    let target = if shared {
        None
    } else {
        display.map(|d| d.id.clone())
    };
    let active = v
        .status
        .active
        .iter()
        .find(|a| shared || Some(a.display.as_str()) == v.selected);
    theme::section_title(ui, "Wallpaper");
    theme::hint(
        ui,
        if shared {
            "All displays"
        } else {
            display.map_or("Selected display", |d| d.name.as_str())
        },
    );
    ui.add_space(8.0);
    if let Some(active) = active {
        let item = summary(v.library, &active.wallpaper);
        let uri = item
            .and_then(|s| s.thumbnail.as_deref())
            .map(widgets::thumbnail_uri);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(112.0, 63.0), Sense::hover());
            widgets::thumbnail(ui, rect, uri.as_deref(), active.kind, CornerRadius::same(6));
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(&active.title).strong()).truncate())
                    .on_hover_text(&active.title);
                let state = if !active.loaded {
                    "Loading…"
                } else if active.kind == Kind::Picture {
                    "Picture"
                } else if active.paused {
                    "Paused"
                } else {
                    "Playing"
                };
                theme::hint(ui, state);
            });
        });
    } else {
        theme::weak(ui, "No wallpaper set");
    }
    ui.add_space(8.0);
    if ui
        .add(theme::primary(if active.is_some() {
            "Change wallpaper…"
        } else {
            "Choose wallpaper…"
        }))
        .clicked()
    {
        actions.push(Action::GoToLibrary);
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!v.library.is_empty(), theme::secondary_button("Shuffle"))
            .clicked()
        {
            actions.push(Action::Send(Request::Set {
                target: "random".into(),
                display: target.clone(),
            }));
        }
        if let Some(active) = active {
            ui.menu_button("More…", |ui| wallpaper_menu(ui, active, target, actions));
        }
    });
    ui.add_space(12.0);
    widgets::divider(ui);
    ui.add_space(12.0);
}

fn wallpaper_menu(
    ui: &mut egui::Ui,
    active: &ActiveInfo,
    target: Option<String>,
    actions: &mut Vec<Action>,
) {
    if active.kind.has_timeline() && ui.button("Restart").clicked() {
        actions.push(Action::Send(Request::Seek {
            display: target.clone(),
            value: "0".into(),
        }));
        ui.close();
    }
    if ui.button("Reload").clicked() {
        actions.push(Action::Send(Request::Set {
            target: "reload".into(),
            display: target.clone(),
        }));
        ui.close();
    }
    ui.separator();
    if ui.button("Remove wallpaper").clicked() {
        actions.push(Action::Send(Request::Close { display: target }));
        ui.close();
    }
}
