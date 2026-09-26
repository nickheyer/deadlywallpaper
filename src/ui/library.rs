use crate::ipc::ActiveInfo;
use crate::model::{Arrangement, Display, Kind, Summary};
use crate::ui::theme;
use crate::ui::widgets::chip;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind};
use std::path::PathBuf;

pub enum Action {
    Apply { id: String, display: Option<String> },
    Customize { id: String },
    Edit { id: String },
    Export { id: String },
    Reveal { path: PathBuf },
    Thumbnail { id: String },
    Delete { id: String },
    Open { url: String },
    PickFiles,
}

/// Library filters kept by the app between frames.
#[derive(Default)]
pub struct Filter {
    pub kind: Option<Kind>,
}

pub struct Grid<'a> {
    pub items: &'a [Summary],
    pub search: &'a str,
    pub filter: &'a Filter,
    pub displays: &'a [Display],
    pub active: &'a [ActiveInfo],
    pub arrangement: Arrangement,
    pub selected_display: Option<&'a str>,
    pub hovering_files: bool,
    pub connected: bool,
}

const CARD_W: f32 = 232.0;
const IMAGE_H: f32 = 130.0;
const CARD_H: f32 = IMAGE_H + 58.0;

const KIND_FILTERS: [(Option<Kind>, &str); 7] = [
    (None, "All"),
    (Some(Kind::Video), "Videos"),
    (Some(Kind::Web), "Web"),
    (Some(Kind::Picture), "Pictures"),
    (Some(Kind::Gif), "GIFs"),
    (Some(Kind::VideoStream), "Streams"),
    (Some(Kind::Program), "Programs"),
];

fn matches_filter(w: &Summary, filter: &Filter) -> bool {
    match filter.kind {
        None => true,
        Some(Kind::Web) => w.kind.is_web(),
        Some(k) => w.kind == k,
    }
}

/// Filter chips; returns the new kind filter when it changed.
pub fn filter_bar(ui: &mut egui::Ui, filter: &Filter, items: &[Summary]) -> Option<Option<Kind>> {
    let mut chosen = None;
    ui.horizontal_wrapped(|ui| {
        for (kind, label) in KIND_FILTERS {
            let count = items.iter().filter(|w| matches_filter(w, &Filter { kind })).count();
            if kind.is_some() && count == 0 {
                continue;
            }
            if chip(ui, filter.kind == kind, format!("{label} · {count}")).clicked() {
                chosen = Some(kind);
            }
        }
    });
    chosen
}

pub fn grid(ui: &mut egui::Ui, g: &Grid<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let needle = g.search.trim().to_lowercase();
    let items: Vec<&Summary> = g
        .items
        .iter()
        .filter(|w| matches_filter(w, g.filter))
        .filter(|w| needle.is_empty() || w.title.to_lowercase().contains(&needle) || w.author.as_deref().is_some_and(|a| a.to_lowercase().contains(&needle)))
        .collect();
    if g.hovering_files {
        drop_target(ui);
        return actions;
    }
    if items.is_empty() {
        empty_state(ui, g, &mut actions);
        return actions;
    }
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(14.0, 14.0);
            for w in items {
                card(ui, g, w, &mut actions);
            }
        });
        ui.add_space(16.0);
    });
    actions
}

fn drop_target(ui: &mut egui::Ui) {
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    painter.rect(rect.shrink(6.0), CornerRadius::same(12), theme::accent(ui).gamma_multiply(0.12), Stroke::new(2.0, theme::accent(ui)), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, "Drop to add to the library", FontId::proportional(22.0), ui.visuals().strong_text_color());
    ui.allocate_rect(rect, Sense::hover());
}

fn empty_state(ui: &mut egui::Ui, g: &Grid<'_>, actions: &mut Vec<Action>) {
    ui.add_space(ui.available_height() * 0.25);
    ui.vertical_centered(|ui| {
        ui.add(theme::logo().fit_to_exact_size(egui::vec2(96.0, 96.0)));
        ui.add_space(10.0);
        if !g.connected {
            ui.label(RichText::new("Connecting to the wallpaper daemon…").size(18.0));
            return;
        }
        if g.items.is_empty() {
            ui.label(RichText::new("Your library is empty").size(20.0).strong());
            ui.add_space(4.0);
            ui.label("Add a video, GIF, picture, a web page folder, a Lively .zip, or a link. You can also drop files anywhere in this window.");
            ui.add_space(12.0);
            if ui.add(egui::Button::new(RichText::new("＋  Add wallpapers…").size(15.0)).fill(theme::accent(ui)).min_size(egui::vec2(180.0, 36.0))).clicked() {
                actions.push(Action::PickFiles);
            }
        } else {
            ui.label(RichText::new("Nothing matches").size(20.0).strong());
            ui.add_space(4.0);
            ui.label("Try another search or filter.");
        }
    });
}

fn card(ui: &mut egui::Ui, g: &Grid<'_>, w: &Summary, actions: &mut Vec<Action>) {
    let on: Vec<&ActiveInfo> = g.active.iter().filter(|a| a.wallpaper == w.id).collect();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(CARD_W, CARD_H), Sense::click());
    let hovered = response.hovered() || response.context_menu_opened();
    let visuals = ui.visuals();
    let fill = if hovered { visuals.widgets.hovered.weak_bg_fill } else { visuals.faint_bg_color };
    let stroke = if !on.is_empty() { Stroke::new(2.0, theme::accent(ui)) } else if hovered { Stroke::new(1.0, visuals.widgets.hovered.bg_stroke.color) } else { visuals.widgets.noninteractive.bg_stroke };
    ui.painter().rect(rect, CornerRadius::same(10), fill, stroke, StrokeKind::Inside);

    let image_rect = Rect::from_min_size(rect.min, egui::vec2(CARD_W, IMAGE_H)).shrink(1.0);
    let top_corners = CornerRadius { nw: 9, ne: 9, sw: 0, se: 0 };
    match &w.thumbnail {
        Some(path) => {
            ui.put(image_rect, egui::Image::from_uri(format!("file://{}", path.display())).fit_to_exact_size(image_rect.size()).corner_radius(top_corners));
        }
        None => {
            let painter = ui.painter_at(image_rect);
            painter.rect_filled(image_rect, top_corners, theme::kind_color(w.kind));
            painter.text(image_rect.center() - egui::vec2(0.0, 8.0), Align2::CENTER_CENTER, theme::kind_glyph(w.kind), FontId::proportional(40.0), Color32::from_white_alpha(210));
            painter.text(image_rect.center() + egui::vec2(0.0, 26.0), Align2::CENTER_CENTER, w.kind.label(), FontId::proportional(12.0), Color32::from_white_alpha(170));
        }
    }
    if let Some(a) = on.first() {
        let label = match g.arrangement {
            Arrangement::Per => format!("▶ Display {}", crate::model::display::index_of(g.displays, &a.display).unwrap_or(0)),
            Arrangement::Span => "▶ Spanning".to_string(),
            Arrangement::Duplicate => "▶ Every display".to_string(),
        };
        let label = if a.paused { format!("{label} · paused") } else { label };
        let font = FontId::proportional(12.0);
        let galley = ui.painter().layout_no_wrap(label, font.clone(), Color32::WHITE);
        let badge = Rect::from_min_size(image_rect.min + egui::vec2(8.0, 8.0), galley.size() + egui::vec2(14.0, 8.0));
        ui.painter().rect_filled(badge, CornerRadius::same(6), theme::accent(ui).gamma_multiply(0.95));
        ui.painter().galley(badge.min + egui::vec2(7.0, 4.0), galley, Color32::WHITE);
    }

    let text_rect = Rect::from_min_max(egui::pos2(rect.min.x + 12.0, image_rect.max.y + 8.0), rect.max - egui::vec2(12.0, 8.0));
    let title_rect = Rect::from_min_size(text_rect.min, egui::vec2(text_rect.width() - 28.0, 20.0));
    ui.put(title_rect, egui::Label::new(RichText::new(&w.title).size(14.0).strong()).truncate().selectable(false));
    let sub = match w.author.as_deref() {
        Some(a) => format!("{} · {a}", w.kind.label()),
        None => w.kind.label().to_string(),
    };
    let sub_rect = Rect::from_min_size(text_rect.min + egui::vec2(0.0, 24.0), egui::vec2(text_rect.width() - 28.0, 18.0));
    ui.put(sub_rect, egui::Label::new(RichText::new(sub).small().color(ui.visuals().weak_text_color())).truncate().selectable(false));
    ui.painter().text(text_rect.right_top() + egui::vec2(0.0, 2.0), Align2::RIGHT_TOP, theme::kind_glyph(w.kind), FontId::proportional(15.0), ui.visuals().weak_text_color());

    if hovered {
        let btn = Rect::from_min_size(egui::pos2(image_rect.max.x - 78.0, image_rect.max.y - 34.0), egui::vec2(70.0, 26.0));
        let apply = ui.put(btn, egui::Button::new(RichText::new("Apply").size(13.0)).fill(theme::accent(ui)).corner_radius(CornerRadius::same(6)));
        if apply.clicked() {
            actions.push(Action::Apply { id: w.id.clone(), display: None });
        }
    }
    if response.clicked() {
        actions.push(Action::Apply { id: w.id.clone(), display: None });
    }
    response.on_hover_text(hover_text(w)).context_menu(|ui| {
        ui.set_min_width(200.0);
        if g.arrangement == Arrangement::Per && g.displays.len() > 1 {
            for (i, d) in g.displays.iter().enumerate() {
                if ui.button(format!("Apply to display {} · {}", i + 1, d.name)).clicked() {
                    actions.push(Action::Apply { id: w.id.clone(), display: Some(d.id.clone()) });
                    ui.close();
                }
            }
        } else if ui.button("Apply").clicked() {
            actions.push(Action::Apply { id: w.id.clone(), display: g.selected_display.map(str::to_owned) });
            ui.close();
        }
        if w.customizable && ui.button("Customize…").clicked() {
            actions.push(Action::Customize { id: w.id.clone() });
            ui.close();
        }
        ui.separator();
        if ui.button("Edit details…").clicked() {
            actions.push(Action::Edit { id: w.id.clone() });
            ui.close();
        }
        if ui.button("Refresh thumbnail").clicked() {
            actions.push(Action::Thumbnail { id: w.id.clone() });
            ui.close();
        }
        if ui.button("Export as Lively package…").clicked() {
            actions.push(Action::Export { id: w.id.clone() });
            ui.close();
        }
        if ui.button("Show in folder").clicked() {
            actions.push(Action::Reveal { path: if w.kind.is_online() { w.dir.clone() } else { PathBuf::from(&w.source) } });
            ui.close();
        }
        if let Some(url) = &w.contact {
            if url.starts_with("http") && ui.button("Open website").clicked() {
                actions.push(Action::Open { url: url.clone() });
                ui.close();
            }
        }
        ui.separator();
        if ui.button(RichText::new("Remove from library…").color(ui.visuals().error_fg_color)).clicked() {
            actions.push(Action::Delete { id: w.id.clone() });
            ui.close();
        }
    });
}

fn hover_text(w: &Summary) -> String {
    let mut s = format!("{}\n{}", w.title, w.kind.label());
    if let Some(a) = &w.author {
        s.push_str(&format!("\nby {a}"));
    }
    if let Some(d) = &w.desc {
        s.push('\n');
        s.push_str(d);
    }
    s
}
