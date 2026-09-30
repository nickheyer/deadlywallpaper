//! Live wallpaper property editor.

use crate::error::Result;
use crate::ipc::{Request, Response, Status};
use crate::model::Control;
use crate::model::props::ControlKind;
use crate::ui::widgets::Toasts;
use crate::ui::{Backend, theme, widgets};
use eframe::egui::{self, RichText};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Panel {
    key: Option<(String, Option<String>)>,
    controls: Vec<(String, Control)>,
    root: PathBuf,
    folders: HashMap<String, Vec<String>>,
    last_send: Option<Instant>,
}

impl Panel {
    /// Load controls for `wallpaper` on `display` unless already shown.
    pub fn ensure(
        &mut self,
        backend: &mut Backend,
        wallpaper: &str,
        display: Option<&str>,
        root: PathBuf,
    ) -> Result<()> {
        let key = (wallpaper.to_string(), display.map(str::to_owned));
        if self.key.as_ref() == Some(&key) {
            return Ok(());
        }
        match backend.call(Request::Properties {
            wallpaper: wallpaper.into(),
            display: display.map(str::to_owned),
        })? {
            Response::Controls { controls, .. } => {
                self.controls = controls;
                self.key = Some(key);
                self.root = root;
                self.folders.clear();
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Drop the loaded controls when their wallpaper stopped running.
    pub fn invalidate_if_gone(&mut self, status: Option<&Status>) {
        if let (Some((wallpaper, _)), Some(s)) = (&self.key, status) {
            if !s.active.iter().any(|a| &a.wallpaper == wallpaper) {
                self.clear();
            }
        }
    }

    pub fn clear(&mut self) {
        self.key = None;
        self.controls.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.controls.is_empty()
    }

    fn send(&mut self, backend: &mut Backend, toasts: &mut Toasts, name: &str, value: Value) {
        let Some((wallpaper, display)) = self.key.clone() else {
            return;
        };
        if let Err(e) = backend.call(Request::SetProperty {
            wallpaper,
            display,
            name: name.into(),
            value,
        }) {
            toasts.error(e.to_string());
        }
        self.last_send = Some(Instant::now());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, backend: &mut Backend, toasts: &mut Toasts) {
        let p = theme::palette(ui);
        let mut pending: Vec<(String, Value)> = Vec::new();
        let mut reset = false;
        ui.horizontal(|ui| {
            theme::section_title(ui, "Adjustments");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                reset = ui
                    .add(theme::secondary_button("Reset"))
                    .on_hover_text("Reset wallpaper adjustments")
                    .clicked();
            });
        });
        ui.add_space(6.0);
        let throttle_ok = self
            .last_send
            .is_none_or(|t| t.elapsed() > Duration::from_millis(60));
        let label_w = (ui.available_width() * 0.3).min(160.0);
        let control_w = ui.available_width() - label_w - 12.0;
        let root = self.root.clone();
        let mut folder_cache = std::mem::take(&mut self.folders);
        let hidden = hidden_controls(&self.controls);
        egui::Grid::new("props")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .min_col_width(label_w)
            .max_col_width(control_w)
            .show(ui, |ui| {
                ui.spacing_mut().slider_width = control_w - 64.0;
                for (i, (name, control)) in self.controls.iter_mut().enumerate() {
                    if hidden.contains(name) {
                        continue;
                    }
                    let label = if control.text.is_empty() {
                        name.clone()
                    } else {
                        control.text.clone()
                    };
                    match &mut control.kind {
                        ControlKind::Label { value } => {
                            ui.label(RichText::new(value.as_str()).strong().color(p.text_strong));
                            ui.label("");
                            ui.end_row();
                            continue;
                        }
                        ControlKind::Button { value } => {
                            ui.label(RichText::new(&label).color(p.text));
                            if ui
                                .add(theme::secondary_button(if value.is_empty() {
                                    "Run"
                                } else {
                                    value.as_str()
                                }))
                                .clicked()
                            {
                                pending.push((name.clone(), Value::Null));
                            }
                            ui.end_row();
                            continue;
                        }
                        _ => {}
                    }
                    let l = ui
                        .allocate_ui_with_layout(
                            egui::vec2(label_w, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.add(egui::Label::new(RichText::new(&label).color(p.text)).wrap())
                            },
                        )
                        .inner;
                    if let Some(h) = control.help.as_deref().filter(|h| !h.trim().is_empty()) {
                        l.on_hover_text(h);
                    }
                    ui.horizontal(|ui| {
                        ui.set_width(control_w);
                        match &mut control.kind {
                            ControlKind::Slider {
                                value,
                                min,
                                max,
                                step,
                            } => {
                                let (lo, hi) = (min.min(*max), max.max(*min));
                                let decimals = if *step >= 1.0 {
                                    0
                                } else if *step >= 0.1 {
                                    1
                                } else {
                                    2
                                };
                                let r = ui.add(
                                    egui::Slider::new(value, lo..=hi)
                                        .step_by(*step)
                                        .show_value(true)
                                        .fixed_decimals(decimals),
                                );
                                if r.drag_stopped()
                                    || (r.changed() && (!r.dragged() || throttle_ok))
                                {
                                    pending.push((name.clone(), Value::from(*value)));
                                }
                            }
                            ControlKind::Checkbox { value } => {
                                if widgets::toggle(ui, value).changed() {
                                    pending.push((name.clone(), Value::Bool(*value)));
                                }
                            }
                            ControlKind::Dropdown { value, items }
                            | ControlKind::ScalerDropdown { value, items } => {
                                let current =
                                    items.get(*value as usize).cloned().unwrap_or_default();
                                egui::ComboBox::from_id_salt(("dd", i))
                                    .width(control_w.min(260.0))
                                    .selected_text(current)
                                    .show_ui(ui, |ui| {
                                        for (idx, item) in items.iter().enumerate() {
                                            if ui
                                                .selectable_value(value, idx as i64, item)
                                                .clicked()
                                            {
                                                pending
                                                    .push((name.clone(), Value::from(idx as i64)));
                                            }
                                        }
                                    });
                            }
                            ControlKind::Textbox { value } => {
                                let r = ui.add(
                                    egui::TextEdit::singleline(value)
                                        .desired_width(control_w.min(320.0)),
                                );
                                if r.lost_focus() && r.changed()
                                    || (r.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                                {
                                    pending.push((name.clone(), Value::String(value.clone())));
                                }
                            }
                            ControlKind::Color { value } => {
                                let mut rgb = parse_hex(value);
                                if ui.color_edit_button_srgb(&mut rgb).changed() {
                                    *value = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                                    pending.push((name.clone(), Value::String(value.clone())));
                                }
                                ui.label(
                                    RichText::new(value.as_str()).monospace().color(p.text_weak),
                                );
                            }
                            ControlKind::FolderDropdown {
                                value,
                                folder,
                                filter,
                            } => {
                                let (folder, filter) = (folder.clone(), filter.clone());
                                let current = value.clone().unwrap_or_else(|| "(none)".into());
                                let mut chosen: Option<Option<String>> = None;
                                let names =
                                    folder_files(&folder, &filter, &root, &mut folder_cache);
                                egui::ComboBox::from_id_salt(("fd", i))
                                    .width((control_w - 40.0).min(260.0))
                                    .selected_text(current)
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_label(value.is_none(), "(none)").clicked()
                                        {
                                            chosen = Some(None);
                                        }
                                        for n in &names {
                                            if ui
                                                .selectable_label(value.as_deref() == Some(n), n)
                                                .clicked()
                                            {
                                                chosen = Some(Some(n.clone()));
                                            }
                                        }
                                    });
                                if widgets::icon_button(ui, "+", "Add file").clicked() {
                                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                                        let dir = root.join(&folder);
                                        let mut last = None;
                                        match std::fs::create_dir_all(&dir) {
                                            Ok(()) => {
                                                for f in files {
                                                    let file_name = crate::paths::file_name(&f);
                                                    match std::fs::copy(&f, dir.join(&file_name)) {
                                                        Ok(_) => last = Some(file_name),
                                                        Err(e) => toasts.error(format!(
                                                            "copy {}: {e}",
                                                            f.display()
                                                        )),
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                toasts.error(format!("{}: {e}", dir.display()))
                                            }
                                        }
                                        folder_cache.remove(&folder);
                                        if let Some(n) = last {
                                            chosen = Some(Some(n));
                                        }
                                    }
                                }
                                if let Some(c) = chosen {
                                    *value = c.clone();
                                    pending.push((
                                        name.clone(),
                                        c.map(Value::String).unwrap_or(Value::Null),
                                    ));
                                }
                            }
                            ControlKind::File { value, filter } => {
                                let shown = if value.is_empty() {
                                    "(none)".to_string()
                                } else {
                                    crate::paths::file_name(std::path::Path::new(value.as_str()))
                                };
                                let mut chosen: Option<String> = None;
                                if ui
                                    .add(theme::secondary_button("Choose…"))
                                    .on_hover_text(value.as_str())
                                    .clicked()
                                {
                                    let mut dialog = rfd::FileDialog::new();
                                    if !filter.is_empty() {
                                        let exts: Vec<&str> =
                                            filter.iter().map(String::as_str).collect();
                                        dialog = dialog.add_filter("Supported files", &exts);
                                    }
                                    if let Some(f) = dialog.pick_file() {
                                        chosen = Some(f.to_string_lossy().into_owned());
                                    }
                                }
                                if !value.is_empty()
                                    && widgets::icon_button(ui, "✖", "Clear").clicked()
                                {
                                    chosen = Some(String::new());
                                }
                                ui.add(
                                    egui::Label::new(RichText::new(shown).color(p.text_weak))
                                        .truncate(),
                                );
                                if let Some(c) = chosen {
                                    *value = c.clone();
                                    pending.push((name.clone(), Value::String(c)));
                                }
                            }
                            ControlKind::Folder { value, .. } => {
                                let shown = if value.is_empty() {
                                    "(none)".to_string()
                                } else {
                                    value.clone()
                                };
                                let mut chosen: Option<String> = None;
                                if ui.add(theme::secondary_button("Choose…")).clicked() {
                                    if let Some(d) = rfd::FileDialog::new().pick_folder() {
                                        chosen = Some(d.to_string_lossy().into_owned());
                                    }
                                }
                                if !value.is_empty()
                                    && widgets::icon_button(ui, "✖", "Clear").clicked()
                                {
                                    chosen = Some(String::new());
                                }
                                ui.add(
                                    egui::Label::new(RichText::new(shown).color(p.text_weak))
                                        .truncate(),
                                );
                                if let Some(c) = chosen {
                                    *value = c.clone();
                                    pending.push((name.clone(), Value::String(c)));
                                }
                            }
                            ControlKind::Button { .. } | ControlKind::Label { .. } => {}
                        }
                    });
                    ui.end_row();
                }
            });
        self.folders = folder_cache;
        for (name, value) in pending {
            self.send(backend, toasts, &name, value);
        }
        if reset {
            if let Some((wallpaper, display)) = self.key.clone() {
                if let Err(e) = backend.call(Request::ResetProperties { wallpaper, display }) {
                    toasts.error(e.to_string());
                }
                self.key = None;
            }
        }
    }
}

/// Names of controls whose Wallpaper Engine display condition does not hold right now.
fn hidden_controls(controls: &[(String, Control)]) -> std::collections::HashSet<String> {
    let lookup = |name: &str| -> Option<Value> {
        controls
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| crate::we::project::we_value(c, c.value().as_ref()).unwrap_or(Value::Null))
    };
    controls
        .iter()
        .filter(|(_, c)| {
            c.condition
                .as_deref()
                .is_some_and(|cond| !crate::we::condition::holds(cond, &lookup))
        })
        .map(|(n, _)| n.clone())
        .collect()
}

fn folder_files(
    folder: &str,
    filter: &str,
    root: &std::path::Path,
    cache: &mut HashMap<String, Vec<String>>,
) -> Vec<String> {
    if let Some(c) = cache.get(folder) {
        return c.clone();
    }
    let patterns: Vec<String> = filter
        .split('|')
        .map(|p| {
            p.trim()
                .trim_start_matches('*')
                .trim_start_matches('.')
                .to_ascii_lowercase()
        })
        .collect();
    let mut names: Vec<String> = std::fs::read_dir(root.join(folder))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    let ext = std::path::Path::new(n)
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    patterns.iter().any(|p| p.is_empty() || *p == ext)
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    cache.insert(folder.to_string(), names.clone());
    names
}

fn parse_hex(s: &str) -> [u8; 3] {
    let h = s.trim().trim_start_matches('#');
    if h.len() >= 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&h[0..2], 16),
            u8::from_str_radix(&h[2..4], 16),
            u8::from_str_radix(&h[4..6], 16),
        ) {
            return [r, g, b];
        }
    }
    [255, 255, 255]
}
