use crate::ipc::{ActiveInfo, Request, Status};
use crate::model::{Arrangement, Display, Kind, Summary};
use crate::ui::{align, theme, widgets};
use eframe::egui::{self, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind};

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

pub fn page(ui: &mut egui::Ui, v: &View<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let status = v.status;
    let arrangement = status.layout.arrangement;
    let subtitle = match status.displays.len() {
        0 => "No displays".to_string(),
        1 => "1 display".to_string(),
        n => format!("{n} displays · {}", arrangement.label()),
    };
    theme::page_header(ui, "Screens", Some(&subtitle), |ui| {
        if status.displays.len() > 1 {
            let mut a = arrangement;
            if widgets::segmented(ui, "arrangement", &mut a, &[(Arrangement::Per, "Per display"), (Arrangement::Span, "Span"), (Arrangement::Duplicate, "Duplicate")]) {
                actions.push(Action::Send(Request::SetArrangement { arrangement: a, display: v.selected.map(str::to_owned) }));
            }
            if arrangement == Arrangement::Span && status.layout.is_aligned(&status.displays) && ui.add(theme::secondary_button("Reset")).clicked() {
                actions.push(Action::Send(Request::ResetAlignment));
            }
        }
    });
    ui.add_space(14.0);

    let all = arrangement != Arrangement::Per;
    let items: Vec<widgets::DiagramItem<'_>> = status
        .displays
        .iter()
        .map(|d| {
            let active = status.active.iter().find(|a| a.display == d.id);
            let s = active.and_then(|a| summary(v.library, &a.wallpaper));
            widgets::DiagramItem { display: d, thumbnail: s.and_then(|s| s.thumbnail.as_deref()), kind: active.map(|a| a.kind), paused: active.is_some_and(|a| a.paused) }
        })
        .collect();
    theme::card(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.vertical_centered(|ui| {
            let max = egui::vec2((ui.available_width() - 20.0).min(860.0), 300.0);
            if arrangement == Arrangement::Span {
                let shared = status.layout.shared.as_deref().and_then(|id| summary(v.library, id));
                let kind = shared.map(|s| s.kind);
                let thumbnail = shared.and_then(|s| s.thumbnail.as_deref()).map(widgets::thumbnail_uri);
                let scene = align::Scene {
                    displays: &status.displays,
                    layout: &status.layout,
                    thumbnail: thumbnail.as_deref(),
                    kind,
                    selected: v.selected,
                    scalable: kind != Some(Kind::Program),
                    rotatable: match kind {
                        None => true,
                        Some(Kind::Program) => false,
                        Some(k) if k.is_web() => status.capabilities.rotate_web,
                        Some(_) => true,
                    },
                };
                match align::editor(ui, &scene, max) {
                    Some(align::Event::Clicked(id)) => actions.push(Action::Select(id)),
                    Some(align::Event::Image(pose)) => actions.push(Action::Send(Request::AlignImage { pose })),
                    Some(align::Event::Display { id, pose }) => actions.push(Action::Send(Request::AlignDisplay { display: id, pose })),
                    None => {}
                }
            } else if let Some(id) = widgets::display_diagram(ui, &items, v.selected, all, max) {
                actions.push(Action::Select(id));
            }
        });
    });
    ui.add_space(14.0);

    if all {
        let active = status.active.first();
        let name = if arrangement == Arrangement::Span { "Every display, spanning" } else { "Every display" };
        let detail = format!("{} displays", status.displays.len());
        display_card(ui, DisplayCard { key: "shared", name, detail: &detail, display: None, active, library: v.library, selected: true, all: true }, &mut actions);
    } else {
        for (i, d) in status.displays.iter().enumerate() {
            let active = status.active.iter().find(|a| a.display == d.id);
            let selected = v.selected == Some(d.id.as_str());
            let name = format!("{}  {}", i + 1, d.name);
            let detail = format!("{}×{}{}", d.rect.w, d.rect.h, if d.primary { " · primary" } else { "" });
            display_card(ui, DisplayCard { key: &d.id, name: &name, detail: &detail, display: Some(d), active, library: v.library, selected, all: false }, &mut actions);
            ui.add_space(10.0);
        }
    }
    actions
}

struct DisplayCard<'a> {
    key: &'a str,
    name: &'a str,
    detail: &'a str,
    display: Option<&'a Display>,
    active: Option<&'a ActiveInfo>,
    library: &'a [Summary],
    selected: bool,
    all: bool,
}

fn display_card(ui: &mut egui::Ui, c: DisplayCard<'_>, actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    let stroke = if c.selected { Stroke::new(1.5, p.accent) } else { Stroke::new(1.0, p.stroke) };
    let frame = theme::card(ui).stroke(stroke);
    let inner = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            let thumb_size = egui::vec2(176.0, 99.0);
            let (rect, _) = ui.allocate_exact_size(thumb_size, Sense::hover());
            match c.active {
                Some(a) => {
                    let s = summary(c.library, &a.wallpaper);
                    let uri = s.and_then(|s| s.thumbnail.as_deref()).map(widgets::thumbnail_uri);
                    widgets::thumbnail(ui, rect, uri.as_deref(), a.kind, CornerRadius::same(8));
                    if a.paused {
                        ui.painter().rect_filled(rect, CornerRadius::same(8), egui::Color32::from_black_alpha(90));
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "⏸", egui::FontId::proportional(26.0), egui::Color32::from_white_alpha(230));
                    }
                }
                None => {
                    ui.painter().rect(rect, CornerRadius::same(8), p.control, Stroke::new(1.0, p.stroke), StrokeKind::Inside);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Nothing playing", egui::FontId::proportional(12.5), p.text_faint);
                }
            }
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(c.name).size(16.0).strong().color(p.text_strong));
                    ui.label(RichText::new(c.detail).small().color(p.text_weak));
                    if c.selected && !c.all {
                        ui.label(RichText::new(" Selected ").small().color(p.accent).background_color(p.accent_soft));
                    }
                });
                if let Some(a) = c.active {
                    ui.label(RichText::new(&a.title).size(14.5).color(p.text));
                    let state = if !a.loaded {
                        "starting…"
                    } else if a.paused {
                        "paused"
                    } else {
                        "playing"
                    };
                    let volume = if a.kind.has_audio() && a.volume > 0 { format!(" · {}%", a.volume) } else { String::new() };
                    theme::hint(ui, &format!("{} · {state}{volume}", a.kind.label()));
                }
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let target = c.display.map(|d| d.id.clone());
                    if let Some(a) = c.active {
                        if a.kind.has_timeline() && ui.add(theme::secondary_button("⏮  Restart")).clicked() {
                            actions.push(Action::Send(Request::Seek { display: target.clone(), value: "0".into() }));
                        }
                        if ui.add(theme::secondary_button("↻  Reload")).clicked() {
                            actions.push(Action::Send(Request::Set { target: "reload".into(), display: target.clone() }));
                        }
                        if ui.add(theme::secondary_button("✖  Close")).clicked() {
                            actions.push(Action::Send(Request::Close { display: if c.all { None } else { target.clone() } }));
                        }
                    }
                    if ui.add(theme::secondary_button("🔀  Shuffle")).clicked() {
                        actions.push(Action::Send(Request::Set { target: "random".into(), display: target.clone() }));
                    }
                    if ui.add(theme::secondary_button("Choose…")).clicked() {
                        if let Some(d) = c.display {
                            actions.push(Action::Select(d.id.clone()));
                        }
                        actions.push(Action::GoToLibrary);
                    }
                });
            });
        });
    });
    if let Some(d) = c.display {
        let header = Rect::from_min_size(inner.response.rect.min, egui::vec2(inner.response.rect.width(), 40.0));
        let r = ui.interact(header, ui.id().with(("select-display", c.key)), Sense::click());
        if r.clicked() && !c.selected {
            actions.push(Action::Select(d.id.clone()));
        }
    }
}
