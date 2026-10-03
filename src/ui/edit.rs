//! The Edit dialog for one library entry, and the bulk dialog that writes the same fields to
//! several entries at once.

use crate::ipc::InfoPatch;
use crate::model::{Kind, Summary};
use crate::ui::{order, theme, widgets};
use eframe::egui::{self, CornerRadius, RichText, Sense, TextEdit};
use std::path::PathBuf;

pub const WIDTH: f32 = 600.0;

/// The editable fields of one library entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Form {
    pub title: String,
    pub author: String,
    pub desc: String,
    pub contact: String,
    pub license: String,
    pub arguments: String,
}

impl Form {
    pub fn of(w: &Summary) -> Form {
        Form {
            title: w.title.clone(),
            author: w.author.clone().unwrap_or_default(),
            desc: w.desc.clone().unwrap_or_default(),
            contact: w.contact.clone().unwrap_or_default(),
            license: w.license.clone().unwrap_or_default(),
            arguments: w.arguments.clone().unwrap_or_default(),
        }
    }

    /// Every field, so a cleared field clears the stored value; arguments only for programs.
    pub fn patch(&self, program: bool) -> InfoPatch {
        InfoPatch {
            title: Some(self.title.clone()),
            author: Some(self.author.clone()),
            desc: Some(self.desc.clone()),
            contact: Some(self.contact.clone()),
            license: Some(self.license.clone()),
            arguments: program.then(|| self.arguments.clone()),
        }
    }
}

/// What the dialog shows about the entry besides its fields.
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    pub thumbnail: Option<PathBuf>,
    pub workshop: Option<u64>,
    pub added: Option<u64>,
}

impl Entry {
    pub fn of(w: &Summary) -> Entry {
        Entry {
            id: w.id.clone(),
            kind: w.kind,
            thumbnail: w.thumbnail.clone(),
            workshop: w.workshop,
            added: w.added,
        }
    }
}

/// The dialog for one entry: its fields as edited, and as they were when it opened.
pub struct Single {
    pub entry: Entry,
    pub form: Form,
    pub original: Form,
}

impl Single {
    pub fn of(w: &Summary) -> Single {
        let form = Form::of(w);
        Single {
            entry: Entry::of(w),
            original: form.clone(),
            form,
        }
    }

    pub fn patch(&self) -> InfoPatch {
        self.form.patch(self.entry.kind == Kind::Program)
    }
}

/// The bulk dialog: a field ticked `true` is written to every entry, an empty value clearing it.
pub struct Many {
    pub ids: Vec<String>,
    pub author: (bool, String),
    pub license: (bool, String),
    pub contact: (bool, String),
    pub desc: (bool, String),
}

impl Many {
    pub fn new(ids: Vec<String>) -> Many {
        Many {
            ids,
            author: (false, String::new()),
            license: (false, String::new()),
            contact: (false, String::new()),
            desc: (false, String::new()),
        }
    }

    pub fn any(&self) -> bool {
        self.author.0 || self.license.0 || self.contact.0 || self.desc.0
    }

    pub fn patch(&self) -> InfoPatch {
        let field = |(set, value): &(bool, String)| set.then(|| value.clone());
        InfoPatch {
            title: None,
            author: field(&self.author),
            license: field(&self.license),
            contact: field(&self.contact),
            desc: field(&self.desc),
            arguments: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Save,
    Cancel,
}

/// Draw the dialog for one entry inside an open modal.
pub fn single(ui: &mut egui::Ui, state: &mut Single) -> Option<Outcome> {
    let p = theme::palette(ui);
    ui.set_width(WIDTH);
    header(ui, &state.entry);
    ui.add_space(18.0);
    let hint = |text: &str| RichText::new(text).color(p.text_faint);
    field(ui, "Title", |ui| {
        ui.add(
            TextEdit::singleline(&mut state.form.title)
                .hint_text(hint("Required"))
                .desired_width(f32::INFINITY),
        );
    });
    ui.columns(2, |cols| {
        field(&mut cols[0], "Author", |ui| {
            ui.add(
                TextEdit::singleline(&mut state.form.author)
                    .hint_text(hint("Unknown"))
                    .desired_width(f32::INFINITY),
            );
        });
        field(&mut cols[1], "License", |ui| {
            ui.add(
                TextEdit::singleline(&mut state.form.license)
                    .hint_text(hint("Unknown"))
                    .desired_width(f32::INFINITY),
            );
        });
    });
    field(ui, "Description", |ui| {
        widgets::text_area(ui, "desc", &mut state.form.desc, ui.available_width(), 6);
    });
    field(ui, "Website", |ui| {
        ui.add(
            TextEdit::singleline(&mut state.form.contact)
                .hint_text(hint("https://"))
                .desired_width(f32::INFINITY),
        );
    });
    if state.entry.kind == Kind::Program {
        field(ui, "Arguments", |ui| {
            ui.add(
                TextEdit::singleline(&mut state.form.arguments)
                    .hint_text(hint("Passed to the program when it starts"))
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
        });
    }
    ui.add_space(12.0);
    let changed = state.form != state.original;
    let titled = !state.form.title.trim().is_empty();
    let mut outcome = None;
    widgets::dialog_buttons(ui, |ui| {
        if ui
            .add_enabled(changed && titled, theme::primary("Save"))
            .clicked()
        {
            outcome = Some(Outcome::Save);
        }
        if ui.add(theme::secondary_button("Cancel")).clicked() {
            outcome = Some(Outcome::Cancel);
        }
        ui.add_space(6.0);
        if !titled {
            ui.label(RichText::new("A title is required").color(p.danger));
        } else if changed {
            theme::weak(ui, "Unsaved changes");
        }
    });
    outcome
}

/// Draw the bulk dialog inside an open modal.
pub fn many(ui: &mut egui::Ui, state: &mut Many) -> Option<Outcome> {
    let p = theme::palette(ui);
    ui.set_width(WIDTH);
    ui.label(
        RichText::new(format!(
            "Edit {}",
            crate::ui::library::count(state.ids.len(), "wallpaper")
        ))
        .size(18.0)
        .strong()
        .color(p.text_strong),
    );
    ui.add_space(4.0);
    theme::weak(
        ui,
        "A field switched on is written to every selected wallpaper; leave it empty to clear it.",
    );
    ui.add_space(18.0);
    let hint = |text: &str| RichText::new(text).color(p.text_faint);
    ui.columns(2, |cols| {
        toggled_field(&mut cols[0], "Author", &mut state.author.0, |ui| {
            ui.add(
                TextEdit::singleline(&mut state.author.1)
                    .hint_text(hint("Unknown"))
                    .desired_width(f32::INFINITY),
            );
        });
        toggled_field(&mut cols[1], "License", &mut state.license.0, |ui| {
            ui.add(
                TextEdit::singleline(&mut state.license.1)
                    .hint_text(hint("Unknown"))
                    .desired_width(f32::INFINITY),
            );
        });
    });
    toggled_field(ui, "Description", &mut state.desc.0, |ui| {
        widgets::text_area(ui, "desc-many", &mut state.desc.1, ui.available_width(), 4);
    });
    toggled_field(ui, "Website", &mut state.contact.0, |ui| {
        ui.add(
            TextEdit::singleline(&mut state.contact.1)
                .hint_text(hint("https://"))
                .desired_width(f32::INFINITY),
        );
    });
    ui.add_space(12.0);
    let mut outcome = None;
    widgets::dialog_buttons(ui, |ui| {
        if ui
            .add_enabled(state.any(), theme::primary("Save"))
            .clicked()
        {
            outcome = Some(Outcome::Save);
        }
        if ui.add(theme::secondary_button("Cancel")).clicked() {
            outcome = Some(Outcome::Cancel);
        }
        ui.add_space(6.0);
        if !state.any() {
            theme::weak(ui, "Switch on the fields to write");
        }
    });
    outcome
}

/// Thumbnail, title, and what the entry is.
fn header(ui: &mut egui::Ui, entry: &Entry) {
    let p = theme::palette(ui);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(112.0, 63.0), Sense::hover());
        let uri = entry.thumbnail.as_deref().map(widgets::thumbnail_uri);
        widgets::thumbnail(ui, rect, uri.as_deref(), entry.kind, CornerRadius::same(8));
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(
                RichText::new("Edit wallpaper")
                    .size(18.0)
                    .strong()
                    .color(p.text_strong),
            );
            let mut about = vec![entry.kind.label().to_string()];
            if let Some(id) = entry.workshop {
                about.push(format!("Steam Workshop item {id}"));
            }
            if let Some(secs) = entry.added {
                about.push(format!("Added {}", order::date_text(secs)));
            }
            theme::weak(ui, &about.join("  ·  "));
        });
    });
}

/// A label above a control that fills the width.
fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.label(RichText::new(label).small().color(p.text_weak));
        add(ui);
    });
    ui.add_space(8.0);
}

/// A field with a switch that decides whether it is written at all.
fn toggled_field(ui: &mut egui::Ui, label: &str, on: &mut bool, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette(ui);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            widgets::toggle(ui, on);
            ui.label(
                RichText::new(label)
                    .small()
                    .color(if *on { p.text } else { p.text_weak }),
            );
        });
        ui.add_enabled_ui(*on, add);
    });
    ui.add_space(8.0);
}
