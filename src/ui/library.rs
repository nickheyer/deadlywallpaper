use crate::ipc::ActiveInfo;
use crate::model::{Arrangement, Display, Kind, Summary};
use crate::ui::{theme, widgets};
use eframe::egui::{self, Align, Align2, Button, CornerRadius, CursorIcon, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, UiBuilder};
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
    PickFolders,
    AddLink { url: String },
    SelectDisplay(String),
    Filter(Option<Kind>),
    GoToScreens,
}

pub struct View<'a> {
    pub items: &'a [Summary],
    pub filter: Option<Kind>,
    pub displays: &'a [Display],
    pub active: &'a [ActiveInfo],
    pub arrangement: Arrangement,
    pub selected_display: Option<&'a str>,
    pub connected: bool,
    pub hovering_files: bool,
}

#[derive(Default)]
pub struct State {
    pub search: String,
    /// Move keyboard focus into the search box once.
    pub search_focus: bool,
    pub link: String,
    /// Move keyboard focus into the link field once the Add panel opens.
    pub link_focus: bool,
}

/// Popup id of the Add panel, so the empty state can open it too.
const ADD_POPUP: &str = "library-add";

const MIN_CARD_W: f32 = 210.0;
const MAX_CARD_W: f32 = 320.0;
const META_H: f32 = 60.0;
const GAP: f32 = 14.0;

const KIND_FILTERS: [(Option<Kind>, &str); 7] = [
    (None, "All"),
    (Some(Kind::Video), "Videos"),
    (Some(Kind::Web), "Web"),
    (Some(Kind::Picture), "Pictures"),
    (Some(Kind::Gif), "GIFs"),
    (Some(Kind::VideoStream), "Streams"),
    (Some(Kind::Program), "Programs"),
];

fn kind_matches(w: &Summary, filter: Option<Kind>) -> bool {
    match filter {
        None => true,
        Some(Kind::Web) => w.kind.is_web(),
        Some(k) => w.kind == k,
    }
}

fn search_matches(w: &Summary, needle: &str) -> bool {
    needle.is_empty()
        || w.title.to_lowercase().contains(needle)
        || w.kind.label().to_lowercase().contains(needle)
        || w.author.as_deref().is_some_and(|a| a.to_lowercase().contains(needle))
        || w.desc.as_deref().is_some_and(|d| d.to_lowercase().contains(needle))
}

pub fn page(ui: &mut egui::Ui, v: &View<'_>, state: &mut State) -> Vec<Action> {
    let mut actions = Vec::new();
    let needle = state.search.trim().to_lowercase();
    let shown: Vec<&Summary> = v.items.iter().filter(|w| kind_matches(w, v.filter) && search_matches(w, &needle)).collect();

    let subtitle = match (v.items.len(), shown.len()) {
        (0, _) => None,
        (1, 1) => Some("1 wallpaper".to_string()),
        (n, m) if m == n => Some(format!("{n} wallpapers")),
        (n, m) => Some(format!("{m} of {n} wallpapers")),
    };
    theme::page_header(ui, "Library", subtitle.as_deref(), |ui| {
        let add = ui.add(theme::primary("+  Add"));
        if add.clicked() {
            state.link_focus = true;
        }
        add_popover(ui, &add, state, &mut actions);
        widgets::search_box(ui, &mut state.search, &mut state.search_focus, 170.0);
    });
    ui.add_space(12.0);
    if v.arrangement == Arrangement::Per && v.displays.len() > 1 {
        target_bar(ui, v, &mut actions);
        ui.add_space(6.0);
    }
    filter_bar(ui, v, &mut actions);
    ui.add_space(10.0);

    if v.hovering_files {
        drop_target(ui);
        return actions;
    }
    if shown.is_empty() {
        empty(ui, v, state, &mut actions);
        return actions;
    }
    grid(ui, v, &shown, &mut actions);
    actions
}

fn add_popover(ui: &mut egui::Ui, add: &egui::Response, state: &mut State, actions: &mut Vec<Action>) {
    let id = egui::Id::new(ADD_POPUP);
    let frame = theme::popover_frame(ui);
    egui::Popup::from_toggle_button_response(add)
        .id(id)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(frame)
        .width(400.0)
        .show(|ui| {
            let p = theme::palette(ui);
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 10.0);
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 8.0) / 2.0;
                if ui.add_sized([w, 36.0], theme::secondary_button("Files…")).on_hover_text("Ctrl+O").clicked() {
                    actions.push(Action::PickFiles);
                    egui::Popup::close_id(ui.ctx(), id);
                }
                if ui.add_sized([w, 36.0], theme::secondary_button("Folders…")).clicked() {
                    actions.push(Action::PickFolders);
                    egui::Popup::close_id(ui.ctx(), id);
                }
            });
            widgets::divider(ui);
            ui.horizontal(|ui| {
                let button_w = 64.0;
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut state.link)
                        .hint_text(RichText::new("Paste a link").color(p.text_faint))
                        .desired_width(ui.available_width() - button_w - 8.0)
                        .margin(egui::Margin::symmetric(10, 8)),
                );
                if state.link_focus {
                    edit.request_focus();
                    state.link_focus = false;
                }
                let url = normalize_link(&state.link);
                let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let clicked = ui.add_enabled(url.is_some(), theme::primary("Add").min_size(egui::vec2(button_w, 34.0))).clicked();
                if let Some(url) = url.filter(|_| clicked || submit) {
                    actions.push(Action::AddLink { url });
                    state.link.clear();
                    egui::Popup::close_id(ui.ctx(), id);
                }
            });
        });
}

/// The URL to import for what was typed, accepting links pasted without a scheme.
fn normalize_link(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() || t.contains(char::is_whitespace) {
        return None;
    }
    let url = if t.contains("://") { t.to_string() } else { format!("https://{t}") };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let host = url.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
    (host.contains('.') || host.contains(':') || host.eq_ignore_ascii_case("localhost")).then_some(url)
}

fn short_name(d: &Display) -> String {
    if d.name.chars().count() > 26 { format!("{}…", d.name.chars().take(24).collect::<String>()) } else { d.name.clone() }
}

fn target_bar(ui: &mut egui::Ui, v: &View<'_>, actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        ui.label(RichText::new("Apply to").color(p.text_weak));
        ui.add_space(2.0);
        for (i, d) in v.displays.iter().enumerate() {
            let selected = v.selected_display == Some(d.id.as_str());
            let running = v.active.iter().find(|a| a.display == d.id);
            let label = format!("{}  {}", i + 1, short_name(d));
            let tip = match running {
                Some(a) => format!("{}\n{}×{}\n▶ {}", d.name, d.rect.w, d.rect.h, a.title),
                None => format!("{}\n{}×{}", d.name, d.rect.w, d.rect.h),
            };
            if widgets::chip(ui, selected, &label).on_hover_text(tip).clicked() {
                actions.push(Action::SelectDisplay(d.id.clone()));
            }
        }
        ui.add_space(4.0);
        if ui.link(RichText::new(v.arrangement.label()).small()).on_hover_text("Change on the Screens page").clicked() {
            actions.push(Action::GoToScreens);
        }
    });
}

fn filter_bar(ui: &mut egui::Ui, v: &View<'_>, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for (kind, label) in KIND_FILTERS {
            let count = v.items.iter().filter(|w| kind_matches(w, kind)).count();
            if kind.is_some() && count == 0 {
                continue;
            }
            let text = if v.items.is_empty() { label.to_string() } else { format!("{label}  {count}") };
            if widgets::chip(ui, v.filter == kind, &text).clicked() {
                actions.push(Action::Filter(kind));
            }
        }
    });
}

fn drop_target(ui: &mut egui::Ui) {
    let p = theme::palette(ui);
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    painter.rect(rect.shrink(4.0), CornerRadius::same(14), p.accent_soft, Stroke::new(2.0, p.accent), StrokeKind::Inside);
    painter.text(rect.center() - egui::vec2(0.0, 18.0), Align2::CENTER_CENTER, "📥", FontId::proportional(36.0), p.accent);
    painter.text(rect.center() + egui::vec2(0.0, 24.0), Align2::CENTER_CENTER, "Drop to add", FontId::proportional(20.0), p.text_strong);
    ui.allocate_rect(rect, Sense::hover());
}

fn empty(ui: &mut egui::Ui, v: &View<'_>, state: &mut State, actions: &mut Vec<Action>) {
    if !v.connected && v.items.is_empty() {
        widgets::empty_state(ui, "⏳", "Connecting…", "", None);
    } else if v.items.is_empty() {
        if widgets::empty_state(ui, "🖼", "Library is empty", "Drop files or folders here.", Some("Add…")) {
            egui::Popup::open_id(ui.ctx(), egui::Id::new(ADD_POPUP));
            state.link_focus = true;
        }
    } else if widgets::empty_state(ui, "🔍", "Nothing matches", "", Some("Show all")) {
        state.search.clear();
        actions.push(Action::Filter(None));
    }
}

fn grid(ui: &mut egui::Ui, v: &View<'_>, shown: &[&Summary], actions: &mut Vec<Action>) {
    egui::ScrollArea::vertical().id_salt("library-grid").auto_shrink([false; 2]).show(ui, |ui| {
        let avail = ui.available_width();
        let cols = (((avail + GAP) / (MIN_CARD_W + GAP)).floor() as usize).max(1);
        let card_w = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32).min(MAX_CARD_W).floor();
        let image_h = (card_w * 9.0 / 16.0).round();
        let card_h = image_h + META_H;
        ui.spacing_mut().item_spacing.y = GAP;
        ui.add_space(2.0);
        for row in shown.chunks(cols) {
            let (row_rect, _) = ui.allocate_exact_size(egui::vec2(avail, card_h), Sense::hover());
            if !ui.is_rect_visible(row_rect) {
                continue;
            }
            for (i, w) in row.iter().enumerate() {
                let rect = Rect::from_min_size(row_rect.min + egui::vec2(i as f32 * (card_w + GAP), 0.0), egui::vec2(card_w, card_h));
                card(ui, v, w, rect, image_h, actions);
            }
        }
        ui.add_space(10.0);
    });
}

fn subtitle(w: &Summary) -> String {
    match w.author.as_deref() {
        Some(a) => format!("{} · {a}", w.kind.label()),
        None => w.kind.label().to_string(),
    }
}

fn card(ui: &mut egui::Ui, v: &View<'_>, w: &Summary, rect: Rect, image_h: f32, actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    let id = ui.id().with(("card", &w.id));
    let response = ui.interact(rect, id, Sense::click());
    let menu_open = response.context_menu_opened() || egui::Popup::is_id_open(ui.ctx(), id.with("menu"));
    let hovered = ui.rect_contains_pointer(rect) || menu_open;
    let instances: Vec<&ActiveInfo> = v.active.iter().filter(|a| a.wallpaper == w.id).collect();
    let playing = !instances.is_empty();
    let t = ui.ctx().animate_bool_responsive(id.with("hover"), hovered);
    let fill = p.surface.lerp_to_gamma(p.surface_hover, t);
    let stroke = if playing { Stroke::new(1.5, p.accent) } else { Stroke::new(1.0, p.stroke.lerp_to_gamma(p.stroke_strong, t)) };
    ui.painter().rect(rect, CornerRadius::same(12), fill, stroke, StrokeKind::Inside);

    let image_rect = Rect::from_min_size(rect.min, egui::vec2(rect.width(), image_h)).shrink(1.5);
    let corners = CornerRadius { nw: 11, ne: 11, sw: 0, se: 0 };
    let uri = w.thumbnail.as_deref().map(widgets::thumbnail_uri);
    widgets::thumbnail(ui, image_rect, uri.as_deref(), w.kind, corners);

    if let Some(a) = instances.first() {
        let glyph = if !a.loaded {
            "⏳"
        } else if a.paused {
            "⏸"
        } else {
            "▶"
        };
        let label = match v.arrangement {
            Arrangement::Per if v.displays.len() > 1 => {
                let mut numbers: Vec<usize> = instances.iter().filter_map(|i| crate::model::display::index_of(v.displays, &i.display)).collect();
                numbers.sort_unstable();
                format!("{glyph} {}", numbers.iter().map(usize::to_string).collect::<Vec<_>>().join(" "))
            }
            _ => glyph.to_string(),
        };
        widgets::badge(ui.painter(), image_rect.min + egui::vec2(8.0, 8.0), Align2::LEFT_TOP, &label, p.accent, p.on_accent);
    }

    if hovered {
        let bar_h = 42.0;
        let bar = Rect::from_min_max(egui::pos2(image_rect.left(), image_rect.bottom() - bar_h), image_rect.right_bottom());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, p.overlay);
        let mut child = ui.new_child(UiBuilder::new().id_salt(("card-actions", &w.id)).max_rect(bar.shrink2(egui::vec2(8.0, 6.0))).layout(Layout::right_to_left(Align::Center)));
        child.spacing_mut().item_spacing.x = 6.0;
        if child.add(theme::primary("Apply").small()).clicked() {
            actions.push(Action::Apply { id: w.id.clone(), display: None });
        }
        if w.customizable {
            let b = Button::new(RichText::new("Customize").color(egui::Color32::WHITE)).fill(egui::Color32::from_white_alpha(40)).stroke(Stroke::NONE).corner_radius(CornerRadius::same(8)).min_size(egui::vec2(0.0, 28.0));
            if child.add(b).clicked() {
                actions.push(Action::Customize { id: w.id.clone() });
            }
        }
    }

    let meta = Rect::from_min_max(egui::pos2(rect.left() + 12.0, image_rect.bottom() + 9.0), egui::pos2(rect.right() - 8.0, rect.bottom() - 8.0));
    let more_rect = Rect::from_min_size(egui::pos2(meta.right() - 28.0, meta.top() - 2.0), egui::vec2(28.0, 28.0));
    let text_w = more_rect.left() - meta.left() - 6.0;
    widgets::elided(ui.painter(), meta.left_top(), Align2::LEFT_TOP, &w.title, FontId::proportional(14.0), p.text_strong, text_w);
    widgets::elided(ui.painter(), egui::pos2(meta.left(), meta.top() + 22.0), Align2::LEFT_TOP, &subtitle(w), FontId::proportional(12.0), p.text_weak, text_w);

    let mut child = ui.new_child(UiBuilder::new().id_salt(("card-more", &w.id)).max_rect(more_rect).layout(Layout::centered_and_justified(egui::Direction::LeftToRight)));
    let more = child.add(Button::new(RichText::new("···").size(16.0).color(if hovered { p.text } else { p.text_faint })).frame_when_inactive(false).min_size(more_rect.size()).corner_radius(CornerRadius::same(7))).on_hover_text("More actions");
    egui::Popup::menu(&more).id(id.with("menu")).show(|ui| menu_items(ui, v, w, actions));

    let response = response.on_hover_cursor(CursorIcon::PointingHand).on_hover_text(hover_text(w));
    if response.clicked() {
        actions.push(Action::Apply { id: w.id.clone(), display: None });
    }
    response.context_menu(|ui| menu_items(ui, v, w, actions));
}

fn menu_items(ui: &mut egui::Ui, v: &View<'_>, w: &Summary, actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    ui.set_min_width(230.0);
    if v.arrangement == Arrangement::Per && v.displays.len() > 1 {
        for (i, d) in v.displays.iter().enumerate() {
            if ui.button(RichText::new(format!("Apply to {}  {}", i + 1, short_name(d)))).clicked() {
                actions.push(Action::Apply { id: w.id.clone(), display: Some(d.id.clone()) });
            }
        }
    } else if ui.button("Apply").clicked() {
        actions.push(Action::Apply { id: w.id.clone(), display: None });
    }
    if w.customizable && ui.button("Customize…").clicked() {
        actions.push(Action::Customize { id: w.id.clone() });
    }
    ui.separator();
    if ui.button("Edit…").clicked() {
        actions.push(Action::Edit { id: w.id.clone() });
    }
    if ui.button("New thumbnail").clicked() {
        actions.push(Action::Thumbnail { id: w.id.clone() });
    }
    if ui.button("Export…").clicked() {
        actions.push(Action::Export { id: w.id.clone() });
    }
    if ui.button("Show in folder").clicked() {
        actions.push(Action::Reveal { path: if w.kind.is_online() { w.dir.clone() } else { PathBuf::from(&w.source) } });
    }
    if let Some(url) = w.contact.as_deref().filter(|u| u.starts_with("http")) {
        if ui.button("Website").clicked() {
            actions.push(Action::Open { url: url.to_string() });
        }
    }
    ui.separator();
    if ui.button(RichText::new("Remove…").color(p.danger)).clicked() {
        actions.push(Action::Delete { id: w.id.clone() });
    }
}

fn hover_text(w: &Summary) -> String {
    let mut s = format!("{}\n{}", w.title, subtitle(w));
    if let Some(d) = &w.desc {
        s.push_str("\n\n");
        s.push_str(d.trim());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::normalize_link;

    #[test]
    fn links_are_normalized() {
        assert_eq!(normalize_link(" youtube.com/watch?v=x "), Some("https://youtube.com/watch?v=x".into()));
        assert_eq!(normalize_link("http://localhost:8000"), Some("http://localhost:8000".into()));
        assert_eq!(normalize_link("https://example.org"), Some("https://example.org".into()));
        assert_eq!(normalize_link("ftp://a.b"), None);
        assert_eq!(normalize_link("not a link"), None);
        assert_eq!(normalize_link("hello"), None);
        assert_eq!(normalize_link(""), None);
    }
}
