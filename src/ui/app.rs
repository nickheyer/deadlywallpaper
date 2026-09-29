//! The application: state, daemon messages, navigation, dialogs and the glue between pages.

use crate::ipc::{AudioDevice, Event, InfoPatch, Request, Response, Status};
use crate::model::settings::Theme;
use crate::model::{Arrangement, Kind, Settings, Summary};
use crate::paths::Paths;
use crate::ui::widgets::{self, Toasts};
use crate::ui::{Backend, UiMsg, about, customize, library, screens, settings, theme};
use eframe::egui::{self, Frame, Key, Margin, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Page {
    Library,
    Screens,
    Settings,
    About,
}

impl Page {
    const ALL: [Page; 4] = [Page::Library, Page::Screens, Page::Settings, Page::About];

    fn label(self) -> &'static str {
        match self {
            Page::Library => "Library",
            Page::Screens => "Screens",
            Page::Settings => "Settings",
            Page::About => "About",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Page::Library => "🖼",
            Page::Screens => "🖥",
            Page::Settings => "⚙",
            Page::About => "ℹ",
        }
    }

    fn key(self) -> Key {
        match self {
            Page::Library => Key::Num1,
            Page::Screens => Key::Num2,
            Page::Settings => Key::Num3,
            Page::About => Key::Num4,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Page::Library => "library",
            Page::Screens => "screens",
            Page::Settings => "settings",
            Page::About => "about",
        }
    }

    fn from_name(name: &str) -> Option<Page> {
        Page::ALL.into_iter().find(|p| p.name() == name)
    }

    fn shortcut(self) -> &'static str {
        match self {
            Page::Library => "Ctrl+1",
            Page::Screens => "Ctrl+2",
            Page::Settings => "Ctrl+3",
            Page::About => "Ctrl+4",
        }
    }
}

/// Storage key of the page to reopen on the next start.
const PAGE_KEY: &str = "page";

pub enum Dialog {
    Edit { id: String, title: String, author: String, desc: String, contact: String, license: String, arguments: String, program: bool },
    Delete { id: String, title: String },
}

pub struct App {
    backend: Backend,
    status: Option<Status>,
    library: Vec<Summary>,
    /// Modification times of loaded thumbnails, to reload only the ones that changed.
    thumb_stamps: HashMap<PathBuf, SystemTime>,
    /// Settings as the daemon last reported or acknowledged them.
    saved: Option<Settings>,
    /// Settings as edited on the Settings page.
    draft: Option<Settings>,
    devices: Vec<AudioDevice>,
    selected_display: Option<String>,
    library_state: library::State,
    filter: Option<Kind>,
    page: Page,
    customize: customize::Panel,
    settings_state: settings::State,
    dialog: Option<Dialog>,
    toasts: Toasts,
    volume: u8,
    volume_dragging: bool,
    pick_files_requested: bool,
    connect_error: Option<String>,
    last_connect_attempt: Instant,
    paths: Paths,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> App {
        let mut backend = Backend::new();
        backend.connect(&cc.egui_ctx);
        let page = cc.storage.and_then(|s| s.get_string(PAGE_KEY)).and_then(|n| Page::from_name(&n)).unwrap_or(Page::Library);
        App {
            backend,
            status: None,
            library: Vec::new(),
            thumb_stamps: HashMap::new(),
            saved: None,
            draft: None,
            devices: Vec::new(),
            selected_display: None,
            library_state: library::State::default(),
            filter: None,
            page,
            customize: customize::Panel::default(),
            settings_state: settings::State::default(),
            dialog: None,
            toasts: Toasts::default(),
            volume: 75,
            volume_dragging: false,
            pick_files_requested: false,
            connect_error: None,
            last_connect_attempt: Instant::now(),
            paths,
        }
    }

    // ---- daemon state ---------------------------------------------------------------------

    fn refresh_all(&mut self, ctx: &egui::Context) {
        self.refresh_status();
        self.refresh_library(ctx);
        self.refresh_settings(ctx);
    }

    fn refresh_status(&mut self) {
        match self.backend.call(Request::Status) {
            Ok(Response::Status(s)) => {
                if self.selected_display.as_ref().is_none_or(|id| !s.displays.iter().any(|d| &d.id == id)) {
                    self.selected_display = crate::model::display::primary(&s.displays).map(|d| d.id.clone());
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
        let (Some(draft), Some(saved)) = (&self.draft, &self.saved) else { return };
        if saved.theme != draft.theme {
            self.apply_theme(ctx, draft.theme);
        }
        if busy || draft == saved {
            return;
        }
        let draft = draft.clone();
        self.saved = Some(draft.clone());
        self.send(Request::SetSettings { settings: draft });
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.backend.rx.try_recv() {
            match msg {
                UiMsg::Connected(client) => {
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
                UiMsg::Event(Event::Error { message }) => self.toasts.error(message),
                UiMsg::Event(Event::Info { message }) => self.toasts.info(message),
                UiMsg::Done { label, result } => match *result {
                    Ok(Response::Wallpaper(w)) => self.toasts.success(format!("{label} {}", w.title)),
                    Ok(Response::Wallpapers(ws)) => self.toasts.success(match ws.as_slice() {
                        [w] => format!("{label} {}", w.title),
                        ws => format!("{label} {} wallpapers", ws.len()),
                    }),
                    Ok(_) => self.toasts.success(label),
                    Err(e) => self.toasts.error(e.to_string()),
                },
                UiMsg::Disconnected => {
                    self.backend.disconnect();
                    self.status = None;
                    self.customize.clear();
                    self.connect_error = Some("daemon stopped".into());
                }
            }
        }
        if !self.backend.connected() {
            if !self.backend.connecting() && self.last_connect_attempt.elapsed() > Duration::from_secs(2) {
                self.last_connect_attempt = Instant::now();
                self.backend.connect(ctx);
            }
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn arrangement(&self) -> Arrangement {
        self.status.as_ref().map(|s| s.layout.arrangement).unwrap_or_default()
    }

    fn send(&mut self, req: Request) {
        if let Err(e) = self.backend.call(req) {
            self.toasts.error(e.to_string());
        }
    }

    fn apply(&mut self, id: &str, display: Option<String>) {
        let display = display.or_else(|| self.selected_display.clone());
        self.send(Request::Set { target: id.to_string(), display });
    }

    fn import_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        for p in paths {
            self.backend.background(ctx, "Added", Request::Import { source: p.to_string_lossy().into_owned() });
        }
    }

    /// One picker for every kind: media, web pages, programs and Lively packages. Programs
    /// without a listed extension come in through "All files".
    fn pick_files(&mut self, ctx: &egui::Context) {
        let media: Vec<&str> = [Kind::Video, Kind::Gif, Kind::Picture].iter().flat_map(|k| k.extensions().iter().copied()).collect();
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

    /// Whole folders: each yields every wallpaper inside it.
    fn pick_folders(&mut self, ctx: &egui::Context) {
        if let Some(dirs) = rfd::FileDialog::new().pick_folders() {
            self.import_paths(ctx, dirs);
        }
    }

    // ---- chrome ---------------------------------------------------------------------------

    fn nav(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.add(theme::logo().fit_to_exact_size(egui::vec2(30.0, 30.0)));
            ui.add_space(2.0);
            ui.label(RichText::new("Deadly Wallpaper").size(15.0).strong().color(p.text_strong));
        });
        ui.add_space(18.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for page in Page::ALL {
            if widgets::nav_item(ui, self.page == page, page.glyph(), page.label(), page.shortcut()).clicked() {
                self.page = page;
            }
        }
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(2.0);
            let (connected, paused, playing) = match &self.status {
                Some(s) => (true, s.paused, s.active.len()),
                None => (false, false, 0),
            };
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                let glyph = match self.volume {
                    0 => "🔇",
                    1..=49 => "🔉",
                    _ => "🔊",
                };
                ui.label(RichText::new(glyph).color(p.text_weak));
                ui.spacing_mut().slider_width = ui.available_width() - 12.0;
                let slider = ui.add_enabled(connected, egui::Slider::new(&mut self.volume, 0..=100).show_value(false)).on_hover_text(format!("Volume {}%", self.volume));
                self.volume_dragging = slider.dragged();
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    for s in [&mut self.draft, &mut self.saved].into_iter().flatten() {
                        s.volume = self.volume;
                    }
                    self.send(Request::Volume { value: self.volume.to_string() });
                }
            });
            if connected {
                let label = if paused { "▶  Resume" } else { "⏸  Pause" };
                if ui.add_sized([ui.available_width(), 34.0], theme::secondary_button(label)).clicked() {
                    self.send(Request::Play { play: paused });
                }
            }
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let (color, text) = if !connected {
                    (p.warning, "Offline")
                } else if paused {
                    (p.warning, "Paused")
                } else if playing == 0 {
                    (p.text_faint, "Idle")
                } else {
                    (p.success, "Playing")
                };
                widgets::dot(ui, color);
                ui.label(RichText::new(text).small().color(p.text_weak));
            });
        });
    }

    fn connection_banner(&mut self, ui: &mut egui::Ui) {
        if self.backend.connected() {
            return;
        }
        let p = theme::palette(ui);
        let frame = Frame::new().fill(p.surface).stroke(egui::Stroke::new(1.0, p.warning.gamma_multiply(0.6))).corner_radius(egui::CornerRadius::same(10)).inner_margin(Margin::symmetric(14, 10));
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

    // ---- pages ----------------------------------------------------------------------------

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
        };
        let actions = library::page(ui, &view, &mut self.library_state);
        self.library_actions(ctx, actions);
    }

    fn screens_page(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            theme::page_header(ui, "Screens", None, |_| {});
            ui.add_space(12.0);
            widgets::empty_state(ui, "🖥", "Connecting…", "", None);
            return;
        };
        let view = screens::View { status: &status, library: &self.library, selected: self.selected_display.as_deref() };
        let actions = egui::ScrollArea::vertical().id_salt("screens").auto_shrink([false; 2]).show(ui, |ui| {
            let actions = screens::page(ui, &view);
            let selected = self.selected_display.clone().or_else(|| crate::model::display::primary(&status.displays).map(|d| d.id.clone()));
            let active = status.active.iter().find(|a| Some(&a.display) == selected.as_ref() || status.layout.arrangement != Arrangement::Per).cloned();
            if let Some(a) = active.filter(|a| a.customizable) {
                ui.add_space(6.0);
                theme::section_title(ui, "Customize");
                ui.add_space(6.0);
                theme::card(ui).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let root = self.library.iter().find(|w| w.id == a.wallpaper).map(|w| if w.absolute { PathBuf::from(&w.source).parent().map(|p| p.to_path_buf()).unwrap_or(w.dir.clone()) } else { w.dir.clone() });
                    match self.customize.ensure(&mut self.backend, &a.wallpaper, selected.as_deref(), root.unwrap_or_default()) {
                        Err(e) => {
                            ui.label(RichText::new(e.to_string()).color(ui.visuals().error_fg_color));
                        }
                        Ok(()) if self.customize.is_empty() => theme::hint(ui, "No properties"),
                        Ok(()) => self.customize.ui(ui, &mut self.backend, &mut self.toasts),
                    }
                });
            }
            ui.add_space(12.0);
            actions
        });
        for action in actions.inner {
            match action {
                screens::Action::Select(id) => self.selected_display = Some(id),
                screens::Action::Send(req) => self.send(req),
                screens::Action::GoToLibrary => self.page = Page::Library,
            }
        }
    }

    fn settings_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
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
        let busy = egui::ScrollArea::vertical()
            .id_salt("settings")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                ui.set_max_width(780.0);
                let cx = settings::Context { devices: &self.devices, displays: &displays, capabilities: &capabilities };
                settings::ui(ui, &mut draft, &mut self.settings_state, &cx)
            })
            .inner;
        self.draft = Some(draft);
        self.commit_settings(ctx, busy);
    }

    fn about_page(&mut self, ui: &mut egui::Ui) {
        let view = about::View { status: self.status.as_ref(), settings: self.saved.as_ref(), paths: &self.paths };
        for action in about::page(ui, &view) {
            match action {
                about::Action::OpenLibraryFolder => {
                    let dir = self.saved.as_ref().map(|s| s.library_dir.clone()).unwrap_or_else(|| self.paths.default_library_dir());
                    crate::ui::reveal(&dir);
                }
                about::Action::OpenLogFile => crate::ui::open_url(&self.paths.log_file().to_string_lossy()),
                about::Action::OpenSource => crate::ui::open_url("https://github.com/nickheyer/deadlywallpaper"),
            }
        }
    }

    // ---- dialogs --------------------------------------------------------------------------

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let p = theme::palette_of(ctx);
        let frame = theme::dialog_frame(ctx);
        let mut next = None;
        match dialog {
            Dialog::Edit { id, mut title, mut author, mut desc, mut contact, mut license, mut arguments, program } => {
                let modal = egui::Modal::new(egui::Id::new("edit")).frame(frame).show(ctx, |ui| {
                    ui.set_width(560.0);
                    ui.label(RichText::new("Edit").size(18.0).strong().color(p.text_strong));
                    ui.add_space(10.0);
                    egui::Grid::new("edit").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                        for (label, value, multiline) in [("Title", &mut title, false), ("Author", &mut author, false), ("Description", &mut desc, true), ("Website", &mut contact, false), ("License", &mut license, false)] {
                            ui.label(RichText::new(label).color(p.text_weak));
                            if multiline {
                                ui.add(egui::TextEdit::multiline(value).desired_rows(3).desired_width(420.0));
                            } else {
                                ui.add(egui::TextEdit::singleline(value).desired_width(420.0));
                            }
                            ui.end_row();
                        }
                        if program {
                            ui.label(RichText::new("Arguments").color(p.text_weak));
                            ui.add(egui::TextEdit::singleline(&mut arguments).desired_width(420.0).font(egui::TextStyle::Monospace));
                            ui.end_row();
                        }
                    });
                    ui.add_space(14.0);
                    let mut done = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(!title.trim().is_empty(), theme::primary("Save")).clicked() {
                            let patch = InfoPatch {
                                title: Some(title.clone()),
                                author: Some(author.clone()),
                                desc: Some(desc.clone()),
                                contact: Some(contact.clone()),
                                license: Some(license.clone()),
                                arguments: program.then(|| arguments.clone()),
                            };
                            self.send(Request::EditInfo { wallpaper: id.clone(), patch });
                            done = true;
                        }
                        if ui.add(theme::secondary_button("Cancel")).clicked() {
                            done = true;
                        }
                    });
                    done
                });
                if !modal.inner && !modal.should_close() {
                    next = Some(Dialog::Edit { id, title, author, desc, contact, license, arguments, program });
                }
            }
            Dialog::Delete { id, title } => {
                let modal = egui::Modal::new(egui::Id::new("delete")).frame(frame).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.label(RichText::new(format!("Remove “{title}”?")).size(18.0).strong().color(p.text_strong));
                    ui.add_space(14.0);
                    let mut done = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(theme::danger("Remove")).clicked() {
                            self.send(Request::Delete { wallpaper: id.clone() });
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
        }
        self.dialog = next;
    }

    // ---- actions --------------------------------------------------------------------------

    fn library_actions(&mut self, ctx: &egui::Context, actions: Vec<library::Action>) {
        for action in actions {
            match action {
                library::Action::Apply { id, display } => self.apply(&id, display),
                library::Action::Customize { id } => {
                    let running = self.status.as_ref().and_then(|s| s.active.iter().find(|a| a.wallpaper == id).map(|a| a.display.clone()));
                    match running {
                        Some(display) => self.selected_display = Some(display),
                        None => self.apply(&id, None),
                    }
                    self.page = Page::Screens;
                }
                library::Action::Edit { id } => {
                    if let Some(w) = self.library.iter().find(|w| w.id == id) {
                        self.dialog = Some(Dialog::Edit {
                            id: id.clone(),
                            title: w.title.clone(),
                            author: w.author.clone().unwrap_or_default(),
                            desc: w.desc.clone().unwrap_or_default(),
                            contact: w.contact.clone().unwrap_or_default(),
                            license: w.license.clone().unwrap_or_default(),
                            arguments: w.arguments.clone().unwrap_or_default(),
                            program: w.kind == Kind::Program,
                        });
                    }
                }
                library::Action::Export { id } => {
                    let title = self.library.iter().find(|w| w.id == id).map(|w| w.title.clone()).unwrap_or(id.clone());
                    if let Some(file) = rfd::FileDialog::new().add_filter("Lively package", &["zip"]).set_file_name(format!("{}.zip", crate::paths::slug(&title))).save_file() {
                        self.backend.background(ctx, format!("Exported {title}"), Request::Export { wallpaper: id, file });
                    }
                }
                library::Action::Reveal { path } => crate::ui::reveal(&path),
                library::Action::Thumbnail { id } => self.backend.background(ctx, "Thumbnail updated", Request::Thumbnail { wallpaper: id }),
                library::Action::Delete { id } => {
                    let title = self.library.iter().find(|w| w.id == id).map(|w| w.title.clone()).unwrap_or(id.clone());
                    self.dialog = Some(Dialog::Delete { id, title });
                }
                library::Action::Open { url } => crate::ui::open_url(&url),
                library::Action::PickFiles => self.pick_files(ctx),
                library::Action::PickFolders => self.pick_folders(ctx),
                library::Action::AddLink { url } => self.backend.background(ctx, "Added", Request::Import { source: url }),
                library::Action::SelectDisplay(id) => self.selected_display = Some(id),
                library::Action::Filter(kind) => self.filter = kind,
                library::Action::GoToScreens => self.page = Page::Screens,
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        ctx.input_mut(|i| {
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
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import_paths(ctx, dropped);
        }
        let p = theme::palette(root);
        let nav_frame = Frame::new().fill(p.sidebar).inner_margin(Margin { left: 12, right: 12, top: 18, bottom: 14 });
        egui::Panel::left("nav").exact_size(232.0).resizable(false).show_separator_line(false).frame(nav_frame).show(root, |ui| self.nav(ui));
        let content_frame = Frame::new().fill(p.bg).inner_margin(Margin { left: 28, right: 28, top: 22, bottom: 16 });
        egui::CentralPanel::default().frame(content_frame).show(root, |ui| {
            self.connection_banner(ui);
            match self.page {
                Page::Library => self.library_page(ctx, ui),
                Page::Screens => self.screens_page(ui),
                Page::Settings => self.settings_page(ctx, ui),
                Page::About => self.about_page(ui),
            }
        });
        self.dialogs(ctx);
        self.toasts.show(ctx);
    }
}
