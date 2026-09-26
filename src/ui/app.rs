use eframe::egui;
use crate::ipc::{AudioDevice, Event, InfoPatch, Request, Response, Status};
use crate::model::settings::Theme;
use crate::model::{Arrangement, Kind, Settings, Summary};
use crate::paths::Paths;
use crate::ui::widgets::{Toasts, display_strip};
use crate::ui::{Backend, UiMsg, customize, library, settings};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Panel {
    Wallpaper,
    Settings,
}

pub enum Dialog {
    AddUrl { url: String },
    Edit { id: String, title: String, author: String, desc: String, contact: String, license: String, arguments: String, program: bool },
    Delete { id: String, title: String },
}

pub struct App {
    backend: Backend,
    status: Option<Status>,
    library: Vec<Summary>,
    settings: Option<Settings>,
    devices: Vec<AudioDevice>,
    selected_display: Option<String>,
    search: String,
    panel: Panel,
    panel_open: bool,
    customize: customize::Panel,
    dialog: Option<Dialog>,
    toasts: Toasts,
    volume: u8,
    last_poll: Instant,
    _paths: Paths,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> App {
        let mut app = App {
            backend: Backend::connect(cc.egui_ctx.clone()),
            status: None,
            library: Vec::new(),
            settings: None,
            devices: Vec::new(),
            selected_display: None,
            search: String::new(),
            panel: Panel::Wallpaper,
            panel_open: true,
            customize: customize::Panel::default(),
            dialog: None,
            toasts: Toasts::default(),
            volume: 75,
            last_poll: Instant::now(),
            _paths: paths,
        };
        app.refresh_all(&cc.egui_ctx);
        app
    }

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
                ctx.forget_all_images();
                self.library = items;
            }
            Ok(_) => {}
            Err(e) => self.toasts.error(e.to_string()),
        }
    }

    fn refresh_settings(&mut self, ctx: &egui::Context) {
        match self.backend.call(Request::Settings) {
            Ok(Response::Settings(s)) => {
                ctx.set_theme(match s.theme {
                    Theme::System => egui::ThemePreference::System,
                    Theme::Light => egui::ThemePreference::Light,
                    Theme::Dark => egui::ThemePreference::Dark,
                });
                self.volume = s.volume;
                self.settings = Some(s);
            }
            Ok(_) => {}
            Err(e) => self.toasts.error(e.to_string()),
        }
        if let Ok(Response::Devices(d)) = self.backend.call(Request::AudioDevices) {
            self.devices = d;
        }
    }

    fn handle_messages(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.backend.rx.try_recv() {
            match msg {
                UiMsg::Event(Event::Library) => self.refresh_library(ctx),
                UiMsg::Event(Event::Layout | Event::Displays | Event::Playback) => {
                    self.refresh_status();
                    self.customize.invalidate_if_gone(self.status.as_ref());
                }
                UiMsg::Event(Event::Settings) => self.refresh_settings(ctx),
                UiMsg::Event(Event::Error { message }) => self.toasts.error(message),
                UiMsg::Event(Event::Info { message }) => self.toasts.info(message),
                UiMsg::Done { label, result } => match result {
                    Ok(Response::Wallpaper(w)) => self.toasts.info(format!("{label}: {}", w.title)),
                    Ok(_) => self.toasts.info(label),
                    Err(e) => self.toasts.error(format!("{label} failed: {e}")),
                },
                UiMsg::Disconnected => {
                    self.backend.drop_connection();
                    self.status = None;
                }
            }
        }
        if !self.backend.connected() && self.last_poll.elapsed() > Duration::from_secs(2) {
            self.last_poll = Instant::now();
            self.backend.reconnect(ctx);
            if self.backend.connected() {
                self.refresh_all(ctx);
            }
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
            let label = format!("Imported {}", crate::paths::file_name(&p));
            self.backend.background(ctx, label, Request::Import { source: p.to_string_lossy().into_owned() });
        }
    }

    fn pick_files(&mut self, ctx: &egui::Context) {
        let media = ["mp4", "mkv", "webm", "mov", "avi", "m4v", "mpg", "mpeg", "wmv", "flv", "ogv", "gif", "jpg", "jpeg", "png", "bmp", "webp", "tif", "tiff", "avif"];
        let picked = rfd::FileDialog::new()
            .add_filter("Wallpapers", &[&media[..], &["html", "htm", "zip", "exe", "appimage"]].concat())
            .add_filter("Media", &media)
            .add_filter("Lively packages", &["zip"])
            .add_filter("Web pages", &["html", "htm"])
            .add_filter("All files", &["*"])
            .pick_files();
        if let Some(files) = picked {
            self.import_paths(ctx, files);
        }
    }

    fn pick_folder(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
            self.import_paths(ctx, vec![dir]);
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Deadly Wallpaper").strong().size(17.0));
            ui.add_space(12.0);
            let (displays, active): (Vec<_>, Vec<String>) = match &self.status {
                Some(s) => (s.displays.clone(), s.active.iter().map(|a| a.display.clone()).collect()),
                None => (Vec::new(), Vec::new()),
            };
            let all = self.arrangement() != Arrangement::Per;
            if let Some(id) = display_strip(ui, &displays, self.selected_display.as_deref(), all, &active) {
                self.selected_display = Some(id);
            }
            let mut arrangement = self.arrangement();
            egui::ComboBox::from_id_salt("arrangement").selected_text(arrangement.label()).width(120.0).show_ui(ui, |ui| {
                for a in [Arrangement::Per, Arrangement::Span, Arrangement::Duplicate] {
                    ui.selectable_value(&mut arrangement, a, a.label());
                }
            });
            if arrangement != self.arrangement() {
                let display = self.selected_display.clone();
                self.send(Request::SetArrangement { arrangement, display });
            }
            ui.add_space(8.0);
            let paused = self.status.as_ref().is_some_and(|s| s.paused);
            if ui.button(if paused { "▶ Resume" } else { "⏸ Pause" }).on_hover_text("Pause every wallpaper").clicked() {
                self.send(Request::Play { play: paused });
            }
            ui.add_space(4.0);
            ui.label("🔊");
            let slider = ui.add(egui::Slider::new(&mut self.volume, 0..=100).show_value(false));
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.send(Request::Volume { value: self.volume.to_string() });
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if self.panel_open { "Hide panel" } else { "Show panel" };
                if ui.button("⚙").on_hover_text("Settings").clicked() {
                    self.panel = Panel::Settings;
                    self.panel_open = !(self.panel_open && self.panel == Panel::Settings) || self.panel != Panel::Settings;
                }
                if ui.button("🎨").on_hover_text(label).clicked() {
                    self.panel_open = !(self.panel_open && self.panel == Panel::Wallpaper);
                    self.panel = Panel::Wallpaper;
                }
                ui.menu_button("＋ Add", |ui| {
                    if ui.button("Files…").clicked() {
                        ui.close();
                        self.pick_files(ctx);
                    }
                    if ui.button("Folder (web project)…").clicked() {
                        ui.close();
                        self.pick_folder(ctx);
                    }
                    if ui.button("URL or video stream…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::AddUrl { url: String::new() });
                    }
                    if ui.button("Random wallpaper").clicked() {
                        ui.close();
                        let display = self.selected_display.clone();
                        self.send(Request::Set { target: "random".into(), display });
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search").desired_width(180.0));
            });
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        let mut open = self.panel_open;
        egui::Panel::right("side").resizable(true).default_size(360.0).min_size(280.0).show_collapsible(ui, &mut open, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.panel, Panel::Wallpaper, "Wallpaper");
                ui.selectable_value(&mut self.panel, Panel::Settings, "Settings");
            });
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match self.panel {
                Panel::Wallpaper => self.wallpaper_panel(ui),
                Panel::Settings => self.settings_panel(ui),
            });
        });
        self.panel_open = open;
    }

    fn wallpaper_panel(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            ui.weak("Daemon not connected");
            return;
        };
        let selected = self.selected_display.clone();
        let active = status.active.iter().find(|a| Some(&a.display) == selected.as_ref() || self.arrangement() != Arrangement::Per).cloned();
        match active {
            None => {
                ui.add_space(8.0);
                ui.weak("No wallpaper on this display. Click a tile to apply one.");
            }
            Some(a) => {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(&a.title).strong().size(16.0));
                ui.weak(format!("{}{}", a.kind.label(), if a.paused { " · paused" } else { "" }));
                ui.horizontal(|ui| {
                    if ui.button("Reload").clicked() {
                        self.send(Request::Set { target: "reload".into(), display: selected.clone() });
                    }
                    if ui.button("Close").clicked() {
                        self.send(Request::Close { display: if self.arrangement() == Arrangement::Per { selected.clone() } else { None } });
                    }
                    if a.kind.is_media() && ui.button("Restart playback").clicked() {
                        self.send(Request::Seek { display: selected.clone(), value: "0".into() });
                    }
                });
                ui.separator();
                if a.customizable {
                    let root = self.library.iter().find(|w| w.id == a.wallpaper).map(|w| {
                        if w.absolute { PathBuf::from(&w.source).parent().map(|p| p.to_path_buf()).unwrap_or(w.dir.clone()) } else { w.dir.clone() }
                    });
                    if let Err(e) = self.customize.ensure(&mut self.backend, &a.wallpaper, selected.as_deref(), root.unwrap_or_default()) {
                        ui.weak(e.to_string());
                    } else {
                        self.customize.ui(ui, &mut self.backend, &mut self.toasts);
                    }
                } else {
                    ui.weak("This wallpaper has no customization controls.");
                }
            }
        }
    }

    fn settings_panel(&mut self, ui: &mut egui::Ui) {
        let Some(mut s) = self.settings.clone() else {
            ui.weak("Daemon not connected");
            return;
        };
        let displays = self.status.as_ref().map(|st| st.displays.clone()).unwrap_or_default();
        if settings::ui(ui, &mut s, &self.devices, &displays) {
            self.settings = Some(s.clone());
            self.send(Request::SetSettings { settings: s });
        }
        ui.add_space(12.0);
        ui.separator();
        if let Some(st) = &self.status {
            ui.weak(format!("deadlywp {} · {} · window monitor: {}", st.version, st.session, st.window_monitor));
            if st.window_monitor == "none" {
                ui.colored_label(egui::Color32::from_rgb(220, 160, 60), "This desktop exposes no window information; fullscreen and focus rules cannot pause wallpapers here.");
            }
            if st.window_monitor == "kwin" && st.session == "wayland" {
                ui.weak("On KDE Plasma the wallpaper layer sits above the Plasma desktop, so desktop icons and widgets are hidden while a wallpaper runs.");
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let mut next = None;
        match dialog {
            Dialog::AddUrl { mut url } => {
                egui::Window::new("Add web page or video stream").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label("Web pages become HTML wallpapers; video links that yt-dlp understands play as streams.");
                    let edit = ui.add(egui::TextEdit::singleline(&mut url).hint_text("https://").desired_width(420.0));
                    edit.request_focus();
                    let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() || submit {
                            let u = url.trim().to_string();
                            if !u.is_empty() {
                                self.backend.background(ctx, "Added", Request::Import { source: u });
                            }
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if keep {
                    next = Some(Dialog::AddUrl { url });
                }
            }
            Dialog::Edit { id, mut title, mut author, mut desc, mut contact, mut license, mut arguments, program } => {
                egui::Window::new("Edit wallpaper").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    egui::Grid::new("edit").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                        ui.label("Title");
                        ui.add(egui::TextEdit::singleline(&mut title).desired_width(360.0));
                        ui.end_row();
                        ui.label("Author");
                        ui.add(egui::TextEdit::singleline(&mut author).desired_width(360.0));
                        ui.end_row();
                        ui.label("Description");
                        ui.add(egui::TextEdit::multiline(&mut desc).desired_rows(3).desired_width(360.0));
                        ui.end_row();
                        ui.label("Website");
                        ui.add(egui::TextEdit::singleline(&mut contact).desired_width(360.0));
                        ui.end_row();
                        ui.label("License");
                        ui.add(egui::TextEdit::singleline(&mut license).desired_width(360.0));
                        ui.end_row();
                        if program {
                            ui.label("Arguments");
                            ui.add(egui::TextEdit::singleline(&mut arguments).desired_width(360.0));
                            ui.end_row();
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let patch = InfoPatch {
                                title: Some(title.clone()),
                                author: Some(author.clone()),
                                desc: Some(desc.clone()),
                                contact: Some(contact.clone()),
                                license: Some(license.clone()),
                                arguments: program.then(|| arguments.clone()),
                            };
                            self.send(Request::EditInfo { wallpaper: id.clone(), patch });
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if keep {
                    next = Some(Dialog::Edit { id, title, author, desc, contact, license, arguments, program });
                }
            }
            Dialog::Delete { id, title } => {
                egui::Window::new("Delete wallpaper").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                    ui.label(format!("Remove '{title}' from the library? Files outside the library stay untouched."));
                    ui.horizontal(|ui| {
                        if ui.button("Delete").clicked() {
                            self.send(Request::Delete { wallpaper: id.clone() });
                            keep = false;
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
                if keep {
                    next = Some(Dialog::Delete { id, title });
                }
            }
        }
        self.dialog = next;
    }

    fn library_actions(&mut self, ctx: &egui::Context, actions: Vec<library::Action>) {
        for action in actions {
            match action {
                library::Action::Apply { id, display } => self.apply(&id, display),
                library::Action::Customize { id } => {
                    if self.status.as_ref().is_some_and(|s| s.active.iter().any(|a| a.wallpaper == id)) {
                        if let Some(a) = self.status.as_ref().and_then(|s| s.active.iter().find(|a| a.wallpaper == id)) {
                            self.selected_display = Some(a.display.clone());
                        }
                    } else {
                        self.apply(&id, None);
                    }
                    self.panel = Panel::Wallpaper;
                    self.panel_open = true;
                }
                library::Action::Edit { id } => {
                    if let Some(w) = self.library.iter().find(|w| w.id == id) {
                        self.dialog = Some(Dialog::Edit {
                            id: id.clone(),
                            title: w.title.clone(),
                            author: w.author.clone().unwrap_or_default(),
                            desc: w.desc.clone().unwrap_or_default(),
                            contact: w.contact.clone().unwrap_or_default(),
                            license: String::new(),
                            arguments: String::new(),
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
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.handle_messages(ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import_paths(ctx, dropped);
        }
        egui::Panel::top("top").show(root, |ui| {
            ui.add_space(4.0);
            self.top_bar(ctx, ui);
            ui.add_space(4.0);
        });
        self.side_panel(root);
        let actions = egui::CentralPanel::default()
            .show(root, |ui| {
                if !self.backend.connected() {
                    ui.centered_and_justified(|ui| ui.label("Connecting to the wallpaper daemon…"));
                    return Vec::new();
                }
                let status = self.status.as_ref();
                let grid = library::Grid {
                    items: &self.library,
                    search: &self.search,
                    displays: status.map(|s| s.displays.as_slice()).unwrap_or(&[]),
                    active: status.map(|s| s.active.as_slice()).unwrap_or(&[]),
                    arrangement: self.arrangement(),
                    selected_display: self.selected_display.as_deref(),
                    hovering_files: ctx.input(|i| !i.raw.hovered_files.is_empty()),
                };
                library::grid(ui, &grid)
            })
            .inner;
        self.library_actions(ctx, actions);
        self.dialogs(ctx);
        self.toasts.show(ctx);
    }
}
