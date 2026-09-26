use eframe::egui;
use crate::error::Result;
use crate::ipc::{Request, Response, Status};
use crate::model::Control;
use crate::model::props::ControlKind;
use crate::ui::Backend;
use crate::ui::widgets::Toasts;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Live property editor for one running wallpaper.
#[derive(Default)]
pub struct Panel {
    key: Option<(String, Option<String>)>,
    controls: Vec<(String, Control)>,
    root: PathBuf,
    folders: HashMap<String, Vec<String>>,
    last_send: Option<Instant>,
    error: Option<String>,
}

impl Panel {
    /// Load controls for `wallpaper` on `display` unless already shown.
    pub fn ensure(&mut self, backend: &mut Backend, wallpaper: &str, display: Option<&str>, root: PathBuf) -> Result<()> {
        let key = (wallpaper.to_string(), display.map(str::to_owned));
        if self.key.as_ref() == Some(&key) {
            return Ok(());
        }
        match backend.call(Request::Properties { wallpaper: wallpaper.into(), display: display.map(str::to_owned) })? {
            Response::Controls { controls, .. } => {
                self.controls = controls;
                self.key = Some(key);
                self.root = root;
                self.folders.clear();
                self.error = None;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    pub fn invalidate_if_gone(&mut self, status: Option<&Status>) {
        if let (Some((wallpaper, _)), Some(s)) = (&self.key, status) {
            if !s.active.iter().any(|a| &a.wallpaper == wallpaper) {
                self.key = None;
                self.controls.clear();
            }
        }
    }

    fn send(&mut self, backend: &mut Backend, toasts: &mut Toasts, name: &str, value: Value) {
        let Some((wallpaper, display)) = self.key.clone() else { return };
        if let Err(e) = backend.call(Request::SetProperty { wallpaper, display, name: name.into(), value }) {
            toasts.error(e.to_string());
        }
        self.last_send = Some(Instant::now());
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, backend: &mut Backend, toasts: &mut Toasts) {
        if let Some(e) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), e);
        }
        let mut pending: Vec<(String, Value)> = Vec::new();
        let mut reset = false;
        let throttle_ok = self.last_send.is_none_or(|t| t.elapsed() > Duration::from_millis(80));
        egui::Grid::new("props").num_columns(2).spacing([10.0, 8.0]).striped(true).show(ui, |ui| {
            for (i, (name, control)) in self.controls.iter_mut().enumerate() {
                let label = if control.text.is_empty() { name.clone() } else { control.text.clone() };
                match &mut control.kind {
                    ControlKind::Label { value } => {
                        ui.label(egui::RichText::new(value.as_str()).strong());
                        ui.end_row();
                        continue;
                    }
                    ControlKind::Button { value } => {
                        ui.label(&label);
                        if ui.button(if value.is_empty() { "Run" } else { value.as_str() }).clicked() {
                            pending.push((name.clone(), Value::Null));
                        }
                        ui.end_row();
                        continue;
                    }
                    _ => {}
                }
                let l = ui.label(&label);
                if let Some(h) = &control.help {
                    l.on_hover_text(h);
                }
                match &mut control.kind {
                    ControlKind::Slider { value, min, max, step } => {
                        let (lo, hi) = (min.min(*max), max.max(*min));
                        let r = ui.add(egui::Slider::new(value, lo..=hi).step_by(*step).show_value(true));
                        if r.drag_stopped() || (r.changed() && (!r.dragged() || throttle_ok)) {
                            pending.push((name.clone(), Value::from(*value)));
                        }
                    }
                    ControlKind::Checkbox { value } => {
                        if ui.checkbox(value, "").changed() {
                            pending.push((name.clone(), Value::Bool(*value)));
                        }
                    }
                    ControlKind::Dropdown { value, items } | ControlKind::ScalerDropdown { value, items } => {
                        let current = items.get(*value as usize).cloned().unwrap_or_default();
                        egui::ComboBox::from_id_salt(("dd", i)).selected_text(current).show_ui(ui, |ui| {
                            for (idx, item) in items.iter().enumerate() {
                                if ui.selectable_value(value, idx as i64, item).clicked() {
                                    pending.push((name.clone(), Value::from(idx as i64)));
                                }
                            }
                        });
                    }
                    ControlKind::Textbox { value } => {
                        let r = ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                        if r.lost_focus() && r.changed() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            pending.push((name.clone(), Value::String(value.clone())));
                        }
                    }
                    ControlKind::Color { value } => {
                        let mut rgb = parse_hex(value);
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            *value = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                            pending.push((name.clone(), Value::String(value.clone())));
                        }
                    }
                    ControlKind::FolderDropdown { value, folder, filter } => {
                        let (folder, filter) = (folder.clone(), filter.clone());
                        let current = value.clone().unwrap_or_else(|| "(none)".into());
                        let mut chosen: Option<Option<String>> = None;
                        let names = folder_files(&folder, &filter, &self.root, &mut self.folders);
                        egui::ComboBox::from_id_salt(("fd", i)).selected_text(current).show_ui(ui, |ui| {
                            if ui.selectable_label(value.is_none(), "(none)").clicked() {
                                chosen = Some(None);
                            }
                            for n in &names {
                                if ui.selectable_label(value.as_deref() == Some(n), n).clicked() {
                                    chosen = Some(Some(n.clone()));
                                }
                            }
                        });
                        if ui.small_button("＋").on_hover_text("Copy a file into this folder").clicked() {
                            if let Some(files) = rfd::FileDialog::new().pick_files() {
                                let dir = self.root.join(&folder);
                                let _ = std::fs::create_dir_all(&dir);
                                let mut last = None;
                                for f in files {
                                    let name = crate::paths::file_name(&f);
                                    if std::fs::copy(&f, dir.join(&name)).is_ok() {
                                        last = Some(name);
                                    }
                                }
                                self.folders.remove(&folder);
                                if let Some(n) = last {
                                    chosen = Some(Some(n));
                                }
                            }
                        }
                        if let Some(c) = chosen {
                            *value = c.clone();
                            pending.push((name.clone(), c.map(Value::String).unwrap_or(Value::Null)));
                        }
                    }
                    ControlKind::Button { .. } | ControlKind::Label { .. } => {}
                }
                ui.end_row();
            }
        });
        ui.add_space(8.0);
        if ui.button("Restore defaults").clicked() {
            reset = true;
        }
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

fn folder_files(folder: &str, filter: &str, root: &std::path::Path, cache: &mut HashMap<String, Vec<String>>) -> Vec<String> {
    if let Some(c) = cache.get(folder) {
        return c.clone();
    }
    let patterns: Vec<String> = filter.split('|').map(|p| p.trim().trim_start_matches('*').trim_start_matches('.').to_ascii_lowercase()).collect();
    let mut names: Vec<String> = std::fs::read_dir(root.join(folder))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    let ext = std::path::Path::new(n).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
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
        if let (Ok(r), Ok(g), Ok(b)) = (u8::from_str_radix(&h[0..2], 16), u8::from_str_radix(&h[2..4], 16), u8::from_str_radix(&h[4..6], 16)) {
            return [r, g, b];
        }
    }
    [255, 255, 255]
}
