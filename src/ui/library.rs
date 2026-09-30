use crate::ipc::ActiveInfo;
use crate::model::{Arrangement, Display, Kind, Summary};
use crate::ui::order::{self, Group, Key, Layout as ViewLayout, Order, Today};
use crate::ui::{theme, widgets};
use eframe::egui::{
    self, Align, Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Key as Hotkey,
    Layout, Margin, Modifiers, Rect, RichText, Sense, Stroke, StrokeKind, UiBuilder,
};
use std::collections::HashSet;
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
    BulkEdit { ids: Vec<String> },
    BulkThumbnail { ids: Vec<String> },
    BulkExport { ids: Vec<String> },
    BulkDelete { ids: Vec<String> },
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
    /// The page may act on Escape, Delete and Ctrl+A: no dialog is open above it.
    pub shortcuts: bool,
}

#[derive(Default)]
pub struct State {
    pub search: String,
    /// Move keyboard focus into the search box once.
    pub search_focus: bool,
    pub link: String,
    /// Move keyboard focus into the link field once the Add panel opens.
    pub link_focus: bool,
    pub order: Order,
    /// Group headers folded shut, keyed by sort key and group label.
    collapsed: HashSet<String>,
    selection: Selection,
}

/// Wallpapers picked for a bulk operation.
#[derive(Default)]
pub struct Selection {
    ids: HashSet<String>,
    /// Where the next shift-click range starts.
    anchor: Option<String>,
}

impl Selection {
    fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    fn len(&self) -> usize {
        self.ids.len()
    }

    fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    /// Selected ids, in a stable order.
    fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.ids.iter().cloned().collect();
        ids.sort();
        ids
    }

    fn toggle(&mut self, id: &str) {
        if !self.ids.remove(id) {
            self.ids.insert(id.to_string());
        }
        self.anchor = Some(id.to_string());
    }

    /// Add everything between the anchor and `id` in `visible` order; without an anchor
    /// this is a plain toggle.
    fn range(&mut self, id: &str, visible: &[String]) {
        let at = |needle: &str| visible.iter().position(|v| v == needle);
        match (self.anchor.as_deref().and_then(at), at(id)) {
            (Some(a), Some(b)) => {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                self.ids.extend(visible[lo..=hi].iter().cloned());
            }
            _ => self.toggle(id),
        }
    }

    fn select_all<'a>(&mut self, ids: impl Iterator<Item = &'a str>) {
        self.ids.extend(ids.map(str::to_owned));
    }

    fn clear(&mut self) {
        self.ids.clear();
        self.anchor = None;
    }

    /// Drop ids that left the library.
    fn retain(&mut self, present: &HashSet<&str>) {
        self.ids.retain(|id| present.contains(id.as_str()));
        if self.anchor.as_deref().is_some_and(|a| !present.contains(a)) {
            self.anchor = None;
        }
    }
}

/// Selection state handed down to every card and row, with the ids on screen in display
/// order for shift-click ranges.
struct Pick<'a> {
    sel: &'a mut Selection,
    visible: &'a [String],
    /// The sort key, which cards and rows show the value of.
    key: Key,
}

/// Popup id of the Add panel, so the empty state can open it too.
const ADD_POPUP: &str = "library-add";

const MIN_CARD_W: f32 = 210.0;
const MAX_CARD_W: f32 = 320.0;
const META_H: f32 = 60.0;
const GAP: f32 = 14.0;
const ROW_H: f32 = 58.0;
const ROW_GAP: f32 = 6.0;
const HEADER_H: f32 = 30.0;
const CHECK: f32 = 18.0;
/// Width the sort, group and layout controls need on the filter row.
const CONTROLS_W: f32 = 400.0;

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
        || w.author
            .as_deref()
            .is_some_and(|a| a.to_lowercase().contains(needle))
        || w.desc
            .as_deref()
            .is_some_and(|d| d.to_lowercase().contains(needle))
}

pub fn page(ui: &mut egui::Ui, v: &View<'_>, state: &mut State) -> Vec<Action> {
    let mut actions = Vec::new();
    let needle = state.search.trim().to_lowercase();
    let shown: Vec<&Summary> = v
        .items
        .iter()
        .filter(|w| kind_matches(w, v.filter) && search_matches(w, &needle))
        .collect();
    let present: HashSet<&str> = v.items.iter().map(|w| w.id.as_str()).collect();
    state.selection.retain(&present);
    if v.shortcuts && !ui.ctx().egui_wants_keyboard_input() {
        let (select_all, escape, delete) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::COMMAND, Hotkey::A),
                i.key_pressed(Hotkey::Escape),
                i.key_pressed(Hotkey::Delete),
            )
        });
        if select_all {
            state
                .selection
                .select_all(shown.iter().map(|w| w.id.as_str()));
        }
        if escape {
            state.selection.clear();
        }
        if delete && !state.selection.is_empty() {
            actions.push(Action::BulkDelete {
                ids: state.selection.ids(),
            });
        }
    }

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
    filter_bar(ui, v, state, &mut actions);
    if !state.selection.is_empty() {
        ui.add_space(8.0);
        selection_bar(ui, &shown, state, &mut actions);
    }
    ui.add_space(10.0);

    if v.hovering_files {
        drop_target(ui);
        return actions;
    }
    if shown.is_empty() {
        empty(ui, v, state, &mut actions);
        return actions;
    }
    let groups = order::arrange(&shown, &state.order, Today::now());
    content(ui, v, state, &groups, &mut actions);
    actions
}

fn add_popover(
    ui: &mut egui::Ui,
    add: &egui::Response,
    state: &mut State,
    actions: &mut Vec<Action>,
) {
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
                if ui
                    .add_sized([w, 36.0], theme::secondary_button("Files…"))
                    .on_hover_text("Ctrl+O")
                    .clicked()
                {
                    actions.push(Action::PickFiles);
                    egui::Popup::close_id(ui.ctx(), id);
                }
                if ui
                    .add_sized([w, 36.0], theme::secondary_button("Folders…"))
                    .clicked()
                {
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
                let clicked = ui
                    .add_enabled(
                        url.is_some(),
                        theme::primary("Add").min_size(egui::vec2(button_w, 34.0)),
                    )
                    .clicked();
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
    let url = if t.contains("://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return None;
    }
    let host = url
        .split("://")
        .nth(1)
        .unwrap_or("")
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    (host.contains('.') || host.contains(':') || host.eq_ignore_ascii_case("localhost"))
        .then_some(url)
}

fn short_name(d: &Display) -> String {
    if d.name.chars().count() > 26 {
        format!("{}…", d.name.chars().take(24).collect::<String>())
    } else {
        d.name.clone()
    }
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
            if widgets::chip(ui, selected, &label)
                .on_hover_text(tip)
                .clicked()
            {
                actions.push(Action::SelectDisplay(d.id.clone()));
            }
        }
        ui.add_space(4.0);
        if ui
            .link(RichText::new(v.arrangement.label()).small())
            .on_hover_text("Change on the Screens page")
            .clicked()
        {
            actions.push(Action::GoToScreens);
        }
    });
}

/// Kind filter chips on the left; sort, group and layout controls on the right.
fn filter_bar(ui: &mut egui::Ui, v: &View<'_>, state: &mut State, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let chips_w = (ui.available_width() - CONTROLS_W).max(180.0);
        ui.allocate_ui_with_layout(
            egui::vec2(chips_w, 0.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_max_width(chips_w);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (kind, label) in KIND_FILTERS {
                        let count = v.items.iter().filter(|w| kind_matches(w, kind)).count();
                        if kind.is_some() && count == 0 {
                            continue;
                        }
                        let text = if v.items.is_empty() {
                            label.to_string()
                        } else {
                            format!("{label}  {count}")
                        };
                        if widgets::chip(ui, v.filter == kind, &text).clicked() {
                            actions.push(Action::Filter(kind));
                        }
                    }
                });
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            view_controls(ui, &mut state.order);
        });
    });
}

/// Laid out right to left: layout switch, group toggle, direction, then the sort key.
fn view_controls(ui: &mut egui::Ui, order: &mut Order) {
    ui.spacing_mut().item_spacing.x = 8.0;
    widgets::segmented(
        ui,
        "library-layout",
        &mut order.layout,
        &[(ViewLayout::Grid, "⊞  Grid"), (ViewLayout::List, "☰  List")],
    );
    if widgets::chip(ui, order.grouped, "Group")
        .on_hover_text(format!("Group by {}", order.key.label().to_lowercase()))
        .clicked()
    {
        order.grouped = !order.grouped;
    }
    let glyph = if order.descending { "⬇" } else { "⬆" };
    if ui
        .add(theme::secondary_button(glyph).min_size(egui::vec2(34.0, 30.0)))
        .on_hover_text(direction_label(order.key, order.descending))
        .clicked()
    {
        order.descending = !order.descending;
    }
    egui::ComboBox::from_id_salt("library-sort")
        .selected_text(format!("Sort: {}", order.key.label()))
        .width(150.0)
        .show_ui(ui, |ui| {
            for key in Key::ALL {
                if ui.selectable_label(order.key == key, key.label()).clicked() {
                    order.set_key(key);
                }
            }
        });
}

fn direction_label(key: Key, descending: bool) -> &'static str {
    match (key, descending) {
        (Key::Added | Key::Modified, true) => "Newest first",
        (Key::Added | Key::Modified, false) => "Oldest first",
        (Key::Size, true) => "Largest first",
        (Key::Size, false) => "Smallest first",
        (Key::Kind, true) => "Reversed",
        (Key::Kind, false) => "Videos first",
        (_, true) => "Z to A",
        (_, false) => "A to Z",
    }
}

/// What is selected and everything that can be done to it at once.
fn selection_bar(
    ui: &mut egui::Ui,
    shown: &[&Summary],
    state: &mut State,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    let frame = Frame::new()
        .fill(p.accent_soft)
        .stroke(Stroke::new(1.0, p.accent))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(12, 8));
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let ids = state.selection.ids();
            let hidden = ids
                .iter()
                .filter(|id| !shown.iter().any(|w| &w.id == *id))
                .count();
            ui.label(
                RichText::new(format!("{} selected", count(ids.len(), "wallpaper")))
                    .strong()
                    .color(p.text_strong),
            );
            if hidden > 0 {
                ui.label(
                    RichText::new(format!("{hidden} hidden by the current filter"))
                        .small()
                        .color(p.text_weak),
                );
            }
            ui.add_space(4.0);
            let unselected = shown
                .iter()
                .filter(|w| !state.selection.contains(&w.id))
                .count();
            if unselected > 0
                && ui
                    .link(format!("Select all {}", shown.len()))
                    .on_hover_text("Ctrl+A")
                    .clicked()
            {
                state
                    .selection
                    .select_all(shown.iter().map(|w| w.id.as_str()));
            }
            if ui.link("Clear").on_hover_text("Escape").clicked() {
                state.selection.clear();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if ui
                    .add(theme::danger("Remove…").small())
                    .on_hover_text("Delete")
                    .clicked()
                {
                    actions.push(Action::BulkDelete { ids: ids.clone() });
                }
                if ui
                    .add(theme::secondary_button("Export…").min_size(egui::vec2(0.0, 28.0)))
                    .on_hover_text("Write a Lively package for each into a folder")
                    .clicked()
                {
                    actions.push(Action::BulkExport { ids: ids.clone() });
                }
                if ui
                    .add(theme::secondary_button("New thumbnails").min_size(egui::vec2(0.0, 28.0)))
                    .on_hover_text("Capture a fresh thumbnail for each")
                    .clicked()
                {
                    actions.push(Action::BulkThumbnail { ids: ids.clone() });
                }
                if ui
                    .add(theme::secondary_button("Edit…").min_size(egui::vec2(0.0, 28.0)))
                    .on_hover_text("Change the author, license, website or description of all")
                    .clicked()
                {
                    actions.push(Action::BulkEdit { ids });
                }
            });
        });
    });
}

/// "1 wallpaper", "5 wallpapers".
pub fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

/// A selection box; returns true when clicked.
fn check_box(ui: &mut egui::Ui, id: egui::Id, rect: Rect, selected: bool, on_image: bool) -> bool {
    let p = theme::palette(ui);
    let response = ui
        .interact(rect, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if selected { "Deselect" } else { "Select" });
    let (fill, stroke) = if selected {
        (p.accent, Stroke::NONE)
    } else if on_image {
        (
            Color32::from_black_alpha(if response.hovered() { 160 } else { 110 }),
            Stroke::new(1.5, Color32::from_white_alpha(210)),
        )
    } else {
        (
            if response.hovered() {
                p.control_hover
            } else {
                p.control
            },
            Stroke::new(1.0, p.stroke_strong),
        )
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(5),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    if selected {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "✔",
            FontId::proportional(13.0),
            p.on_accent,
        );
    }
    response.clicked()
}

fn drop_target(ui: &mut egui::Ui) {
    let p = theme::palette(ui);
    let rect = ui.available_rect_before_wrap();
    let painter = ui.painter_at(rect);
    painter.rect(
        rect.shrink(4.0),
        CornerRadius::same(14),
        p.accent_soft,
        Stroke::new(2.0, p.accent),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center() - egui::vec2(0.0, 18.0),
        Align2::CENTER_CENTER,
        "📥",
        FontId::proportional(36.0),
        p.accent,
    );
    painter.text(
        rect.center() + egui::vec2(0.0, 24.0),
        Align2::CENTER_CENTER,
        "Drop to add",
        FontId::proportional(20.0),
        p.text_strong,
    );
    ui.allocate_rect(rect, Sense::hover());
}

fn empty(ui: &mut egui::Ui, v: &View<'_>, state: &mut State, actions: &mut Vec<Action>) {
    if !v.connected && v.items.is_empty() {
        widgets::empty_state(ui, "⏳", "Connecting…", "", None);
    } else if v.items.is_empty() {
        if widgets::empty_state(
            ui,
            "🖼",
            "Library is empty",
            "Drop files or folders here.",
            Some("Add…"),
        ) {
            egui::Popup::open_id(ui.ctx(), egui::Id::new(ADD_POPUP));
            state.link_focus = true;
        }
    } else if widgets::empty_state(ui, "🔍", "Nothing matches", "", Some("Show all")) {
        state.search.clear();
        actions.push(Action::Filter(None));
    }
}

/// Card grid measurements for the available width.
struct Geometry {
    avail: f32,
    cols: usize,
    card_w: f32,
    image_h: f32,
    card_h: f32,
}

impl Geometry {
    fn fit(avail: f32) -> Geometry {
        let cols = (((avail + GAP) / (MIN_CARD_W + GAP)).floor() as usize).max(1);
        let card_w = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32)
            .min(MAX_CARD_W)
            .floor();
        let image_h = (card_w * 9.0 / 16.0).round();
        Geometry {
            avail,
            cols,
            card_w,
            image_h,
            card_h: image_h + META_H,
        }
    }
}

/// The arranged wallpapers: one grid or list per group, under a header when grouped.
fn content(
    ui: &mut egui::Ui,
    v: &View<'_>,
    state: &mut State,
    groups: &[Group<'_>],
    actions: &mut Vec<Action>,
) {
    let order = state.order;
    let list = order.layout == ViewLayout::List;
    let visible: Vec<String> = groups
        .iter()
        .filter(|g| {
            !order.grouped
                || !state
                    .collapsed
                    .contains(&format!("{}:{}", order.key.name(), g.label))
        })
        .flat_map(|g| g.items.iter().map(|w| w.id.clone()))
        .collect();
    let collapsed = &mut state.collapsed;
    let mut pick = Pick {
        sel: &mut state.selection,
        visible: &visible,
        key: order.key,
    };
    egui::ScrollArea::vertical()
        .id_salt("library-content")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            let geo = Geometry::fit(ui.available_width());
            ui.spacing_mut().item_spacing.y = if list { ROW_GAP } else { GAP };
            ui.add_space(2.0);
            for (i, group) in groups.iter().enumerate() {
                if order.grouped {
                    if i > 0 {
                        ui.add_space(10.0);
                    }
                    let key = format!("{}:{}", order.key.name(), group.label);
                    if !group_header(ui, collapsed, &key, &group.label, group.items.len()) {
                        continue;
                    }
                }
                if list {
                    list_rows(ui, v, &group.items, geo.avail, &mut pick, actions);
                } else {
                    grid_rows(ui, v, &group.items, &geo, &mut pick, actions);
                }
            }
            ui.add_space(10.0);
        });
}

/// A clickable group heading; returns whether the group's items are shown.
fn group_header(
    ui: &mut egui::Ui,
    collapsed: &mut HashSet<String>,
    key: &str,
    label: &str,
    count: usize,
) -> bool {
    let p = theme::palette(ui);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEADER_H), Sense::click());
    let open = !collapsed.contains(key);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = egui::pos2(rect.left() + 8.0, rect.center().y);
        let triangle = if open {
            vec![
                c + egui::vec2(-5.0, -3.0),
                c + egui::vec2(5.0, -3.0),
                c + egui::vec2(0.0, 4.0),
            ]
        } else {
            vec![
                c + egui::vec2(-3.0, -5.0),
                c + egui::vec2(4.0, 0.0),
                c + egui::vec2(-3.0, 5.0),
            ]
        };
        let fg = if response.hovered() {
            p.text_strong
        } else {
            p.text_weak
        };
        painter.add(egui::Shape::convex_polygon(triangle, fg, Stroke::NONE));
        let title = widgets::elided(
            painter,
            egui::pos2(rect.left() + 24.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(15.0),
            p.text_strong,
            (rect.width() * 0.7).max(80.0),
        );
        let count_rect = painter.text(
            egui::pos2(title.right() + 8.0, rect.center().y),
            Align2::LEFT_CENTER,
            count.to_string(),
            FontId::proportional(13.0),
            p.text_weak,
        );
        let line_from = count_rect.right() + 12.0;
        if line_from < rect.right() - 4.0 {
            painter.hline(
                line_from..=rect.right(),
                rect.center().y,
                Stroke::new(1.0, p.stroke),
            );
        }
    }
    if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        if open {
            collapsed.insert(key.to_string());
        } else {
            collapsed.remove(key);
        }
        return !open;
    }
    open
}

fn grid_rows(
    ui: &mut egui::Ui,
    v: &View<'_>,
    items: &[&Summary],
    geo: &Geometry,
    pick: &mut Pick<'_>,
    actions: &mut Vec<Action>,
) {
    for row in items.chunks(geo.cols) {
        let (row_rect, _) =
            ui.allocate_exact_size(egui::vec2(geo.avail, geo.card_h), Sense::hover());
        if !ui.is_rect_visible(row_rect) {
            continue;
        }
        for (i, w) in row.iter().enumerate() {
            let rect = Rect::from_min_size(
                row_rect.min + egui::vec2(i as f32 * (geo.card_w + GAP), 0.0),
                egui::vec2(geo.card_w, geo.card_h),
            );
            card(ui, v, w, rect, geo.image_h, pick, actions);
        }
    }
}

fn list_rows(
    ui: &mut egui::Ui,
    v: &View<'_>,
    items: &[&Summary],
    avail: f32,
    pick: &mut Pick<'_>,
    actions: &mut Vec<Action>,
) {
    for w in items {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(avail, ROW_H), Sense::hover());
        if !ui.is_rect_visible(rect) {
            continue;
        }
        list_row(ui, v, w, rect, pick, actions);
    }
}

/// Second line of a card or row: the kind, then the value of the sort key.
fn subtitle(w: &Summary, key: Key) -> String {
    match order::detail(w, key) {
        Some(d) => format!("{} · {d}", w.kind.label()),
        None => w.kind.label().to_string(),
    }
}

/// The running instances of `w`, with the display numbers they play on.
fn playing_label(v: &View<'_>, instances: &[&ActiveInfo]) -> Option<String> {
    let a = instances.first()?;
    let glyph = if !a.loaded {
        "⏳"
    } else if a.paused {
        "⏸"
    } else {
        "▶"
    };
    Some(match v.arrangement {
        Arrangement::Per if v.displays.len() > 1 => {
            let mut numbers: Vec<usize> = instances
                .iter()
                .filter_map(|i| crate::model::display::index_of(v.displays, &i.display))
                .collect();
            numbers.sort_unstable();
            format!(
                "{glyph} {}",
                numbers
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
        _ => glyph.to_string(),
    })
}

/// Hover and click state shared by cards and rows.
struct Tile<'a> {
    id: egui::Id,
    response: egui::Response,
    hovered: bool,
    selected: bool,
    /// Show the selection box: hovered, selected, or a selection is under way.
    show_check: bool,
    instances: Vec<&'a ActiveInfo>,
    fill: Color32,
    stroke: Stroke,
}

fn tile<'a>(
    ui: &mut egui::Ui,
    v: &View<'a>,
    w: &Summary,
    rect: Rect,
    salt: &str,
    sel: &Selection,
) -> Tile<'a> {
    let p = theme::palette(ui);
    let id = ui.id().with((salt, &w.id));
    let response = ui.interact(rect, id, Sense::click());
    let menu_open =
        response.context_menu_opened() || egui::Popup::is_id_open(ui.ctx(), id.with("menu"));
    let hovered = ui.rect_contains_pointer(rect) || menu_open;
    let selected = sel.contains(&w.id);
    let instances: Vec<&ActiveInfo> = v.active.iter().filter(|a| a.wallpaper == w.id).collect();
    let t = ui.ctx().animate_bool_responsive(id.with("hover"), hovered);
    let fill = if selected {
        p.surface.lerp_to_gamma(p.accent, 0.14)
    } else {
        p.surface.lerp_to_gamma(p.surface_hover, t)
    };
    let stroke = if selected {
        Stroke::new(2.0, p.accent)
    } else if instances.is_empty() {
        Stroke::new(1.0, p.stroke.lerp_to_gamma(p.stroke_strong, t))
    } else {
        Stroke::new(1.5, p.accent)
    };
    Tile {
        id,
        response,
        hovered,
        selected,
        show_check: hovered || selected || !sel.is_empty(),
        instances,
        fill,
        stroke,
    }
}

fn card(
    ui: &mut egui::Ui,
    v: &View<'_>,
    w: &Summary,
    rect: Rect,
    image_h: f32,
    pick: &mut Pick<'_>,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    let tile = tile(ui, v, w, rect, "card", pick.sel);
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        tile.fill,
        tile.stroke,
        StrokeKind::Inside,
    );

    let image_rect = Rect::from_min_size(rect.min, egui::vec2(rect.width(), image_h)).shrink(1.5);
    let corners = CornerRadius {
        nw: 11,
        ne: 11,
        sw: 0,
        se: 0,
    };
    let uri = w.thumbnail.as_deref().map(widgets::thumbnail_uri);
    widgets::thumbnail(ui, image_rect, uri.as_deref(), w.kind, corners);

    if let Some(label) = playing_label(v, &tile.instances) {
        widgets::badge(
            ui.painter(),
            image_rect.min + egui::vec2(8.0, 8.0),
            Align2::LEFT_TOP,
            &label,
            p.accent,
            p.on_accent,
        );
    }
    if tile.show_check {
        let check = Rect::from_min_size(
            image_rect.right_top() + egui::vec2(-8.0 - CHECK, 8.0),
            egui::Vec2::splat(CHECK),
        );
        if check_box(ui, tile.id.with("check"), check, tile.selected, true) {
            pick.sel.toggle(&w.id);
        }
    }

    if tile.hovered {
        let bar_h = 42.0;
        let bar = Rect::from_min_max(
            egui::pos2(image_rect.left(), image_rect.bottom() - bar_h),
            image_rect.right_bottom(),
        );
        ui.painter().rect_filled(bar, CornerRadius::ZERO, p.overlay);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("card-actions", &w.id))
                .max_rect(bar.shrink2(egui::vec2(8.0, 6.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 6.0;
        if child.add(theme::primary("Apply").small()).clicked() {
            actions.push(Action::Apply {
                id: w.id.clone(),
                display: None,
            });
        }
        if w.customizable {
            let b = Button::new(RichText::new("Customize").color(egui::Color32::WHITE))
                .fill(egui::Color32::from_white_alpha(40))
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(8))
                .min_size(egui::vec2(0.0, 28.0));
            if child.add(b).clicked() {
                actions.push(Action::Customize { id: w.id.clone() });
            }
        }
    }

    let meta = Rect::from_min_max(
        egui::pos2(rect.left() + 12.0, image_rect.bottom() + 9.0),
        egui::pos2(rect.right() - 8.0, rect.bottom() - 8.0),
    );
    let more_rect = Rect::from_min_size(
        egui::pos2(meta.right() - 28.0, meta.top() - 2.0),
        egui::vec2(28.0, 28.0),
    );
    let text_w = more_rect.left() - meta.left() - 6.0;
    widgets::elided(
        ui.painter(),
        meta.left_top(),
        Align2::LEFT_TOP,
        &w.title,
        FontId::proportional(14.0),
        p.text_strong,
        text_w,
    );
    widgets::elided(
        ui.painter(),
        egui::pos2(meta.left(), meta.top() + 22.0),
        Align2::LEFT_TOP,
        &subtitle(w, pick.key),
        FontId::proportional(12.0),
        p.text_weak,
        text_w,
    );
    more_button(ui, v, w, &tile, more_rect, pick, actions);
    finish_tile(v, w, tile, pick, actions);
}

fn list_row(
    ui: &mut egui::Ui,
    v: &View<'_>,
    w: &Summary,
    rect: Rect,
    pick: &mut Pick<'_>,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    let key = pick.key;
    let tile = tile(ui, v, w, rect, "row", pick.sel);
    ui.painter().rect(
        rect,
        CornerRadius::same(10),
        tile.fill,
        tile.stroke,
        StrokeKind::Inside,
    );

    if tile.show_check {
        let check = Rect::from_center_size(
            egui::pos2(rect.left() + 9.0 + CHECK / 2.0, rect.center().y),
            egui::Vec2::splat(CHECK),
        );
        if check_box(ui, tile.id.with("check"), check, tile.selected, false) {
            pick.sel.toggle(&w.id);
        }
    }
    let thumb_h = rect.height() - 14.0;
    let thumb = Rect::from_min_size(
        rect.min + egui::vec2(16.0 + CHECK, 7.0),
        egui::vec2((thumb_h * 16.0 / 9.0).round(), thumb_h),
    );
    let uri = w.thumbnail.as_deref().map(widgets::thumbnail_uri);
    widgets::thumbnail(ui, thumb, uri.as_deref(), w.kind, CornerRadius::same(6));

    let more_rect = Rect::from_min_size(
        egui::pos2(rect.right() - 36.0, rect.center().y - 14.0),
        egui::vec2(28.0, 28.0),
    );
    let text_left = thumb.right() + 12.0;
    let cols_right = more_rect.left() - 10.0;
    let mut remaining = cols_right - text_left;
    let date_w = if remaining >= 520.0 { 150.0 } else { 0.0 };
    remaining -= date_w;
    let size_w = if remaining >= 380.0 { 84.0 } else { 0.0 };
    remaining -= size_w;
    let title_w = if remaining >= 300.0 {
        (remaining * 0.45).clamp(180.0, 380.0)
    } else {
        remaining
    };
    let folder_w = remaining - title_w - 16.0;

    let badge_w = if tile.instances.is_empty() { 0.0 } else { 64.0 };
    let title = widgets::elided(
        ui.painter(),
        egui::pos2(text_left, rect.top() + 10.0),
        Align2::LEFT_TOP,
        &w.title,
        FontId::proportional(14.0),
        p.text_strong,
        title_w - badge_w,
    );
    if let Some(label) = playing_label(v, &tile.instances) {
        widgets::badge(
            ui.painter(),
            egui::pos2(title.right() + 8.0, title.center().y),
            Align2::LEFT_CENTER,
            &label,
            p.accent,
            p.on_accent,
        );
    }
    // Columns already show folder, size and dates; the second line then names the author.
    let sub_key = match key {
        Key::Folder if folder_w >= 60.0 => Key::Title,
        Key::Size if size_w > 0.0 => Key::Title,
        Key::Added | Key::Modified if date_w > 0.0 => Key::Title,
        k => k,
    };
    widgets::elided(
        ui.painter(),
        egui::pos2(text_left, rect.top() + 31.0),
        Align2::LEFT_TOP,
        &subtitle(w, sub_key),
        FontId::proportional(12.0),
        p.text_weak,
        title_w,
    );
    let column = FontId::proportional(12.5);
    if folder_w >= 60.0 {
        if let Some(folder) = order::detail(w, Key::Folder) {
            widgets::elided(
                ui.painter(),
                egui::pos2(text_left + title_w + 16.0, rect.center().y),
                Align2::LEFT_CENTER,
                &folder,
                column.clone(),
                p.text_weak,
                folder_w,
            );
        }
    }
    if size_w > 0.0 {
        if let Some(size) = order::detail(w, Key::Size) {
            widgets::elided(
                ui.painter(),
                egui::pos2(cols_right - date_w - size_w, rect.center().y),
                Align2::LEFT_CENTER,
                &size,
                column.clone(),
                p.text_weak,
                size_w - 8.0,
            );
        }
    }
    if date_w > 0.0 {
        let date_key = if key == Key::Modified {
            Key::Modified
        } else {
            Key::Added
        };
        if let Some(date) = order::detail(w, date_key) {
            widgets::elided(
                ui.painter(),
                egui::pos2(cols_right - date_w, rect.center().y),
                Align2::LEFT_CENTER,
                &date,
                column,
                p.text_weak,
                date_w - 4.0,
            );
        }
    }
    more_button(ui, v, w, &tile, more_rect, pick, actions);
    finish_tile(v, w, tile, pick, actions);
}

fn more_button(
    ui: &mut egui::Ui,
    v: &View<'_>,
    w: &Summary,
    tile: &Tile<'_>,
    more_rect: Rect,
    pick: &mut Pick<'_>,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(("more", &w.id))
            .max_rect(more_rect)
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    let more = child
        .add(
            Button::new(RichText::new("···").size(16.0).color(if tile.hovered {
                p.text
            } else {
                p.text_faint
            }))
            .frame_when_inactive(false)
            .min_size(more_rect.size())
            .corner_radius(CornerRadius::same(7)),
        )
        .on_hover_text("More actions");
    egui::Popup::menu(&more)
        .id(tile.id.with("menu"))
        .show(|ui| menu_items(ui, v, w, pick.sel, actions));
}

/// Click applies, or selects once a selection is under way; Ctrl+click toggles and
/// Shift+click extends the selection. Right click opens the same menu as the more button.
fn finish_tile(
    v: &View<'_>,
    w: &Summary,
    tile: Tile<'_>,
    pick: &mut Pick<'_>,
    actions: &mut Vec<Action>,
) {
    let selecting = !pick.sel.is_empty();
    let response = tile
        .response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(hover_text(w, selecting));
    if response.clicked() {
        let mods = response.ctx.input(|i| i.modifiers);
        if mods.shift {
            pick.sel.range(&w.id, pick.visible);
        } else if mods.command || selecting {
            pick.sel.toggle(&w.id);
        } else {
            actions.push(Action::Apply {
                id: w.id.clone(),
                display: None,
            });
        }
    }
    response.context_menu(|ui| menu_items(ui, v, w, pick.sel, actions));
}

fn menu_items(
    ui: &mut egui::Ui,
    v: &View<'_>,
    w: &Summary,
    sel: &mut Selection,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    ui.set_min_width(230.0);
    if sel.len() > 1 && sel.contains(&w.id) {
        bulk_menu_items(ui, sel, actions);
        return;
    }
    if v.arrangement == Arrangement::Per && v.displays.len() > 1 {
        for (i, d) in v.displays.iter().enumerate() {
            if ui
                .button(RichText::new(format!(
                    "Apply to {}  {}",
                    i + 1,
                    short_name(d)
                )))
                .clicked()
            {
                actions.push(Action::Apply {
                    id: w.id.clone(),
                    display: Some(d.id.clone()),
                });
            }
        }
    } else if ui.button("Apply").clicked() {
        actions.push(Action::Apply {
            id: w.id.clone(),
            display: None,
        });
    }
    if w.customizable && ui.button("Customize…").clicked() {
        actions.push(Action::Customize { id: w.id.clone() });
    }
    if ui
        .button(if sel.contains(&w.id) {
            "Deselect"
        } else {
            "Select"
        })
        .clicked()
    {
        sel.toggle(&w.id);
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
        actions.push(Action::Reveal {
            path: if w.kind.is_online() {
                w.dir.clone()
            } else {
                PathBuf::from(&w.source)
            },
        });
    }
    if let Some(url) = w.contact.as_deref().filter(|u| u.starts_with("http")) {
        if ui.button("Website").clicked() {
            actions.push(Action::Open {
                url: url.to_string(),
            });
        }
    }
    ui.separator();
    if ui
        .button(RichText::new("Remove…").color(p.danger))
        .clicked()
    {
        actions.push(Action::Delete { id: w.id.clone() });
    }
}

/// The menu of a wallpaper inside a multiple selection acts on the whole selection.
fn bulk_menu_items(ui: &mut egui::Ui, sel: &mut Selection, actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    let ids = sel.ids();
    let n = ids.len();
    if ui.button(format!("Edit {n} wallpapers…")).clicked() {
        actions.push(Action::BulkEdit { ids: ids.clone() });
    }
    if ui.button(format!("New thumbnails for {n}")).clicked() {
        actions.push(Action::BulkThumbnail { ids: ids.clone() });
    }
    if ui.button(format!("Export {n}…")).clicked() {
        actions.push(Action::BulkExport { ids: ids.clone() });
    }
    ui.separator();
    if ui
        .button(RichText::new(format!("Remove {n}…")).color(p.danger))
        .clicked()
    {
        actions.push(Action::BulkDelete { ids });
    }
    ui.separator();
    if ui.button("Clear selection").clicked() {
        sel.clear();
    }
}

fn hover_text(w: &Summary, selecting: bool) -> String {
    let mut s = format!("{}\n{}", w.title, subtitle(w, Key::Title));
    if let Some(d) = &w.desc {
        s.push_str("\n\n");
        s.push_str(d.trim());
    }
    s.push_str("\n\n");
    s.push_str(&order::detail(w, Key::Format).unwrap_or_default());
    s.push_str(" · ");
    s.push_str(&order::detail(w, Key::Folder).unwrap_or_default());
    s.push('\n');
    let facts: Vec<String> = [Key::Size, Key::Added, Key::Modified]
        .into_iter()
        .filter_map(|k| order::detail(w, k))
        .collect();
    s.push_str(&facts.join(" · "));
    s.push_str("\n\n");
    s.push_str(if selecting {
        "Click to select · Shift+click for a range"
    } else {
        "Click to apply · Ctrl+click to select"
    });
    s
}

#[cfg(test)]
mod tests {
    use super::{Selection, count, normalize_link};
    use std::collections::HashSet;

    #[test]
    fn selection_toggles_ranges_and_forgets_removed_ids() {
        let visible: Vec<String> = ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();
        let mut sel = Selection::default();
        assert!(sel.is_empty());
        sel.range("c", &visible);
        assert_eq!(sel.ids(), ["c"], "a range without an anchor is a toggle");
        sel.range("a", &visible);
        assert_eq!(sel.ids(), ["a", "b", "c"]);
        sel.toggle("e");
        sel.range("c", &visible);
        assert_eq!(sel.ids(), ["a", "b", "c", "d", "e"]);
        sel.toggle("b");
        assert_eq!(sel.len(), 4);
        assert!(!sel.contains("b"));
        let present: HashSet<&str> = ["a", "c", "z"].into_iter().collect();
        sel.retain(&present);
        assert_eq!(sel.ids(), ["a", "c"]);
        assert_eq!(sel.anchor, None, "the anchor left with its wallpaper");
        sel.select_all(["z", "a"].into_iter());
        assert_eq!(sel.ids(), ["a", "c", "z"]);
        sel.clear();
        assert!(sel.is_empty());
        assert_eq!(count(1, "wallpaper"), "1 wallpaper");
        assert_eq!(count(3, "thumbnail"), "3 thumbnails");
    }

    #[test]
    fn links_are_normalized() {
        assert_eq!(
            normalize_link(" youtube.com/watch?v=x "),
            Some("https://youtube.com/watch?v=x".into())
        );
        assert_eq!(
            normalize_link("http://localhost:8000"),
            Some("http://localhost:8000".into())
        );
        assert_eq!(
            normalize_link("https://example.org"),
            Some("https://example.org".into())
        );
        assert_eq!(normalize_link("ftp://a.b"), None);
        assert_eq!(normalize_link("not a link"), None);
        assert_eq!(normalize_link("hello"), None);
        assert_eq!(normalize_link(""), None);
    }
}
