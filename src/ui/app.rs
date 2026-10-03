use crate::ipc::{AudioDevice, Event, Request, Response, Status};
use crate::model::settings::Theme;
use crate::model::{Arrangement, Kind, Settings, Summary};
use crate::paths::Paths;
use crate::ui::order::{Key as SortKey, Layout as ViewLayout};
use crate::ui::widgets::{self, Toasts};
use crate::ui::{
    Backend, UiMsg, about, customize, edit, library, screens, settings, theme, workshop,
};
use eframe::egui::{self, Frame, Key, Margin, RichText};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Page {
    Library,
    Workshop,
    Screens,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [Page::Library, Page::Workshop, Page::Screens, Page::Settings];

    fn label(self) -> &'static str {
        match self {
            Page::Library => "Library",
            Page::Workshop => "Workshop",
            Page::Screens => "Screens",
            Page::Settings => "Settings",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::Library => "🖼",
            Page::Workshop => "🏪",
            Page::Screens => "🖥",
            Page::Settings => "⚙",
        }
    }

    fn key(self) -> Key {
        match self {
            Page::Library => Key::Num1,
            Page::Workshop => Key::Num2,
            Page::Screens => Key::Num3,
            Page::Settings => Key::Num4,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Page::Library => "library",
            Page::Workshop => "workshop",
            Page::Screens => "screens",
            Page::Settings => "settings",
        }
    }

    fn from_name(name: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.name() == name)
    }

    fn shortcut(self) -> &'static str {
        match self {
            Page::Library => "Ctrl+1",
            Page::Workshop => "Ctrl+2",
            Page::Screens => "Ctrl+3",
            Page::Settings => "Ctrl+4",
        }
    }
}

const PAGE_KEY: &str = "page";
const SORT_KEY: &str = "library.sort";
const SORT_DESCENDING_KEY: &str = "library.descending";
const GROUPED_KEY: &str = "library.grouped";
const LAYOUT_KEY: &str = "library.layout";

pub enum Dialog {
    Edit(Box<edit::Single>),
    Delete {
        id: String,
        title: String,
    },
    DeleteMany {
        ids: Vec<String>,
        titles: Vec<String>,
    },
    EditMany(edit::Many),
}

/// A file name for each export that is new in `folder` and unique within the batch.
fn export_names(folder: &Path, titles: &[String]) -> Vec<PathBuf> {
    let mut used: HashSet<String> = HashSet::new();
    titles
        .iter()
        .map(|title| {
            let base = crate::paths::slug(title);
            let mut n = 1;
            loop {
                let name = if n == 1 {
                    format!("{base}.zip")
                } else {
                    format!("{base}-{n}.zip")
                };
                let path = folder.join(&name);
                if !path.exists() && used.insert(name) {
                    return path;
                }
                n += 1;
            }
        })
        .collect()
}

pub struct App {
    backend: Backend,
    status: Option<Status>,
    library: Vec<Summary>,
    /// Modification times of loaded thumbnails, to reload only the ones that changed.
    thumb_stamps: HashMap<PathBuf, SystemTime>,
    /// Settings as the daemon last reported or acknowledged them.
    saved: Option<Settings>,
    draft: Option<Settings>,
    failed_settings: Option<Settings>,
    devices: Vec<AudioDevice>,
    selected_display: Option<String>,
    library_state: library::State,
    filter: Option<Kind>,
    page: Page,
    about_open: bool,
    customize: customize::Panel,
    settings_state: settings::State,
    workshop: workshop::State,
    dialog: Option<Dialog>,
    toasts: Toasts,
    volume: u8,
    volume_dragging: bool,
    pick_files_requested: bool,
    connect_error: Option<String>,
    last_connect_attempt: Instant,
    quitting: bool,
    paths: Paths,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> App {
        let mut backend = Backend::new();
        backend.connect(&cc.egui_ctx);
        let page = cc
            .storage
            .and_then(|s| s.get_string(PAGE_KEY))
            .and_then(|n| Page::from_name(&n))
            .unwrap_or(Page::Library);
        let mut library_state = library::State::default();
        if let Some(storage) = cc.storage {
            let order = &mut library_state.order;
            if let Some(key) = storage
                .get_string(SORT_KEY)
                .and_then(|n| SortKey::from_name(&n))
            {
                order.set_key(key);
            }
            if let Some(descending) = storage.get_string(SORT_DESCENDING_KEY) {
                order.descending = descending == "true";
            }
            if let Some(grouped) = storage.get_string(GROUPED_KEY) {
                order.grouped = grouped == "true";
            }
            if let Some(layout) = storage
                .get_string(LAYOUT_KEY)
                .and_then(|n| ViewLayout::from_name(&n))
            {
                order.layout = layout;
            }
        }
        App {
            backend,
            status: None,
            library: Vec::new(),
            thumb_stamps: HashMap::new(),
            saved: None,
            draft: None,
            failed_settings: None,
            devices: Vec::new(),
            selected_display: None,
            library_state,
            filter: None,
            page,
            about_open: false,
            customize: customize::Panel::default(),
            settings_state: settings::State::default(),
            workshop: workshop::State::new(&paths.cache_dir),
            dialog: None,
            toasts: Toasts::default(),
            volume: 75,
            volume_dragging: false,
            pick_files_requested: false,
            connect_error: None,
            last_connect_attempt: Instant::now(),
            quitting: false,
            paths,
        }
    }

    fn refresh_all(&mut self, ctx: &egui::Context) {
        self.refresh_status();
        self.refresh_library(ctx);
        self.refresh_settings(ctx);
        self.refresh_workshop(ctx);
    }

    fn refresh_workshop(&mut self, ctx: &egui::Context) {
        self.workshop.refresh_status(&self.backend, ctx);
    }

    fn refresh_status(&mut self) {
        match self.backend.call(Request::Status) {
            Ok(Response::Status(s)) => {
                if self
                    .selected_display
                    .as_ref()
                    .is_none_or(|id| !s.displays.iter().any(|d| &d.id == id))
                {
                    self.selected_display =
                        crate::model::display::primary(&s.displays).map(|d| d.id.clone());
                }
                self.status = Some(s);
            }
            Ok(_) => {}
            Err(e) => self.toasts.error(e.to_string()),
        }
    }

    fn refresh_library(&mut self, ctx: &egui::Context) {
        match self.backend.call(Request::Library) {
            Ok(Response::Library(items)) => {
                let mut stamps = HashMap::new();
                for path in items.iter().filter_map(|w| w.thumbnail.as_ref()) {
                    if let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) {
                        if self.thumb_stamps.get(path) != Some(&modified) {
                            ctx.forget_image(&widgets::thumbnail_uri(path));
                        }
                        stamps.insert(path.clone(), modified);
                    }
                }
                self.thumb_stamps = stamps;
                self.library = items;
            }
            Ok(_) => {}
            Err(e) => self.toasts.error(e.to_string()),
        }
    }

    fn refresh_settings(&mut self, ctx: &egui::Context) {
        match self.backend.call(Request::Settings) {
            Ok(Response::Settings(s)) => {
                self.apply_theme(ctx, s.theme);
                if !self.volume_dragging {
                    self.volume = s.volume;
                }
                if self.draft.is_none() || self.draft == self.saved {
                    self.draft = Some(s.clone());
                }
                self.saved = Some(s);
            }
            Ok(_) => {}
            Err(e) => self.toasts.error(e.to_string()),
        }
        if let Ok(Response::Devices(d)) = self.backend.call(Request::AudioDevices) {
            self.devices = d;
        }
    }

    fn apply_theme(&self, ctx: &egui::Context, theme: Theme) {
        ctx.set_theme(match theme {
            Theme::System => egui::ThemePreference::System,
            Theme::Light => egui::ThemePreference::Light,
            Theme::Dark => egui::ThemePreference::Dark,
        });
    }

    /// Send the settings draft once nothing on the page is mid-edit.
    fn commit_settings(&mut self, ctx: &egui::Context, busy: bool) {
        let (Some(draft), Some(saved)) = (&self.draft, &self.saved) else {
            return;
        };
        if saved.theme != draft.theme {
            self.apply_theme(ctx, draft.theme);
        }
        if busy || draft == saved || self.failed_settings.as_ref() == Some(draft) {
            return;
        }
        let mut draft = draft.clone();
        draft.normalize();
        match self.backend.call(Request::SetSettings {
            settings: draft.clone(),
        }) {
            Ok(_) => {
                self.saved = Some(draft.clone());
                self.draft = Some(draft);
                self.failed_settings = None;
            }
            Err(e) => {
                self.failed_settings = self.draft.clone();
                self.toasts.error(e.to_string());
            }
        }
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.backend.rx.try_recv() {
            match msg {
                UiMsg::Connected(client) => {
                    self.failed_settings = None;
                    let was_down = self.connect_error.take().is_some();
                    self.backend.attach(ctx, *client);
                    self.refresh_all(ctx);
                    if was_down {
                        self.toasts.success("Connected");
                    }
                }
                UiMsg::ConnectFailed(message) => {
                    self.backend.connection_failed();
                    self.connect_error = Some(message);
                }
                UiMsg::Event(Event::Library) => self.refresh_library(ctx),
                UiMsg::Event(Event::Layout | Event::Displays | Event::Playback) => {
                    self.refresh_status();
                    self.customize.invalidate_if_gone(self.status.as_ref());
                }
                UiMsg::Event(Event::Settings) => self.refresh_settings(ctx),
                UiMsg::Event(Event::Workshop) => self.refresh_workshop(ctx),
                UiMsg::Event(Event::Error { message }) => self.toasts.error(message),
                UiMsg::Event(Event::Info { message }) => self.toasts.info(message),
                UiMsg::Event(Event::Quit) => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                UiMsg::Workshop(msg) => self.workshop.receive(msg),
                UiMsg::Done { label, result } => match *result {
                    Ok(Response::Wallpaper(w)) => {
                        self.toasts.success(format!("{label} {}", w.title))
                    }
                    Ok(Response::Text(text)) => self.toasts.info(text),
                    Ok(Response::Wallpapers(ws)) => self.toasts.success(match ws.as_slice() {
                        [w] => format!("{label} {}", w.title),
                        ws => format!("{label} {} wallpapers", ws.len()),
                    }),
                    Ok(_) => self.toasts.success(label),
                    Err(e) => self.toasts.error(e.to_string()),
                },
                UiMsg::Batch {
                    verb,
                    noun,
                    done,
                    failed,
                } => {
                    if done > 0 {
                        self.toasts
                            .success(format!("{verb} {}", library::count(done, noun)));
                    }
                    for message in failed.iter().take(3) {
                        self.toasts.error(message.clone());
                    }
                    if failed.len() > 3 {
                        self.toasts
                            .error(format!("{} more failed", failed.len() - 3));
                    }
                }
                UiMsg::Disconnected => {
                    self.backend.disconnect();
                    self.status = None;
                    self.customize.clear();
                    if !self.quitting {
                        self.connect_error = Some("daemon stopped".into());
                    }
                }
            }
        }
        if !self.backend.connected() && !self.quitting {
            if !self.backend.connecting()
                && self.last_connect_attempt.elapsed() > Duration::from_secs(2)
            {
                self.last_connect_attempt = Instant::now();
                self.backend.connect(ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn arrangement(&self) -> Arrangement {
        self.status
            .as_ref()
            .map(|s| s.layout.arrangement)
            .unwrap_or_default()
    }

    fn send(&mut self, req: Request) {
        if let Err(e) = self.backend.call(req) {
            self.toasts.error(e.to_string());
        }
    }

    fn apply(&mut self, id: &str, display: Option<String>) {
        let display = display.or_else(|| self.selected_display.clone());
        self.send(Request::Set {
            target: id.to_string(),
            display,
        });
    }

    fn import_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        for p in paths {
            self.backend.background(
                ctx,
                "Added",
                Request::Import {
                    source: p.to_string_lossy().into_owned(),
                },
            );
        }
    }

    fn pick_files(&mut self, ctx: &egui::Context) {
        let media: Vec<&str> = [Kind::Video, Kind::Gif, Kind::Picture]
            .iter()
            .flat_map(|k| k.extensions().iter().copied())
            .collect();
        let picked = rfd::FileDialog::new()
            .add_filter("Wallpapers", &Kind::importable_extensions())
            .add_filter("Videos, pictures and GIFs", &media)
            .add_filter("Web pages", Kind::Web.extensions())
            .add_filter("Programs", Kind::Program.extensions())
            .add_filter("Lively packages", crate::model::kind::PACKAGE_EXTENSIONS)
            .add_filter("All files", &["*"])
            .pick_files();
        if let Some(files) = picked {
            self.import_paths(ctx, files);
        }
    }

    fn pick_folders(&mut self, ctx: &egui::Context) {
        if let Some(dirs) = rfd::FileDialog::new().pick_folders() {
            self.import_paths(ctx, dirs);
        }
    }

    fn nav(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            theme::logo(ui, 30.0);
            ui.add_space(2.0);
            ui.label(
                RichText::new("Deadly Wallpaper")
                    .size(15.0)
                    .strong()
                    .color(p.text_strong),
            );
        });
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for page in Page::ALL {
            if widgets::nav_item(
                ui,
                self.page == page,
                page.glyph(),
                page.label(),
                page.shortcut(),
            )
            .clicked()
            {
                self.page = page;
            }
        }
        ui.spacing_mut().item_spacing.y = 8.0;
        self.volume_dragging = false;
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if widgets::nav_item(ui, false, "ℹ", "About", "Ctrl+5").clicked() {
                self.about_open = true;
            }
            ui.add_space(8.0);
            widgets::divider(ui);
            ui.add_space(12.0);
            let (paused, can_pause, has_audio) = match &self.status {
                Some(s) => (
                    s.paused,
                    s.active.iter().any(|a| a.kind != Kind::Picture),
                    s.active.iter().any(|a| a.kind.has_audio()),
                ),
                None => (false, false, false),
            };
            if has_audio {
                ui.spacing_mut().slider_width = ui.available_width();
                let slider = ui
                    .add(egui::Slider::new(&mut self.volume, 0..=100).show_value(false))
                    .on_hover_text("Wallpaper volume");
                self.volume_dragging = slider.dragged();
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    match self.backend.call(Request::Volume {
                        value: self.volume.to_string(),
                    }) {
                        Ok(_) => {
                            for s in [&mut self.draft, &mut self.saved].into_iter().flatten() {
                                s.volume = self.volume;
                            }
                        }
                        Err(e) => {
                            self.volume = self.saved.as_ref().map_or(75, |s| s.volume);
                            self.toasts.error(e.to_string());
                        }
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Volume");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{}%", self.volume));
                    });
                });
                ui.add_space(8.0);
            }
            if can_pause {
                let label = if paused {
                    "▶  Resume all"
                } else {
                    "⏸  Pause all"
                };
                if ui
                    .add_sized([ui.available_width(), 32.0], theme::secondary_button(label))
                    .clicked()
                {
                    self.send(Request::Play { play: paused });
                }
            }
        });
    }

    fn connection_banner(&mut self, ui: &mut egui::Ui) {
        if self.backend.connected() {
            return;
        }
        let p = theme::palette(ui);
        let frame = Frame::new()
            .fill(p.surface)
            .stroke(egui::Stroke::new(1.0, p.warning.gamma_multiply(0.6)))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 10));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0).color(p.warning));
                match &self.connect_error {
                    Some(e) => {
                        ui.label(RichText::new("Daemon not reachable").color(p.text_strong));
                        ui.label(RichText::new(e).color(p.text_weak));
                    }
                    None => {
                        ui.label(RichText::new("Starting…").color(p.text_strong));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::secondary_button("Retry")).clicked() {
                        self.last_connect_attempt = Instant::now();
                        self.backend.connect(ui.ctx());
                    }
                });
            });
        });
        ui.add_space(12.0);
    }

    fn library_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let (displays, active): (Vec<_>, Vec<_>) = match &self.status {
            Some(s) => (s.displays.clone(), s.active.clone()),
            None => (Vec::new(), Vec::new()),
        };
        let view = library::View {
            items: &self.library,
            filter: self.filter,
            displays: &displays,
            active: &active,
            arrangement: self.arrangement(),
            selected_display: self.selected_display.as_deref(),
            connected: self.backend.connected(),
            hovering_files: ctx.input(|i| !i.raw.hovered_files.is_empty()),
            shortcuts: self.dialog.is_none() && !self.about_open,
        };
        let actions = library::page(ui, &view, &mut self.library_state);
        self.library_actions(ctx, actions);
    }

    fn workshop_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let view = workshop::View {
            connected: self.backend.connected(),
        };
        let actions = workshop::page(ui, &view, &mut self.workshop, &self.backend);
        for action in actions {
            match action {
                workshop::Action::Download { id, title, author } => {
                    let label = match &title {
                        Some(t) => format!("Requested {t}"),
                        None => format!("Requested item {id}"),
                    };
                    self.backend.background(
                        ctx,
                        label,
                        Request::WorkshopGet {
                            id,
                            title,
                            author,
                            display: None,
                        },
                    );
                }
                workshop::Action::Cancel { id } => {
                    self.backend
                        .background(ctx, "Cancelled", Request::WorkshopCancel { id });
                }
                workshop::Action::Sync => {
                    self.backend
                        .background(ctx, "Synced", Request::WorkshopSync);
                }
                workshop::Action::Apply { wallpaper } => self.apply(&wallpaper, None),
                workshop::Action::Open { url } => crate::ui::open_url(&url),
                workshop::Action::GoToSettings => {
                    self.settings_state.show_wallpaper_engine();
                    self.page = Page::Settings;
                }
            }
        }
    }

    fn screens_page(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            theme::page_header(ui, "Screens", None, |_| {});
            ui.add_space(12.0);
            widgets::empty_state(ui, "🖥", "Connecting…", "", None);
            return;
        };
        let selected = status
            .displays
            .iter()
            .find(|d| Some(&d.id) == self.selected_display.as_ref())
            .or_else(|| crate::model::display::primary(&status.displays))
            .map(|d| d.id.clone());
        self.selected_display = selected.clone();
        let active = status.active.iter().find(|a| {
            Some(&a.display) == selected.as_ref() || status.layout.arrangement != Arrangement::Per
        });
        let root = active
            .and_then(|a| self.library.iter().find(|w| w.id == a.wallpaper))
            .map(|w| {
                if w.absolute {
                    PathBuf::from(&w.source)
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or(w.dir.clone())
                } else {
                    w.dir.clone()
                }
            });
        let view = screens::View {
            status: &status,
            library: &self.library,
            selected: selected.as_deref(),
        };
        let actions = screens::page(ui, &view, |ui| {
            if let Some(a) = active.filter(|a| a.customizable) {
                match self.customize.ensure(
                    &mut self.backend,
                    &a.wallpaper,
                    selected.as_deref(),
                    root.unwrap_or_default(),
                ) {
                    Err(e) => {
                        ui.colored_label(ui.visuals().error_fg_color, e.to_string());
                    }
                    Ok(()) if self.customize.is_empty() => {}
                    Ok(()) => self.customize.ui(ui, &mut self.backend, &mut self.toasts),
                }
            }
        });
        for action in actions {
            match action {
                screens::Action::Select(id) => self.selected_display = Some(id),
                screens::Action::Send(req) => self.send(req),
                screens::Action::GoToLibrary => self.page = Page::Library,
            }
        }
    }

    fn settings_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.set_max_width(ui.available_width().min(780.0));
        theme::page_header(ui, "Settings", None, |_| {});
        ui.add_space(14.0);
        let Some(mut draft) = self.draft.clone() else {
            widgets::empty_state(ui, "⚙", "Connecting…", "", None);
            return;
        };
        let (displays, capabilities) = match &self.status {
            Some(st) => (st.displays.clone(), st.capabilities.clone()),
            None => (Vec::new(), Default::default()),
        };
        settings::tabs(ui, &mut self.settings_state);
        let busy = egui::ScrollArea::vertical()
            .id_salt("settings")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                let cx = settings::Context {
                    devices: &self.devices,
                    displays: &displays,
                    capabilities: &capabilities,
                    steam: self.workshop.status.as_ref().map(|s| &s.steam),
                };
                settings::ui(ui, &mut draft, &mut self.settings_state, &cx)
            })
            .inner;
        self.draft = Some(draft);
        self.commit_settings(ctx, busy);
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        let view = about::View {
            status: self.status.as_ref(),
            settings: self.saved.as_ref(),
            paths: &self.paths,
        };
        for action in about::show(ctx, &view, &mut self.about_open) {
            match action {
                about::Action::Open(path) => crate::ui::open_url(&path.to_string_lossy()),
                about::Action::CopiedDetails => self.toasts.success("Details copied"),
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let p = theme::palette_of(ctx);
        let frame = theme::dialog_frame(ctx);
        let mut next = None;
        match dialog {
            Dialog::Edit(mut state) => {
                let modal = egui::Modal::new(egui::Id::new("edit"))
                    .frame(frame)
                    .show(ctx, |ui| edit::single(ui, &mut state));
                match modal.inner {
                    Some(edit::Outcome::Save) => self.send(Request::EditInfo {
                        wallpaper: state.entry.id.clone(),
                        patch: state.patch(),
                    }),
                    Some(edit::Outcome::Cancel) => {}
                    None if !modal.should_close() => next = Some(Dialog::Edit(state)),
                    None => {}
                }
            }
            Dialog::Delete { id, title } => {
                let modal =
                    egui::Modal::new(egui::Id::new("delete"))
                        .frame(frame)
                        .show(ctx, |ui| {
                            ui.set_width(460.0);
                            ui.label(
                                RichText::new(format!("Remove “{title}”?"))
                                    .size(18.0)
                                    .strong()
                                    .color(p.text_strong),
                            );
                            ui.add_space(14.0);
                            let mut done = false;
                            widgets::dialog_buttons(ui, |ui| {
                                if ui.add(theme::danger("Remove")).clicked() {
                                    self.send(Request::Delete {
                                        wallpaper: id.clone(),
                                    });
                                    done = true;
                                }
                                if ui.add(theme::secondary_button("Cancel")).clicked() {
                                    done = true;
                                }
                            });
                            done
                        });
                if !modal.inner && !modal.should_close() {
                    next = Some(Dialog::Delete { id, title });
                }
            }
            Dialog::DeleteMany { ids, titles } => {
                let modal = egui::Modal::new(egui::Id::new("delete-many"))
                    .frame(frame)
                    .show(ctx, |ui| {
                        ui.set_width(460.0);
                        ui.label(
                            RichText::new(format!(
                                "Remove {}?",
                                library::count(ids.len(), "wallpaper")
                            ))
                            .size(18.0)
                            .strong()
                            .color(p.text_strong),
                        );
                        ui.add_space(8.0);
                        for title in titles.iter().take(6) {
                            ui.label(RichText::new(title).color(p.text_weak));
                        }
                        if titles.len() > 6 {
                            ui.label(
                                RichText::new(format!("and {} more", titles.len() - 6))
                                    .color(p.text_weak),
                            );
                        }
                        ui.add_space(14.0);
                        let mut done = false;
                        widgets::dialog_buttons(ui, |ui| {
                            if ui.add(theme::danger("Remove")).clicked() {
                                self.backend.batch(
                                    ctx,
                                    "Removed",
                                    "wallpaper",
                                    ids.iter()
                                        .map(|id| Request::Delete {
                                            wallpaper: id.clone(),
                                        })
                                        .collect(),
                                );
                                done = true;
                            }
                            if ui.add(theme::secondary_button("Cancel")).clicked() {
                                done = true;
                            }
                        });
                        done
                    });
                if !modal.inner && !modal.should_close() {
                    next = Some(Dialog::DeleteMany { ids, titles });
                }
            }
            Dialog::EditMany(mut state) => {
                let modal = egui::Modal::new(egui::Id::new("edit-many"))
                    .frame(frame)
                    .show(ctx, |ui| edit::many(ui, &mut state));
                match modal.inner {
                    Some(edit::Outcome::Save) => {
                        let patch = state.patch();
                        self.backend.batch(
                            ctx,
                            "Updated",
                            "wallpaper",
                            state
                                .ids
                                .iter()
                                .map(|id| Request::EditInfo {
                                    wallpaper: id.clone(),
                                    patch: patch.clone(),
                                })
                                .collect(),
                        );
                    }
                    Some(edit::Outcome::Cancel) => {}
                    None if !modal.should_close() => next = Some(Dialog::EditMany(state)),
                    None => {}
                }
            }
        }
        self.dialog = next;
    }

    /// A wallpaper's title, or its id when it has already left the library.
    fn title_of(&self, id: &str) -> String {
        self.library
            .iter()
            .find(|w| w.id == id)
            .map(|w| w.title.clone())
            .unwrap_or_else(|| id.to_string())
    }

    fn library_actions(&mut self, ctx: &egui::Context, actions: Vec<library::Action>) {
        for action in actions {
            match action {
                library::Action::Apply { id, display } => self.apply(&id, display),
                library::Action::Customize { id } => {
                    let running = self.status.as_ref().and_then(|s| {
                        s.active
                            .iter()
                            .find(|a| a.wallpaper == id)
                            .map(|a| a.display.clone())
                    });
                    match running {
                        Some(display) => self.selected_display = Some(display),
                        None => self.apply(&id, None),
                    }
                    self.page = Page::Screens;
                }
                library::Action::Edit { id } => {
                    if let Some(w) = self.library.iter().find(|w| w.id == id) {
                        self.dialog = Some(Dialog::Edit(Box::new(edit::Single::of(w))));
                    }
                }
                library::Action::Export { id } => {
                    let title = self.title_of(&id);
                    if let Some(file) = rfd::FileDialog::new()
                        .add_filter("Lively package", &["zip"])
                        .set_file_name(format!("{}.zip", crate::paths::slug(&title)))
                        .save_file()
                    {
                        self.backend.background(
                            ctx,
                            format!("Exported {title}"),
                            Request::Export {
                                wallpaper: id,
                                file,
                            },
                        );
                    }
                }
                library::Action::Reveal { path } => crate::ui::reveal(&path),
                library::Action::Thumbnail { id } => self.backend.background(
                    ctx,
                    "Thumbnail updated",
                    Request::Thumbnail { wallpaper: id },
                ),
                library::Action::Delete { id } => {
                    let title = self.title_of(&id);
                    self.dialog = Some(Dialog::Delete { id, title });
                }
                library::Action::Open { url } => crate::ui::open_url(&url),
                library::Action::PickFiles => self.pick_files(ctx),
                library::Action::PickFolders => self.pick_folders(ctx),
                library::Action::AddLink { url } => {
                    self.backend
                        .background(ctx, "Added", Request::Import { source: url })
                }
                library::Action::SelectDisplay(id) => self.selected_display = Some(id),
                library::Action::Filter(kind) => self.filter = kind,
                library::Action::GoToScreens => self.page = Page::Screens,
                library::Action::BulkEdit { ids } => {
                    self.dialog = Some(Dialog::EditMany(edit::Many::new(ids)));
                }
                library::Action::BulkThumbnail { ids } => self.backend.batch(
                    ctx,
                    "Updated",
                    "thumbnail",
                    ids.into_iter()
                        .map(|id| Request::Thumbnail { wallpaper: id })
                        .collect(),
                ),
                library::Action::BulkExport { ids } => {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        let titles: Vec<String> = ids.iter().map(|id| self.title_of(id)).collect();
                        let files = export_names(&folder, &titles);
                        self.backend.batch(
                            ctx,
                            "Exported",
                            "wallpaper",
                            ids.into_iter()
                                .zip(files)
                                .map(|(wallpaper, file)| Request::Export { wallpaper, file })
                                .collect(),
                        );
                    }
                }
                library::Action::BulkDelete { ids } => {
                    let titles = ids.iter().map(|id| self.title_of(id)).collect();
                    self.dialog = Some(Dialog::DeleteMany { ids, titles });
                }
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() || self.about_open {
            return;
        }
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::COMMAND, Key::Num5) {
                self.about_open = true;
                return;
            }
            for page in Page::ALL {
                if i.consume_key(egui::Modifiers::COMMAND, page.key()) {
                    self.page = page;
                }
            }
            if i.consume_key(egui::Modifiers::COMMAND, Key::F) {
                self.page = Page::Library;
                self.library_state.search_focus = true;
            }
            if i.consume_key(egui::Modifiers::COMMAND, Key::O) {
                self.page = Page::Library;
                self.pick_files_requested = true;
            }
        });
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(PAGE_KEY, self.page.name().to_owned());
        let order = self.library_state.order;
        storage.set_string(SORT_KEY, order.key.name().to_owned());
        storage.set_string(SORT_DESCENDING_KEY, order.descending.to_string());
        storage.set_string(GROUPED_KEY, order.grouped.to_string());
        storage.set_string(LAYOUT_KEY, order.layout.name().to_owned());
    }

    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.handle_messages(ctx);
        self.shortcuts(ctx);
        if self.page != Page::Settings {
            self.commit_settings(ctx, false);
        }
        if std::mem::take(&mut self.pick_files_requested) {
            self.pick_files(ctx);
        }
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.import_paths(ctx, dropped);
        }
        let p = theme::palette(root);
        let nav_frame = Frame::new().fill(p.sidebar).inner_margin(Margin {
            left: 12,
            right: 12,
            top: 18,
            bottom: 14,
        });
        egui::Panel::left("nav")
            .exact_size(216.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(nav_frame)
            .show(root, |ui| self.nav(ui));
        let content_frame = Frame::new().fill(p.bg).inner_margin(Margin {
            left: 28,
            right: 28,
            top: 22,
            bottom: 16,
        });
        egui::CentralPanel::default()
            .frame(content_frame)
            .show(root, |ui| {
                self.connection_banner(ui);
                match self.page {
                    Page::Library => self.library_page(ctx, ui),
                    Page::Workshop => self.workshop_page(ctx, ui),
                    Page::Screens => self.screens_page(ui),
                    Page::Settings => self.settings_page(ctx, ui),
                }
            });
        self.dialogs(ctx);
        self.about_dialog(ctx);
        self.toasts.show(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::export_names;

    #[test]
    fn export_names_avoid_existing_files_and_each_other() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("rain.zip"), "taken").unwrap();
        let names = export_names(
            dir.path(),
            &["Rain".into(), "rain".into(), "Snow".into(), "Snow!".into()],
        );
        let files: Vec<String> = names
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            files,
            ["rain-2.zip", "rain-3.zip", "snow.zip", "snow-2.zip"]
        );
        assert!(names.iter().all(|p| p.starts_with(dir.path())));
    }
}
