use crate::ipc::{AudioDevice, Event, InfoPatch, Request, Response, Status};
use crate::model::settings::Theme;
use crate::model::{Arrangement, Kind, Settings, Summary};
use crate::paths::Paths;
use crate::ui::widgets::{Toasts, chip};
use crate::ui::{Backend, UiMsg, customize, library, screens, settings, theme};
use eframe::egui::{self, RichText};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Page {
    Library,
    Screens,
    Settings,
    About,
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
    filter: library::Filter,
    page: Page,
    customize: customize::Panel,
    dialog: Option<Dialog>,
    toasts: Toasts,
    volume: u8,
    last_poll: Instant,
    paths: Paths,
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
            filter: library::Filter::default(),
            page: Page::Library,
            customize: customize::Panel::default(),
            dialog: None,
            toasts: Toasts::default(),
            volume: 75,
            last_poll: Instant::now(),
            paths,
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
                UiMsg::Done { label, result } => match *result {
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
            let label = format!("Added {}", crate::paths::file_name(&p));
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

    // ---- chrome -----------------------------------------------------------------------------

    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(theme::logo().fit_to_exact_size(egui::vec2(34.0, 34.0)));
            ui.label(RichText::new("Deadly Wallpaper").size(16.0).strong());
        });
        ui.add_space(18.0);
        for (n, page, glyph, label) in [(1, Page::Library, "🖼", "Library"), (2, Page::Screens, "🖥", "Screens"), (3, Page::Settings, "⚙", "Settings"), (4, Page::About, "ℹ", "About")] {
            if nav_item(ui, self.page == page, glyph, label).on_hover_text(format!("Ctrl+{n}")).clicked() {
                self.page = page;
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add_space(4.0);
            let (connected, paused, playing) = match &self.status {
                Some(s) => (true, s.paused, s.active.len()),
                None => (false, false, 0),
            };
            ui.horizontal(|ui| {
                ui.label("🔊");
                let slider = ui.add(egui::Slider::new(&mut self.volume, 0..=100).show_value(false).trailing_fill(true));
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    self.send(Request::Volume { value: self.volume.to_string() });
                }
            });
            if connected {
                let label = if paused { "▶  Resume wallpapers" } else { "⏸  Pause wallpapers" };
                if ui.add_sized([ui.available_width(), 32.0], egui::Button::new(label)).clicked() {
                    self.send(Request::Play { play: paused });
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let (dot, text) = if !connected {
                    (ui.visuals().warn_fg_color, "Connecting to the daemon…".to_string())
                } else if paused {
                    (ui.visuals().warn_fg_color, "Paused".to_string())
                } else {
                    (egui::Color32::from_rgb(76, 175, 110), match playing {
                        0 => "Idle".to_string(),
                        1 => "1 wallpaper playing".to_string(),
                        n => format!("{n} wallpapers playing"),
                    })
                };
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, dot);
                ui.label(RichText::new(text).small().color(ui.visuals().weak_text_color()));
            });
        });
    }

    fn library_page(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::page_title(ui, "Library");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(RichText::new("＋  Add").strong(), |ui| {
                    ui.set_min_width(220.0);
                    if ui.button("Video, picture, GIF or package…").clicked() {
                        ui.close();
                        self.pick_files(ctx);
                    }
                    if ui.button("Web page folder…").clicked() {
                        ui.close();
                        self.pick_folder(ctx);
                    }
                    if ui.button("Link to a website or video stream…").clicked() {
                        ui.close();
                        self.dialog = Some(Dialog::AddUrl { url: String::new() });
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("🔍 Search").desired_width(220.0));
            });
        });
        ui.add_space(6.0);
        let (displays, active): (Vec<_>, Vec<_>) = match &self.status {
            Some(s) => (s.displays.clone(), s.active.clone()),
            None => (Vec::new(), Vec::new()),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Apply to").color(ui.visuals().weak_text_color()));
            match self.arrangement() {
                Arrangement::Per => {
                    for (i, d) in displays.iter().enumerate() {
                        let selected = self.selected_display.as_deref() == Some(d.id.as_str());
                        if chip(ui, selected, format!("{}  {}", i + 1, d.name)).on_hover_text(format!("{}×{}", d.rect.w, d.rect.h)).clicked() {
                            self.selected_display = Some(d.id.clone());
                        }
                    }
                }
                Arrangement::Span => {
                    chip(ui, true, "Every display, spanning");
                }
                Arrangement::Duplicate => {
                    chip(ui, true, "Every display");
                }
            }
            ui.add_space(12.0);
            ui.separator();
            if let Some(kind) = library::filter_bar(ui, &self.filter, &self.library) {
                self.filter.kind = kind;
            }
        });
        ui.add_space(8.0);
        let grid = library::Grid {
            items: &self.library,
            search: &self.search,
            filter: &self.filter,
            displays: &displays,
            active: &active,
            arrangement: self.arrangement(),
            selected_display: self.selected_display.as_deref(),
            hovering_files: ctx.input(|i| !i.raw.hovered_files.is_empty()),
            connected: self.backend.connected(),
        };
        let actions = library::grid(ui, &grid);
        self.library_actions(ctx, actions);
    }

    fn screens_page(&mut self, ui: &mut egui::Ui) {
        let Some(status) = self.status.clone() else {
            theme::page_title(ui, "Screens");
            ui.label(RichText::new("Connecting to the wallpaper daemon…").color(ui.visuals().weak_text_color()));
            return;
        };
        let view = screens::View { status: &status, library: &self.library, selected: self.selected_display.as_deref() };
        let actions = egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            let actions = screens::page(ui, &view);
            let selected = self.selected_display.clone().or_else(|| crate::model::display::primary(&status.displays).map(|d| d.id.clone()));
            let active = status.active.iter().find(|a| Some(&a.display) == selected.as_ref() || status.layout.arrangement != Arrangement::Per).cloned();
            if let Some(a) = active.filter(|a| a.customizable) {
                ui.add_space(12.0);
                theme::section_title(ui, "Customize");
                ui.add_space(4.0);
                theme::card(ui).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let root = self.library.iter().find(|w| w.id == a.wallpaper).map(|w| if w.absolute { PathBuf::from(&w.source).parent().map(|p| p.to_path_buf()).unwrap_or(w.dir.clone()) } else { w.dir.clone() });
                    match self.customize.ensure(&mut self.backend, &a.wallpaper, selected.as_deref(), root.unwrap_or_default()) {
                        Err(e) => {
                            ui.label(RichText::new(e.to_string()).color(ui.visuals().error_fg_color));
                        }
                        Ok(()) if self.customize.is_empty() => theme::hint(ui, "This wallpaper has no adjustable properties."),
                        Ok(()) => self.customize.ui(ui, &mut self.backend, &mut self.toasts),
                    }
                });
            }
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

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        theme::page_title(ui, "Settings");
        ui.add_space(8.0);
        let Some(mut s) = self.settings.clone() else {
            ui.label(RichText::new("Connecting to the wallpaper daemon…").color(ui.visuals().weak_text_color()));
            return;
        };
        let (displays, capabilities) = match &self.status {
            Some(st) => (st.displays.clone(), st.capabilities.clone()),
            None => (Vec::new(), Default::default()),
        };
        egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
            ui.set_max_width(760.0);
            let cx = settings::Context { devices: &self.devices, displays: &displays, capabilities: &capabilities };
            if settings::ui(ui, &mut s, &cx) {
                self.settings = Some(s.clone());
                self.send(Request::SetSettings { settings: s });
            }
        });
    }

    fn about_page(&mut self, ui: &mut egui::Ui) {
        theme::page_title(ui, "About");
        ui.add_space(8.0);
        theme::card(ui).show(ui, |ui| {
            ui.set_min_width(ui.available_width().min(760.0));
            ui.horizontal(|ui| {
                ui.add(theme::logo().fit_to_exact_size(egui::vec2(72.0, 72.0)));
                ui.vertical(|ui| {
                    ui.label(RichText::new("Deadly Wallpaper").size(20.0).strong());
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                    theme::hint(ui, "Live wallpapers for Linux, macOS and Windows. Plays Lively Wallpaper packages.");
                });
            });
            ui.add_space(10.0);
            if let Some(s) = &self.status {
                egui::Grid::new("about").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("Presenter").color(ui.visuals().weak_text_color()));
                    ui.label(presenter_label(&s.capabilities.presenter));
                    ui.end_row();
                    ui.label(RichText::new("Session").color(ui.visuals().weak_text_color()));
                    ui.label(format!("{} on {}", s.session, s.platform));
                    ui.end_row();
                    ui.label(RichText::new("Window monitor").color(ui.visuals().weak_text_color()));
                    ui.label(if s.window_monitor == "none" { "none: playback rules that depend on window positions stay inactive".to_string() } else { s.window_monitor.clone() });
                    ui.end_row();
                });
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Open library folder").clicked() {
                    if let Some(s) = &self.settings {
                        crate::ui::reveal(&s.library_dir);
                    }
                }
                if ui.button("Open log file").clicked() {
                    crate::ui::reveal(&self.paths.log_file());
                }
                ui.hyperlink_to("Source code", "https://github.com/nickheyer/deadlywallpaper");
            });
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.dialog.take() else { return };
        let mut next = None;
        match dialog {
            Dialog::AddUrl { mut url } => {
                let modal = egui::Modal::new(egui::Id::new("add-url")).show(ctx, |ui| {
                    ui.set_width(460.0);
                    ui.label(RichText::new("Add a link").size(17.0).strong());
                    theme::hint(ui, "Web pages become website wallpapers. Video links that yt-dlp understands play as streams.");
                    ui.add_space(6.0);
                    let edit = ui.add(egui::TextEdit::singleline(&mut url).hint_text("https://").desired_width(f32::INFINITY));
                    edit.request_focus();
                    let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.add_space(8.0);
                    let mut done = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("Add").fill(theme::accent(ui))).clicked() || submit {
                            let u = url.trim().to_string();
                            if !u.is_empty() {
                                self.backend.background(ctx, "Added", Request::Import { source: u });
                            }
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
                            done = true;
                        }
                    });
                    done
                });
                if !modal.inner && !modal.should_close() {
                    next = Some(Dialog::AddUrl { url });
                }
            }
            Dialog::Edit { id, mut title, mut author, mut desc, mut contact, mut license, mut arguments, program } => {
                let modal = egui::Modal::new(egui::Id::new("edit")).show(ctx, |ui| {
                    ui.set_width(520.0);
                    ui.label(RichText::new("Edit details").size(17.0).strong());
                    ui.add_space(6.0);
                    egui::Grid::new("edit").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                        for (label, value, multiline) in [("Title", &mut title, false), ("Author", &mut author, false), ("Description", &mut desc, true), ("Website", &mut contact, false), ("License", &mut license, false)] {
                            ui.label(label);
                            if multiline {
                                ui.add(egui::TextEdit::multiline(value).desired_rows(3).desired_width(380.0));
                            } else {
                                ui.add(egui::TextEdit::singleline(value).desired_width(380.0));
                            }
                            ui.end_row();
                        }
                        if program {
                            ui.label("Arguments");
                            ui.add(egui::TextEdit::singleline(&mut arguments).desired_width(380.0));
                            ui.end_row();
                        }
                    });
                    ui.add_space(8.0);
                    let mut done = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("Save").fill(theme::accent(ui))).clicked() {
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
                        if ui.button("Cancel").clicked() {
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
                let modal = egui::Modal::new(egui::Id::new("delete")).show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.label(RichText::new(format!("Remove “{title}”?")).size(17.0).strong());
                    theme::hint(ui, "The wallpaper leaves the library. Files that live outside the library folder stay where they are.");
                    ui.add_space(8.0);
                    let mut done = false;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new("Remove").fill(ui.visuals().error_fg_color)).clicked() {
                            self.send(Request::Delete { wallpaper: id.clone() });
                            done = true;
                        }
                        if ui.button("Cancel").clicked() {
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

    fn library_actions(&mut self, ctx: &egui::Context, actions: Vec<library::Action>) {
        for action in actions {
            match action {
                library::Action::Apply { id, display } => self.apply(&id, display),
                library::Action::Customize { id } => {
                    match self.status.as_ref().and_then(|s| s.active.iter().find(|a| a.wallpaper == id)) {
                        Some(a) => self.selected_display = Some(a.display.clone()),
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
                library::Action::PickFiles => self.pick_files(ctx),
            }
        }
    }
}

fn presenter_label(presenter: &str) -> String {
    match presenter {
        "plasma" => "KDE Plasma wallpaper plugin: wallpapers play beneath the desktop icons and widgets".into(),
        "layer-shell" => "Wayland layer shell: background surfaces behind every window".into(),
        "x11" => "X11: keep-below windows behind every application window".into(),
        "win32" => "Windows: behind the desktop icons under Explorer's WorkerW".into(),
        "quartz" => "macOS: desktop-level windows below the Finder icons".into(),
        other => other.to_string(),
    }
}

fn nav_item(ui: &mut egui::Ui, selected: bool, glyph: &str, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 36.0), egui::Sense::click());
    let visuals = ui.visuals();
    if selected {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), theme::accent(ui).gamma_multiply(0.35));
    } else if response.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(8), visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if selected { visuals.strong_text_color() } else { visuals.text_color() };
    ui.painter().text(rect.left_center() + egui::vec2(12.0, 0.0), egui::Align2::LEFT_CENTER, glyph, egui::FontId::proportional(17.0), color);
    ui.painter().text(rect.left_center() + egui::vec2(42.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), color);
    response
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        self.handle_messages(ctx);
        ctx.input(|i| {
            for (n, page) in [(egui::Key::Num1, Page::Library), (egui::Key::Num2, Page::Screens), (egui::Key::Num3, Page::Settings), (egui::Key::Num4, Page::About)] {
                if i.modifiers.command && i.key_pressed(n) {
                    self.page = page;
                }
            }
        });
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import_paths(ctx, dropped);
        }
        let (panel_fill, window_fill) = (root.visuals().panel_fill, root.visuals().window_fill);
        let nav_frame = egui::Frame::new().fill(panel_fill).inner_margin(egui::Margin { left: 12, right: 12, top: 16, bottom: 12 });
        egui::Panel::left("nav").exact_size(210.0).resizable(false).frame(nav_frame).show(root, |ui| self.nav(ui));
        let content_frame = egui::Frame::new().fill(window_fill).inner_margin(egui::Margin { left: 24, right: 24, top: 18, bottom: 16 });
        egui::CentralPanel::default().frame(content_frame).show(root, |ui| match self.page {
            Page::Library => self.library_page(ctx, ui),
            Page::Screens => self.screens_page(ui),
            Page::Settings => self.settings_page(ui),
            Page::About => self.about_page(ui),
        });
        self.dialogs(ctx);
        self.toasts.show(ctx);
    }
}
