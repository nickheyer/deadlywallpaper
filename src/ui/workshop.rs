//! The Steam Workshop page: browse Wallpaper Engine's workshop, have Steam download items,
//! watch them arrive and keep track of what Steam holds.

use crate::error::Result;
use crate::ipc::{DownloadPhase, Request, Response, SteamClient, WorkshopDownload, WorkshopStatus};
use crate::model::Kind;
use crate::ui::order::{date_text, size_text};
use crate::ui::{Backend, UiMsg, theme, widgets};
use crate::we::project::ProjectType;
use crate::we::workshop::{self, Client, Item, Page, Query, Rating, Sort, TREND_DAYS};
use eframe::egui::load::TexturePoll;
use eframe::egui::{
    self, Align, Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Layout, Margin,
    Rect, RichText, Sense, Stroke, StrokeKind, UiBuilder,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub enum Action {
    /// Have Steam fetch an item and add it to the library; items Steam already holds are
    /// added right away.
    Download {
        id: u64,
        title: Option<String>,
        author: Option<String>,
    },
    Cancel {
        id: u64,
    },
    Sync,
    Apply {
        wallpaper: String,
    },
    Open {
        url: String,
    },
    GoToSettings,
}

pub struct View {
    pub connected: bool,
}

/// Results of background work, delivered through [`UiMsg::Workshop`].
pub enum Msg {
    Listing {
        query: Query,
        result: Result<Page>,
    },
    Preview {
        url: String,
        result: Result<PathBuf>,
    },
    Details {
        id: u64,
        result: Result<Item>,
    },
    Status(Result<WorkshopStatus>),
}

enum Preview {
    Loading,
    Ready(PathBuf),
    Failed(String),
}

struct Listing {
    page: Page,
}

/// How Steam and the library stand with respect to one item.
#[derive(Clone, Debug, PartialEq)]
enum ItemState {
    Unknown,
    Downloading(WorkshopDownload),
    Downloaded,
    InLibrary(String),
    Stale(String),
}

pub struct State {
    pub query: Query,
    pub search_focus: bool,
    /// The "item id or URL" box in the header.
    pub paste: String,
    client: Arc<Client>,
    listing: Option<Listing>,
    loading: Option<Query>,
    /// A listing just arrived: show it from the top.
    scroll_top: bool,
    error: Option<String>,
    previews: HashMap<String, Preview>,
    selected: Option<u64>,
    details: HashMap<u64, Item>,
    detail_loading: Option<u64>,
    detail_error: Option<(u64, String)>,
    pub status: Option<WorkshopStatus>,
    status_error: Option<String>,
    status_loading: bool,
}

const MIN_CARD_W: f32 = 200.0;
const MAX_CARD_W: f32 = 280.0;
const META_H: f32 = 62.0;
const GAP: f32 = 14.0;
const DETAILS_W: f32 = 360.0;
const PREVIEW_PX: u32 = 322;
const MAX_PREVIEW_DOWNLOADS: usize = 6;

impl State {
    pub fn new(cache_dir: &Path) -> State {
        State {
            query: Query::default(),
            search_focus: false,
            paste: String::new(),
            client: Arc::new(Client::new(cache_dir)),
            listing: None,
            loading: None,
            scroll_top: false,
            error: None,
            previews: HashMap::new(),
            selected: None,
            details: HashMap::new(),
            detail_loading: None,
            detail_error: None,
            status: None,
            status_error: None,
            status_loading: false,
        }
    }

    pub fn receive(&mut self, msg: Msg) {
        match msg {
            Msg::Listing { query, result } => {
                if self.loading.as_ref() != Some(&query) {
                    return;
                }
                self.loading = None;
                match result {
                    Ok(page) => {
                        self.error = None;
                        self.listing = Some(Listing { page });
                        self.scroll_top = true;
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
            Msg::Preview { url, result } => {
                self.previews.insert(
                    url,
                    match result {
                        Ok(path) => Preview::Ready(path),
                        Err(e) => Preview::Failed(e.to_string()),
                    },
                );
            }
            Msg::Details { id, result } => {
                if self.detail_loading == Some(id) {
                    self.detail_loading = None;
                }
                match result {
                    Ok(item) => {
                        self.detail_error = None;
                        self.details.insert(id, item);
                    }
                    Err(e) => self.detail_error = Some((id, e.to_string())),
                }
            }
            Msg::Status(result) => {
                self.status_loading = false;
                match result {
                    Ok(s) => {
                        self.status_error = None;
                        self.status = Some(s);
                    }
                    Err(e) => self.status_error = Some(e.to_string()),
                }
            }
        }
    }

    /// Ask the daemon where Steam is and what it has downloaded.
    pub fn refresh_status(&mut self, backend: &Backend, ctx: &egui::Context) {
        if self.status_loading {
            return;
        }
        self.status_loading = true;
        backend.spawn(ctx, || {
            let result = crate::ipc::client::Client::connect()
                .and_then(|mut c| c.call(&Request::WorkshopStatus))
                .and_then(|r| match r {
                    Response::Workshop(s) => Ok(s),
                    other => Err(crate::error::Error::Ipc(format!(
                        "unexpected reply to the workshop status request: {other:?}"
                    ))),
                });
            UiMsg::Workshop(Msg::Status(result))
        });
    }

    /// Run the current query on a worker.
    fn search(&mut self, backend: &Backend, ctx: &egui::Context) {
        let query = self.query.clone();
        if self.loading.as_ref() == Some(&query) {
            return;
        }
        self.loading = Some(query.clone());
        self.error = None;
        let client = self.client.clone();
        backend.spawn(ctx, move || {
            let result = client.browse(&query);
            UiMsg::Workshop(Msg::Listing { query, result })
        });
    }

    /// Start over from page one with the filters as they are now.
    fn search_fresh(&mut self, backend: &Backend, ctx: &egui::Context) {
        self.query.page = 1;
        self.search(backend, ctx);
    }

    fn preview_downloads_in_flight(&self) -> usize {
        self.previews
            .values()
            .filter(|p| matches!(p, Preview::Loading))
            .count()
    }

    /// Fetch a preview image once, keeping a few downloads going at a time.
    fn want_preview(&mut self, backend: &Backend, ctx: &egui::Context, url: &str) {
        if self.previews.contains_key(url)
            || self.preview_downloads_in_flight() >= MAX_PREVIEW_DOWNLOADS
        {
            return;
        }
        self.previews.insert(url.to_string(), Preview::Loading);
        let client = self.client.clone();
        let url = url.to_string();
        backend.spawn(ctx, move || {
            let result = client.preview(&url);
            UiMsg::Workshop(Msg::Preview { url, result })
        });
    }

    fn want_details(&mut self, backend: &Backend, ctx: &egui::Context, id: u64) {
        if self.details.contains_key(&id)
            || self.detail_loading == Some(id)
            || self.detail_error.as_ref().is_some_and(|(e, _)| *e == id)
        {
            return;
        }
        self.detail_loading = Some(id);
        let client = self.client.clone();
        backend.spawn(ctx, move || {
            let result = client.item(id);
            UiMsg::Workshop(Msg::Details { id, result })
        });
    }

    /// Why Steam's client library cannot be used on this machine, when the daemon says so;
    /// nothing can be downloaded then.
    fn unavailable(&self) -> Option<&str> {
        match self.status.as_ref().map(|s| &s.client) {
            Some(SteamClient::Unavailable { reason }) => Some(reason),
            _ => None,
        }
    }

    fn item_state(&self, id: u64) -> ItemState {
        let Some(status) = &self.status else {
            return ItemState::Unknown;
        };
        if let Some(d) = status.downloads.iter().find(|d| d.id == id) {
            return ItemState::Downloading(d.clone());
        }
        match status.items.iter().find(|i| i.id == id) {
            Some(i) => match (&i.wallpaper, i.stale) {
                (Some(w), true) => ItemState::Stale(w.clone()),
                (Some(w), false) => ItemState::InLibrary(w.clone()),
                (None, _) => ItemState::Downloaded,
            },
            None => ItemState::Unknown,
        }
    }

    /// The item as the details panel shows it: full details when fetched, else the listing's.
    fn item(&self, id: u64) -> Option<Item> {
        self.details.get(&id).cloned().or_else(|| {
            self.listing
                .as_ref()
                .and_then(|l| l.page.items.iter().find(|i| i.id == id).cloned())
        })
    }
}

pub fn page(ui: &mut egui::Ui, v: &View, state: &mut State, backend: &Backend) -> Vec<Action> {
    let mut actions = Vec::new();
    let ctx = ui.ctx().clone();
    if state.listing.is_none() && state.loading.is_none() && state.error.is_none() {
        state.search(backend, &ctx);
    }
    if state.status.is_none() && v.connected && state.status_error.is_none() {
        state.refresh_status(backend, &ctx);
    }

    let subtitle = state.listing.as_ref().map(|l| {
        let items = if l.page.total == 1 {
            "1 item".to_string()
        } else {
            format!("{} items", group_digits(l.page.total))
        };
        if l.page.pages > 1 {
            format!("{items} · page {} of {}", l.page.page.max(1), l.page.pages)
        } else {
            items
        }
    });
    theme::page_header(ui, "Workshop", subtitle.as_deref(), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let fetch = ui
            .add_enabled(
                v.connected
                    && state.unavailable().is_none()
                    && workshop::parse_ref(&state.paste).is_some(),
                theme::primary("Download"),
            )
            .on_hover_text("Have Steam fetch this item and add it to the library")
            .on_disabled_hover_text(match state.unavailable() {
                Some(reason) => reason,
                None if !v.connected => "Connecting to the daemon…",
                None => "Paste an item id or Workshop URL",
            });
        let edit = ui.add(
            egui::TextEdit::singleline(&mut state.paste)
                .hint_text(
                    RichText::new("Item id or Steam URL").color(theme::palette(ui).text_faint),
                )
                .desired_width(230.0)
                .margin(Margin::symmetric(10, 7)),
        );
        let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(id) = workshop::parse_ref(&state.paste).filter(|_| fetch.clicked() || submit) {
            if v.connected && state.unavailable().is_none() {
                actions.push(Action::Download {
                    id,
                    title: None,
                    author: None,
                });
                state.paste.clear();
                state.selected = Some(id);
                state.want_details(backend, &ctx, id);
            }
        }
        if ui
            .add_enabled(
                v.connected && state.unavailable().is_none(),
                theme::secondary_button("Sync subscriptions"),
            )
            .on_hover_text(
                "Fetch every subscription Steam has not downloaded, add every download to the library and refresh the ones Steam updated",
            )
            .on_disabled_hover_text(state.unavailable().unwrap_or("Connecting to the daemon…"))
            .clicked()
        {
            actions.push(Action::Sync);
        }
    });
    ui.add_space(10.0);
    steam_line(ui, state, v, &mut actions);
    ui.add_space(10.0);
    if filter_bar(ui, state) {
        state.search_fresh(backend, &ctx);
    }
    ui.add_space(10.0);

    if let Some(id) = state.selected {
        state.want_details(backend, &ctx, id);
        let panel_frame = Frame::new()
            .fill(theme::palette(ui).surface)
            .stroke(Stroke::new(1.0, theme::palette(ui).stroke))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::same(16));
        egui::Panel::right("workshop-details")
            .exact_size(DETAILS_W)
            .resizable(false)
            .show_separator_line(false)
            .frame(panel_frame)
            .show(ui, |ui| {
                details_panel(ui, v, state, backend, id, &mut actions)
            });
    }
    egui::CentralPanel::default()
        .frame(Frame::NONE)
        .show(ui, |ui| listing(ui, v, state, backend, &mut actions));
    actions
}

/// Where Steam and Wallpaper Engine stand: found, running, signed in, installed; what is
/// downloaded and what is on its way.
fn steam_line(ui: &mut egui::Ui, state: &State, v: &View, actions: &mut Vec<Action>) {
    let p = theme::palette(ui);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !v.connected {
            ui.label(RichText::new("⏳").color(p.text_weak));
            ui.label(
                RichText::new("Connecting to the daemon; browsing works, downloading needs it")
                    .color(p.text_weak),
            );
            return;
        }
        if let Some(e) = &state.status_error {
            ui.label(RichText::new("⚠").color(p.danger));
            ui.label(RichText::new(format!("Steam status: {e}")).color(p.text_weak));
            return;
        }
        let Some(status) = &state.status else {
            ui.label(RichText::new("⏳").color(p.text_weak));
            ui.label(RichText::new("Looking for Steam…").color(p.text_weak));
            return;
        };
        let steam = &status.steam;
        let Some(dir) = &steam.steam_dir else {
            ui.label(RichText::new("⚠").color(p.warning));
            ui.label(
                RichText::new(
                    "Steam was not found. The Steam client fetches Workshop items, so install Steam and Wallpaper Engine, or point Settings at the Steam folder.",
                )
                .color(p.text),
            );
            if ui.link("Open settings").clicked() {
                actions.push(Action::GoToSettings);
            }
            return;
        };
        let dot = |ui: &mut egui::Ui| {
            ui.label(RichText::new("·").color(p.text_faint));
        };
        let (glyph, color) = match (&status.client, &steam.install_dir, &steam.assets_dir) {
            (SteamClient::Running, Some(_), Some(_)) => ("✔", p.success),
            _ => ("⚠", p.warning),
        };
        ui.label(RichText::new(glyph).color(color));
        ui.label(RichText::new(format!("Steam at {}", dir.display())).color(p.text_weak));
        dot(ui);
        match &status.client {
            SteamClient::Running => {
                ui.label(
                    RichText::new(match &status.account {
                        Some(a) => format!("running, signed in as {a}"),
                        None => "running".to_string(),
                    })
                    .color(p.text_weak),
                );
            }
            SteamClient::NotRunning => {
                ui.label(RichText::new("not running: start it to download").color(p.warning));
            }
            SteamClient::Unavailable { reason } => {
                ui.label(RichText::new(reason).color(p.warning));
            }
        }
        dot(ui);
        match &steam.install_dir {
            Some(_) => {
                ui.label(RichText::new("Wallpaper Engine installed").color(p.text_weak));
            }
            None => {
                ui.label(
                    RichText::new(
                        "Wallpaper Engine is not installed: Steam fetches Workshop items only for installed games",
                    )
                    .color(p.warning),
                );
            }
        }
        dot(ui);
        match &steam.assets_dir {
            Some(a) => {
                ui.label(RichText::new(format!("assets at {}", a.display())).color(p.text_weak));
            }
            None => {
                ui.label(RichText::new("assets folder missing").color(p.warning));
                if ui.link("Open settings").clicked() {
                    actions.push(Action::GoToSettings);
                }
            }
        }
        let n = status.items.len();
        dot(ui);
        ui.label(
            RichText::new(format!(
                "{} downloaded",
                if n == 1 {
                    "1 item".to_string()
                } else {
                    format!("{n} items")
                }
            ))
            .color(p.text_weak),
        );
        if !status.downloads.is_empty() {
            dot(ui);
            ui.label(
                RichText::new(format!("{} on the way", status.downloads.len())).color(p.accent),
            );
        }
    });
}

/// Search words and the labelled sort, period, type and rating choices, then the tags being
/// filtered by; returns whether a new search is due.
fn filter_bar(ui: &mut egui::Ui, state: &mut State) -> bool {
    let p = theme::palette(ui);
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
        let edit = widgets::search_box(ui, &mut state.query.text, &mut state.search_focus, 220.0);
        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            changed = true;
        }
        if ui
            .add(theme::secondary_button("Search").min_size(egui::vec2(0.0, 30.0)))
            .on_hover_text("Search the Workshop for these words (Enter does too)")
            .clicked()
        {
            changed = true;
        }
        labelled(ui, "Sort", |ui| {
            egui::ComboBox::from_id_salt("workshop-sort")
                .selected_text(state.query.sort.label())
                .width(150.0)
                .show_ui(ui, |ui| {
                    for sort in Sort::ALL {
                        if ui
                            .selectable_value(&mut state.query.sort, sort, sort.label())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
        if state.query.sort == Sort::Trend {
            labelled(ui, "Period", |ui| {
                egui::ComboBox::from_id_salt("workshop-days")
                    .selected_text(days_label(state.query.days))
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        for days in TREND_DAYS {
                            if ui
                                .selectable_value(&mut state.query.days, days, days_label(days))
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    });
            });
        }
        labelled(ui, "Type", |ui| {
            egui::ComboBox::from_id_salt("workshop-type")
                .selected_text(match state.query.kind {
                    Some(k) => k.tag(),
                    None => "Any",
                })
                .width(120.0)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(&mut state.query.kind, None, "Any")
                        .changed()
                    {
                        changed = true;
                    }
                    for kind in ProjectType::ALL {
                        if ui
                            .selectable_value(&mut state.query.kind, Some(kind), kind.tag())
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
        labelled(ui, "Rating", |ui| {
            egui::ComboBox::from_id_salt("workshop-rating")
                .selected_text(rating_label(state.query.mature))
                .width(120.0)
                .show_ui(ui, |ui| {
                    for mature in [false, true] {
                        if ui
                            .selectable_value(&mut state.query.mature, mature, rating_label(mature))
                            .on_hover_text(if mature {
                                "Items rated Everyone, Questionable or Mature"
                            } else {
                                "Items rated Everyone only"
                            })
                            .changed()
                        {
                            changed = true;
                        }
                    }
                });
        });
    });
    if !state.query.tags.is_empty() {
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.label(RichText::new("Tagged").color(p.text_weak));
            let mut drop = None;
            for (i, tag) in state.query.tags.iter().enumerate() {
                if widgets::chip(ui, true, &format!("{tag}  ✖"))
                    .on_hover_text("Stop filtering by this tag")
                    .clicked()
                {
                    drop = Some(i);
                }
            }
            if let Some(i) = drop {
                state.query.tags.remove(i);
                changed = true;
            }
            if state.query.tags.len() > 1 && ui.link("Clear tags").clicked() {
                state.query.tags.clear();
                changed = true;
            }
        });
    }
    changed
}

/// A control with its name in front, kept on one line when the bar wraps.
fn labelled(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(label).color(p.text_weak));
        add(ui);
    });
}

fn rating_label(mature: bool) -> &'static str {
    if mature { "All ratings" } else { "Everyone" }
}

/// Previous and next page under the cards; returns whether the page changed.
fn pagination(ui: &mut egui::Ui, state: &mut State) -> bool {
    let p = theme::palette(ui);
    let Some(l) = &state.listing else {
        return false;
    };
    let (page, pages) = (l.page.page.max(1), l.page.pages.max(1));
    if pages <= 1 {
        return false;
    }
    let busy = state.loading.is_some();
    let mut changed = false;
    ui.vertical_centered(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if ui
                .add_enabled(
                    page > 1 && !busy,
                    theme::secondary_button("‹  Previous").min_size(egui::vec2(0.0, 30.0)),
                )
                .clicked()
            {
                state.query.page = page - 1;
                changed = true;
            }
            ui.label(RichText::new(format!("Page {page} of {pages}")).color(p.text_weak));
            if ui
                .add_enabled(
                    page < pages && !busy,
                    theme::secondary_button("Next  ›").min_size(egui::vec2(0.0, 30.0)),
                )
                .clicked()
            {
                state.query.page = page + 1;
                changed = true;
            }
        });
    });
    changed
}

fn days_label(days: u32) -> &'static str {
    match days {
        1 => "Today",
        7 => "This week",
        30 => "This month",
        90 => "3 months",
        180 => "6 months",
        _ => "This year",
    }
}

/// The card grid, or what stands in for it while searching or after a failure.
fn listing(
    ui: &mut egui::Ui,
    v: &View,
    state: &mut State,
    backend: &Backend,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    if let Some(e) = state.error.clone() {
        if widgets::empty_state(
            ui,
            "⚠",
            "The Workshop could not be reached",
            &e,
            Some("Retry"),
        ) {
            state.error = None;
            state.search(backend, &ui.ctx().clone());
        }
        return;
    }
    let Some(listing) = &state.listing else {
        ui.add_space((ui.available_height() * 0.25).max(24.0));
        ui.vertical_centered(|ui| {
            ui.add(egui::Spinner::new().size(28.0).color(p.accent));
            ui.add_space(8.0);
            ui.label(RichText::new("Searching the Workshop…").color(p.text_weak));
        });
        return;
    };
    if listing.page.items.is_empty() {
        if widgets::empty_state(ui, "🔍", "Nothing matches", "", Some("Show trending")) {
            state.query = Query::default();
            state.search(backend, &ui.ctx().clone());
        }
        return;
    }
    let items = listing.page.items.clone();
    let stale_results = state.loading.is_some();
    let mut area = egui::ScrollArea::vertical()
        .id_salt("workshop-content")
        .auto_shrink([false; 2]);
    if std::mem::take(&mut state.scroll_top) {
        area = area.vertical_scroll_offset(0.0);
    }
    let mut page_changed = false;
    area.show(ui, |ui| {
        let avail = ui.available_width();
        let cols = (((avail + GAP) / (MIN_CARD_W + GAP)).floor() as usize).max(1);
        let card_w = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32)
            .min(MAX_CARD_W)
            .floor();
        let card_h = card_w + META_H;
        ui.spacing_mut().item_spacing.y = GAP;
        ui.add_space(2.0);
        if stale_results {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0).color(p.accent));
                ui.label(RichText::new("Searching…").small().color(p.text_weak));
            });
        }
        for row in items.chunks(cols) {
            let (row_rect, _) = ui.allocate_exact_size(egui::vec2(avail, card_h), Sense::hover());
            if !ui.is_rect_visible(row_rect) {
                continue;
            }
            for (i, item) in row.iter().enumerate() {
                let rect = Rect::from_min_size(
                    row_rect.min + egui::vec2(i as f32 * (card_w + GAP), 0.0),
                    egui::vec2(card_w, card_h),
                );
                card(ui, v, state, backend, item, rect, actions);
            }
        }
        ui.add_space(6.0);
        page_changed = pagination(ui, state);
        ui.add_space(10.0);
    });
    if page_changed {
        state.search(backend, &ui.ctx().clone());
    }
}

fn kind_of(item: &Item) -> Kind {
    match item.kind() {
        Some(ProjectType::Scene) => Kind::Scene,
        Some(ProjectType::Video) => Kind::Video,
        Some(ProjectType::Web) => Kind::Web,
        Some(ProjectType::Application) => Kind::Program,
        None => Kind::Scene,
    }
}

/// Draw a preview covering `rect`; spinner while it downloads, a glyph with the error when it
/// could not be fetched.
fn preview(
    ui: &mut egui::Ui,
    state: &mut State,
    backend: &Backend,
    item: &Item,
    rect: Rect,
    corner: CornerRadius,
) {
    let p = theme::palette(ui);
    let kind = kind_of(item);
    let url = item.preview_sized(PREVIEW_PX);
    if let Some(url) = &url {
        state.want_preview(backend, &ui.ctx().clone(), url);
    }
    let entry = url.as_deref().and_then(|u| state.previews.get(u));
    match entry {
        Some(Preview::Ready(path)) => {
            let uri = widgets::thumbnail_uri(path);
            let image = egui::Image::from_uri(uri.clone());
            match image.load_for_size(ui.ctx(), rect.size()) {
                Ok(TexturePoll::Ready { texture }) => {
                    let uv = widgets::cover_uv(texture.size, rect.size());
                    image.uv(uv).corner_radius(corner).paint_at(ui, rect);
                }
                Ok(TexturePoll::Pending { .. }) => {
                    widgets::thumbnail(ui, rect, None, kind, corner);
                    ui.put(
                        Rect::from_center_size(rect.center(), egui::Vec2::splat(24.0)),
                        egui::Spinner::new().size(22.0).color(p.text_weak),
                    );
                }
                Err(e) => {
                    widgets::thumbnail(ui, rect, None, kind, corner);
                    failed_glyph(ui, rect, &format!("The preview could not be shown: {e}"));
                }
            }
        }
        Some(Preview::Failed(e)) => {
            widgets::thumbnail(ui, rect, None, kind, corner);
            failed_glyph(
                ui,
                rect,
                &format!("The preview could not be downloaded: {e}"),
            );
        }
        Some(Preview::Loading) => {
            widgets::thumbnail(ui, rect, None, kind, corner);
            ui.put(
                Rect::from_center_size(rect.center(), egui::Vec2::splat(24.0)),
                egui::Spinner::new().size(22.0).color(p.text_weak),
            );
        }
        None => {
            widgets::thumbnail(ui, rect, None, kind, corner);
            if url.is_some() {
                ui.put(
                    Rect::from_center_size(rect.center(), egui::Vec2::splat(24.0)),
                    egui::Spinner::new().size(22.0).color(p.text_weak),
                );
            }
        }
    }
}

fn failed_glyph(ui: &mut egui::Ui, rect: Rect, tooltip: &str) {
    let p = theme::palette(ui);
    let glyph = Rect::from_center_size(rect.center(), egui::Vec2::splat(40.0));
    ui.painter().text(
        glyph.center(),
        Align2::CENTER_CENTER,
        "⚠",
        FontId::proportional(24.0),
        p.warning,
    );
    ui.interact(
        glyph,
        ui.id()
            .with(("preview-failed", rect.min.x as i32, rect.min.y as i32)),
        Sense::hover(),
    )
    .on_hover_text(tooltip);
}

/// A thin bar along the bottom of a preview showing how much of a download has arrived; it
/// pulses while Steam has not said how much there is.
fn progress_bar(ui: &mut egui::Ui, image_rect: Rect, d: &WorkshopDownload) {
    let p = theme::palette(ui);
    let track = Rect::from_min_max(
        egui::pos2(image_rect.left(), image_rect.bottom() - 5.0),
        image_rect.right_bottom(),
    );
    ui.painter()
        .rect_filled(track, CornerRadius::ZERO, Color32::from_black_alpha(140));
    let fill = match d.fraction() {
        Some(f) => Rect::from_min_max(
            track.min,
            egui::pos2(track.left() + track.width() * f, track.bottom()),
        ),
        None => {
            let t = ui.input(|i| i.time) as f32;
            let w = track.width() * 0.3;
            let x = track.left() + (track.width() - w) * (0.5 + 0.5 * (t * 2.0).sin());
            ui.ctx().request_repaint();
            Rect::from_min_max(
                egui::pos2(x, track.top()),
                egui::pos2(x + w, track.bottom()),
            )
        }
    };
    ui.painter().rect_filled(fill, CornerRadius::ZERO, p.accent);
}

fn state_badge(state: &ItemState) -> Option<(String, bool)> {
    Some(match state {
        ItemState::Unknown => return None,
        ItemState::Downloading(d) => (progress_label(d), false),
        ItemState::Downloaded => ("Downloaded".into(), false),
        ItemState::InLibrary(_) => ("In library".into(), true),
        ItemState::Stale(_) => ("Update available".into(), false),
    })
}

/// "Downloading 43%" and the like, for a download under way.
fn progress_label(d: &WorkshopDownload) -> String {
    match (d.phase, d.fraction()) {
        (DownloadPhase::Downloading, Some(f)) => format!("Downloading {:.0}%", f * 100.0),
        (phase, _) => format!("{}…", phase.label()),
    }
}

/// The one action that moves an item forward from where it stands: its label, what it does
/// (nothing while it cannot be done) and why. `unavailable` is the reason Steam's client
/// library cannot be used here, which rules out fetching anything Steam does not hold yet.
fn primary_action(
    item: &Item,
    state: &ItemState,
    v: &View,
    unavailable: Option<&str>,
) -> (String, Option<Action>, String) {
    let download = || Action::Download {
        id: item.id,
        title: Some(item.title.clone()),
        author: item.author.clone(),
    };
    match state {
        ItemState::InLibrary(w) => (
            "Apply".into(),
            Some(Action::Apply {
                wallpaper: w.clone(),
            }),
            "Show it on the selected display".into(),
        ),
        ItemState::Stale(_) => (
            "Update".into(),
            Some(download()),
            "Refresh the library entry from Steam's newer download".into(),
        ),
        ItemState::Downloaded => (
            "Add to library".into(),
            Some(download()),
            "Steam has it already; add it to the library".into(),
        ),
        ItemState::Downloading(d) => (
            progress_label(d),
            None,
            "Steam is fetching it; it joins the library as soon as it lands".into(),
        ),
        ItemState::Unknown if !v.connected => {
            ("Download".into(), None, "Connecting to the daemon…".into())
        }
        ItemState::Unknown => match unavailable {
            Some(reason) => ("Download".into(), None, reason.into()),
            None => (
                "Download".into(),
                Some(download()),
                "Steam subscribes to it and downloads it; it joins the library as soon as it lands"
                    .into(),
            ),
        },
    }
}

fn card(
    ui: &mut egui::Ui,
    v: &View,
    state: &mut State,
    backend: &Backend,
    item: &Item,
    rect: Rect,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    let id = ui.id().with(("workshop-card", item.id));
    let response = ui.interact(rect, id, Sense::click());
    let hovered = ui.rect_contains_pointer(rect);
    let selected = state.selected == Some(item.id);
    let t = ui.ctx().animate_bool_responsive(id.with("hover"), hovered);
    let item_state = state.item_state(item.id);
    let fill = if selected {
        p.surface.lerp_to_gamma(p.accent, 0.14)
    } else {
        p.surface.lerp_to_gamma(p.surface_hover, t)
    };
    let stroke = if selected {
        Stroke::new(2.0, p.accent)
    } else if matches!(item_state, ItemState::InLibrary(_)) {
        Stroke::new(1.5, p.accent)
    } else {
        Stroke::new(1.0, p.stroke.lerp_to_gamma(p.stroke_strong, t))
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(12),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    let image_rect =
        Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.width())).shrink(1.5);
    let corners = CornerRadius {
        nw: 11,
        ne: 11,
        sw: 0,
        se: 0,
    };
    preview(ui, state, backend, item, image_rect, corners);
    if let Some((label, accent)) = state_badge(&item_state) {
        widgets::badge(
            ui.painter(),
            image_rect.min + egui::vec2(8.0, 8.0),
            Align2::LEFT_TOP,
            &label,
            if accent {
                p.accent
            } else {
                Color32::from_black_alpha(170)
            },
            Color32::WHITE,
        );
    }
    if let ItemState::Downloading(d) = &item_state {
        progress_bar(ui, image_rect, d);
    }
    widgets::badge(
        ui.painter(),
        image_rect.right_top() + egui::vec2(-8.0, 8.0),
        Align2::RIGHT_TOP,
        theme::kind_glyph(kind_of(item)),
        Color32::from_black_alpha(150),
        Color32::WHITE,
    );
    if hovered {
        let bar_h = 42.0;
        let bar = Rect::from_min_max(
            egui::pos2(image_rect.left(), image_rect.bottom() - bar_h),
            image_rect.right_bottom(),
        );
        ui.painter().rect_filled(bar, CornerRadius::ZERO, p.overlay);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("workshop-card-actions", item.id))
                .max_rect(bar.shrink2(egui::vec2(8.0, 6.0)))
                .layout(Layout::right_to_left(Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 6.0;
        let (label, action, tip) = primary_action(item, &item_state, v, state.unavailable());
        let enabled = action.is_some();
        if child
            .add_enabled(enabled, theme::primary(&label).small())
            .on_hover_text(&tip)
            .on_disabled_hover_text(&tip)
            .clicked()
        {
            actions.extend(action);
        }
        let glass = |text: &str| {
            Button::new(RichText::new(text).color(Color32::WHITE))
                .fill(Color32::from_white_alpha(40))
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(8))
                .min_size(egui::vec2(0.0, 28.0))
        };
        if matches!(item_state, ItemState::Downloading(ref d) if d.phase != DownloadPhase::Importing)
            && child
                .add(glass("Cancel"))
                .on_hover_text("Stop this download")
                .clicked()
        {
            actions.push(Action::Cancel { id: item.id });
        }
        if child.add(glass("Details")).clicked() {
            state.selected = Some(item.id);
        }
    }
    let meta = Rect::from_min_max(
        egui::pos2(rect.left() + 12.0, image_rect.bottom() + 9.0),
        egui::pos2(rect.right() - 12.0, rect.bottom() - 8.0),
    );
    widgets::elided(
        ui.painter(),
        meta.left_top(),
        Align2::LEFT_TOP,
        &item.title,
        FontId::proportional(14.0),
        p.text_strong,
        meta.width(),
    );
    widgets::elided(
        ui.painter(),
        egui::pos2(meta.left(), meta.top() + 22.0),
        Align2::LEFT_TOP,
        &card_subtitle(item),
        FontId::proportional(12.0),
        p.text_weak,
        meta.width(),
    );
    let response = response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(hover_text(item));
    if response.clicked() {
        state.selected = if selected { None } else { Some(item.id) };
    }
}

fn card_subtitle(item: &Item) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(a) = &item.author {
        parts.push(a.clone());
    }
    if let Some(k) = item.kind() {
        parts.push(k.tag().to_string());
    }
    parts.push(format!("{} subscribers", compact(item.subscriptions)));
    parts.join(" · ")
}

fn hover_text(item: &Item) -> String {
    let mut s = item.title.clone();
    if let Some(a) = &item.author {
        s.push_str(&format!("\nby {a}"));
    }
    let facts: Vec<String> = [
        item.kind().map(|k| k.tag().to_string()),
        item.rating().map(|r| r.tag().to_string()),
        Some(format!("{} subscribers", compact(item.subscriptions))),
        (item.updated > 0).then(|| format!("updated {}", date_text(item.updated))),
    ]
    .into_iter()
    .flatten()
    .collect();
    s.push_str("\n\n");
    s.push_str(&facts.join(" · "));
    if !item.description.trim().is_empty() {
        s.push_str("\n\n");
        s.push_str(&excerpt(&item.description, 240));
    }
    s.push_str("\n\nClick for details");
    s
}

fn excerpt(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let cut: String = t.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// 1234 → "1.2k", 2_345_678 → "2.3M".
fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{}k", n / 1000)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// 2410 → "2,410".
fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn stars_text(item: &Item) -> String {
    match item.stars {
        Some(s) => format!(
            "{}{}  {} votes",
            "★".repeat(s as usize),
            "☆".repeat(5usize.saturating_sub(s as usize)),
            group_digits(item.votes)
        ),
        None if item.votes > 0 => format!("Not rated yet ({} votes)", item.votes),
        None => "Not rated yet".into(),
    }
}

fn details_panel(
    ui: &mut egui::Ui,
    v: &View,
    state: &mut State,
    backend: &Backend,
    id: u64,
    actions: &mut Vec<Action>,
) {
    let p = theme::palette(ui);
    let Some(item) = state.item(id) else {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, "✖", "Close").clicked() {
                    state.selected = None;
                }
            });
        });
        match &state.detail_error {
            Some((e, msg)) if *e == id => {
                widgets::empty_state(ui, "⚠", &format!("Item {id}"), msg, None);
            }
            _ => {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(24.0).color(p.accent));
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("Fetching item {id}…")).color(p.text_weak));
                });
            }
        }
        return;
    };
    let item_state = state.item_state(id);
    let mut new_tags: Option<String> = None;
    egui::ScrollArea::vertical()
        .id_salt(("workshop-details", id))
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let width = ui.available_width();
            let (image_rect, _) = ui.allocate_exact_size(egui::vec2(width, width), Sense::hover());
            preview(
                ui,
                state,
                backend,
                &item,
                image_rect,
                CornerRadius::same(10),
            );
            let close = Rect::from_min_size(
                image_rect.right_top() + egui::vec2(-36.0, 8.0),
                egui::Vec2::splat(28.0),
            );
            let mut close_ui =
                ui.new_child(UiBuilder::new().id_salt("workshop-close").max_rect(close));
            if close_ui
                .add(
                    Button::new(RichText::new("✖").color(Color32::WHITE))
                        .fill(Color32::from_black_alpha(150))
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(14))
                        .min_size(egui::Vec2::splat(28.0)),
                )
                .on_hover_text("Close")
                .clicked()
            {
                state.selected = None;
            }
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    RichText::new(&item.title)
                        .size(17.0)
                        .strong()
                        .color(p.text_strong),
                )
                .wrap(),
            );
            match &item.author {
                Some(a) => theme::weak(ui, &format!("by {a}")),
                None if state.detail_loading == Some(id) => theme::weak(ui, "by …"),
                None => theme::weak(ui, "by an unknown uploader"),
            }
            if let Some((label, accent)) = state_badge(&item_state) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 24.0), Sense::hover());
                widgets::badge(
                    ui.painter(),
                    rect.left_center(),
                    Align2::LEFT_CENTER,
                    &label,
                    if accent { p.accent } else { p.control_active },
                    if accent { p.on_accent } else { p.text },
                );
            }
            if let ItemState::Downloading(d) = &item_state {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
                progress_bar(ui, rect.expand2(egui::vec2(0.0, 0.0)), d);
                if d.total > 0 {
                    theme::weak(
                        ui,
                        &format!("{} of {}", size_text(d.done), size_text(d.total)),
                    );
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                let (label, action, tip) =
                    primary_action(&item, &item_state, v, state.unavailable());
                if ui
                    .add_enabled(action.is_some(), theme::primary(&label))
                    .on_hover_text(&tip)
                    .on_disabled_hover_text(&tip)
                    .clicked()
                {
                    actions.extend(action);
                }
                if let ItemState::Downloading(d) = &item_state {
                    if d.phase != DownloadPhase::Importing
                        && ui
                            .add(theme::secondary_button("Cancel"))
                            .on_hover_text("Stop this download")
                            .clicked()
                    {
                        actions.push(Action::Cancel { id });
                    }
                }
                if let ItemState::Stale(w) = &item_state {
                    if ui
                        .add(theme::secondary_button("Apply"))
                        .on_hover_text("Show the library's copy on the selected display")
                        .clicked()
                    {
                        actions.push(Action::Apply {
                            wallpaper: w.clone(),
                        });
                    }
                }
                if ui
                    .add(theme::secondary_button("Open in Steam"))
                    .on_hover_text(item.url())
                    .clicked()
                {
                    actions.push(Action::Open { url: item.url() });
                }
            });
            ui.add_space(6.0);
            widgets::divider(ui);
            egui::Grid::new(("workshop-stats", id))
                .num_columns(2)
                .spacing([14.0, 6.0])
                .show(ui, |ui| {
                    let mut fact = |label: &str, value: String| {
                        ui.label(RichText::new(label).color(p.text_weak));
                        ui.add(egui::Label::new(RichText::new(value).color(p.text)).wrap());
                        ui.end_row();
                    };
                    fact(
                        "Type",
                        item.kind()
                            .map(|k| k.tag().to_string())
                            .unwrap_or_else(|| "Unknown".into()),
                    );
                    fact(
                        "Rating",
                        item.rating()
                            .map(|r| r.tag().to_string())
                            .unwrap_or_else(|| "Not rated".into()),
                    );
                    fact("Stars", stars_text(&item));
                    fact("Subscribers", group_digits(item.subscriptions));
                    fact("Favorites", group_digits(item.favorites));
                    fact("Views", group_digits(item.views));
                    fact(
                        "Size",
                        if item.size > 0 {
                            size_text(item.size)
                        } else {
                            "Unknown".into()
                        },
                    );
                    fact(
                        "Updated",
                        if item.updated > 0 {
                            date_text(item.updated)
                        } else {
                            "Unknown".into()
                        },
                    );
                    fact(
                        "Posted",
                        if item.created > 0 {
                            date_text(item.created)
                        } else {
                            "Unknown".into()
                        },
                    );
                    fact("Item id", item.id.to_string());
                });
            if !item.tags.is_empty() {
                ui.add_space(4.0);
                widgets::divider(ui);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for tag in &item.tags {
                        let filtered = state.query.tags.iter().any(|t| t == tag)
                            || ProjectType::parse(tag).is_some_and(|k| state.query.kind == Some(k));
                        if widgets::chip(ui, filtered, tag)
                            .on_hover_text(format!("Show items tagged {tag}"))
                            .clicked()
                        {
                            new_tags = Some(tag.clone());
                        }
                    }
                });
            }
            ui.add_space(4.0);
            widgets::divider(ui);
            let description = item.description.trim();
            if description.is_empty() {
                theme::weak(ui, "No description");
            } else {
                ui.add(egui::Label::new(RichText::new(description).color(p.text)).wrap());
            }
            if state.detail_loading == Some(id) {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(12.0).color(p.text_weak));
                    ui.label(
                        RichText::new("Fetching the full description…")
                            .small()
                            .color(p.text_weak),
                    );
                });
            }
            if let Some((e, msg)) = &state.detail_error {
                if *e == id {
                    ui.label(
                        RichText::new(format!("Full details unavailable: {msg}"))
                            .small()
                            .color(p.warning),
                    );
                }
            }
            ui.add_space(8.0);
        });
    if let Some(tag) = new_tags {
        apply_tag_filter(state, &tag);
        state.search_fresh(backend, &ui.ctx().clone());
    }
}

/// Clicking a tag filters by it: type tags become the type filter, rating tags the mature
/// switch, anything else a required tag.
fn apply_tag_filter(state: &mut State, tag: &str) {
    if let Some(kind) = ProjectType::parse(tag) {
        state.query.kind = if state.query.kind == Some(kind) {
            None
        } else {
            Some(kind)
        };
        return;
    }
    if [Rating::Questionable, Rating::Mature]
        .iter()
        .any(|r| r.tag().eq_ignore_ascii_case(tag))
    {
        state.query.mature = true;
        return;
    }
    if Rating::Everyone.tag().eq_ignore_ascii_case(tag) {
        state.query.mature = false;
        return;
    }
    if let Some(i) = state.query.tags.iter().position(|t| t == tag) {
        state.query.tags.remove(i);
    } else {
        state.query.tags.push(tag.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::we::steam::SteamInfo;

    #[test]
    fn numbers_and_stars_read_well() {
        assert_eq!(compact(999), "999");
        assert_eq!(compact(1_234), "1.2k");
        assert_eq!(compact(12_345), "12k");
        assert_eq!(compact(2_345_678), "2.3M");
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(2410), "2,410");
        assert_eq!(group_digits(2_902_770), "2,902,770");
        let mut item = Item {
            stars: Some(4),
            votes: 1234,
            ..Item::default()
        };
        assert_eq!(stars_text(&item), "★★★★☆  1,234 votes");
        item.stars = None;
        item.votes = 3;
        assert_eq!(stars_text(&item), "Not rated yet (3 votes)");
        assert_eq!(excerpt("short", 10), "short");
        assert_eq!(excerpt("a long description here", 6), "a long…");
    }

    #[test]
    fn tag_clicks_become_filters() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::new(dir.path());
        apply_tag_filter(&mut state, "Scene");
        assert_eq!(state.query.kind, Some(ProjectType::Scene));
        apply_tag_filter(&mut state, "Scene");
        assert_eq!(state.query.kind, None);
        apply_tag_filter(&mut state, "Mature");
        assert!(state.query.mature);
        apply_tag_filter(&mut state, "Everyone");
        assert!(!state.query.mature);
        apply_tag_filter(&mut state, "Anime");
        apply_tag_filter(&mut state, "3840 x 2160");
        assert_eq!(state.query.tags, ["Anime", "3840 x 2160"]);
        apply_tag_filter(&mut state, "Anime");
        assert_eq!(state.query.tags, ["3840 x 2160"]);
    }

    #[test]
    fn item_states_follow_the_daemon() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State::new(dir.path());
        assert_eq!(state.item_state(1), ItemState::Unknown);
        state.receive(Msg::Status(Ok(WorkshopStatus {
            steam: SteamInfo::default(),
            client: SteamClient::Running,
            account: None,
            items: vec![
                crate::ipc::WorkshopItemStatus {
                    id: 1,
                    wallpaper: Some("one-000001".into()),
                    stale: false,
                    ..Default::default()
                },
                crate::ipc::WorkshopItemStatus {
                    id: 2,
                    wallpaper: Some("two-000002".into()),
                    stale: true,
                    ..Default::default()
                },
                crate::ipc::WorkshopItemStatus {
                    id: 3,
                    ..Default::default()
                },
            ],
            downloads: vec![WorkshopDownload {
                id: 4,
                title: Some("Four".into()),
                phase: DownloadPhase::Downloading,
                done: 43,
                total: 100,
            }],
        })));
        assert_eq!(
            state.item_state(1),
            ItemState::InLibrary("one-000001".into())
        );
        assert_eq!(state.item_state(2), ItemState::Stale("two-000002".into()));
        assert_eq!(state.item_state(3), ItemState::Downloaded);
        let downloading = state.item_state(4);
        assert!(matches!(&downloading, ItemState::Downloading(d) if d.id == 4));
        assert_eq!(state.item_state(5), ItemState::Unknown);
        let v = View { connected: true };
        let item = Item {
            id: 4,
            ..Item::default()
        };
        let (label, action, _) = primary_action(&item, &downloading, &v, None);
        assert_eq!(label, "Downloading 43%");
        assert!(action.is_none());
        let (label, action, _) =
            primary_action(&item, &ItemState::Unknown, &View { connected: false }, None);
        assert_eq!(label, "Download");
        assert!(action.is_none());
        let (label, action, _) = primary_action(&item, &ItemState::Unknown, &v, None);
        assert_eq!(label, "Download");
        assert!(matches!(action, Some(Action::Download { id: 4, .. })));
        let (label, action, tip) =
            primary_action(&item, &ItemState::Unknown, &v, Some("no Steam here"));
        assert_eq!(
            (label.as_str(), tip.as_str()),
            ("Download", "no Steam here")
        );
        assert!(action.is_none());
        let (_, action, _) = primary_action(&item, &ItemState::Downloaded, &v, Some("no Steam"));
        assert!(matches!(action, Some(Action::Download { id: 4, .. })));
        let (label, action, _) = primary_action(&item, &ItemState::InLibrary("w".into()), &v, None);
        assert_eq!(label, "Apply");
        assert!(matches!(action, Some(Action::Apply { .. })));
        assert_eq!(
            progress_label(&WorkshopDownload {
                phase: DownloadPhase::Queued,
                ..Default::default()
            }),
            "Queued in Steam…"
        );
        assert_eq!(rating_label(true), "All ratings");
    }
}
