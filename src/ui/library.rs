use eframe::egui;
use crate::ipc::ActiveInfo;
use crate::model::{Arrangement, Display, Kind, Summary};
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
}

pub struct Grid<'a> {
    pub items: &'a [Summary],
    pub search: &'a str,
    pub displays: &'a [Display],
    pub active: &'a [ActiveInfo],
    pub arrangement: Arrangement,
    pub selected_display: Option<&'a str>,
    pub hovering_files: bool,
}

const TILE: egui::Vec2 = egui::vec2(236.0, 172.0);
const IMAGE: egui::Vec2 = egui::vec2(236.0, 133.0);

fn kind_icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Video => "🎬",
        Kind::Gif => "🎞",
        Kind::Picture => "🖼",
        Kind::VideoStream => "📡",
        Kind::Web | Kind::Url => "🌐",
        Kind::WebAudio => "🎵",
        Kind::Program => "⚙",
    }
}

pub fn grid(ui: &mut egui::Ui, g: &Grid<'_>) -> Vec<Action> {
    let mut actions = Vec::new();
    let needle = g.search.trim().to_lowercase();
    let items: Vec<&Summary> = g
        .items
        .iter()
        .filter(|w| needle.is_empty() || w.title.to_lowercase().contains(&needle) || w.author.as_deref().is_some_and(|a| a.to_lowercase().contains(&needle)))
        .collect();
    if g.hovering_files {
        ui.centered_and_justified(|ui| ui.label(egui::RichText::new("Drop to add to the library").size(22.0)));
        return actions;
    }
    if items.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(if g.items.is_empty() {
                "The library is empty. Add a video, GIF, picture, web page folder, Lively .zip, or a URL, or drop files here."
            } else {
                "No wallpapers match the search."
            })
        });
        return actions;
    }
    egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
            for w in items {
                tile(ui, g, w, &mut actions);
            }
        });
        ui.add_space(12.0);
    });
    actions
}

fn tile(ui: &mut egui::Ui, g: &Grid<'_>, w: &Summary, actions: &mut Vec<Action>) {
    let on: Vec<&ActiveInfo> = g.active.iter().filter(|a| a.wallpaper == w.id).collect();
    let (rect, response) = ui.allocate_exact_size(TILE, egui::Sense::click());
    let visuals = ui.visuals();
    let hovered = response.hovered();
    let frame_fill = if hovered { visuals.widgets.hovered.bg_fill } else { visuals.widgets.noninteractive.bg_fill };
    let stroke = if !on.is_empty() { egui::Stroke::new(2.0, visuals.selection.stroke.color) } else { visuals.widgets.noninteractive.bg_stroke };
    ui.painter().rect(rect, 8.0, frame_fill, stroke, egui::StrokeKind::Inside);
    let image_rect = egui::Rect::from_min_size(rect.min, IMAGE);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(image_rect).layout(egui::Layout::centered_and_justified(egui::Direction::TopDown)));
    child.set_clip_rect(image_rect);
    match &w.thumbnail {
        Some(path) => {
            let uri = format!("file://{}", path.display());
            child.add(egui::Image::from_uri(uri).fit_to_exact_size(IMAGE).corner_radius(egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }));
        }
        None => {
            child.label(egui::RichText::new(kind_icon(w.kind)).size(40.0));
        }
    }
    let text_rect = egui::Rect::from_min_max(egui::pos2(rect.min.x + 8.0, image_rect.max.y + 4.0), rect.max - egui::vec2(8.0, 4.0));
    let painter = ui.painter_at(text_rect);
    let title_font = egui::FontId::proportional(13.5);
    let title = truncate(ui, &w.title, &title_font, text_rect.width() - 20.0);
    painter.text(text_rect.left_top(), egui::Align2::LEFT_TOP, title, title_font, ui.visuals().strong_text_color());
    let sub = match on.first() {
        Some(a) => {
            let idx = crate::model::display::index_of(g.displays, &a.display).unwrap_or(0);
            let label = if g.arrangement == Arrangement::Per { format!("On display {idx}") } else { g.arrangement.label().to_string() };
            if a.paused { format!("{label} · paused") } else { label }
        }
        None => w.author.clone().map(|a| format!("by {a}")).unwrap_or_else(|| w.kind.label().to_string()),
    };
    painter.text(text_rect.left_bottom(), egui::Align2::LEFT_BOTTOM, sub, egui::FontId::proportional(11.5), ui.visuals().weak_text_color());
    painter.text(text_rect.right_top(), egui::Align2::RIGHT_TOP, kind_icon(w.kind), egui::FontId::proportional(13.0), ui.visuals().weak_text_color());
    if response.clicked() {
        actions.push(Action::Apply { id: w.id.clone(), display: None });
    }
    response.on_hover_text(hover_text(w)).context_menu(|ui| {
        if g.arrangement == Arrangement::Per && g.displays.len() > 1 {
            for (i, d) in g.displays.iter().enumerate() {
                if ui.button(format!("Apply to display {} ({})", i + 1, d.name)).clicked() {
                    actions.push(Action::Apply { id: w.id.clone(), display: Some(d.id.clone()) });
                    ui.close();
                }
            }
        } else if ui.button("Apply").clicked() {
            actions.push(Action::Apply { id: w.id.clone(), display: g.selected_display.map(str::to_owned) });
            ui.close();
        }
        if w.customizable && ui.button("Customize").clicked() {
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
        if ui.button("Export package…").clicked() {
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
        if ui.button(egui::RichText::new("Delete").color(egui::Color32::from_rgb(230, 90, 90))).clicked() {
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

fn truncate(ui: &egui::Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let fits = |t: &str| ui.fonts_mut(|f| f.layout_no_wrap(t.to_string(), font.clone(), egui::Color32::WHITE).size().x) <= width;
    if fits(text) {
        return text.to_string();
    }
    let mut out: String = text.chars().take(40).collect();
    while !out.is_empty() && !fits(&format!("{out}…")) {
        out.pop();
    }
    format!("{}…", out.trim_end())
}
