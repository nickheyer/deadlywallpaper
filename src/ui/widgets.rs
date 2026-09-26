use eframe::egui;
use crate::model::Display;
use crate::model::display::virtual_bounds;
use std::time::{Duration, Instant};

/// Monitor layout strip; returns the id of a clicked display.
pub fn display_strip(ui: &mut egui::Ui, displays: &[Display], selected: Option<&str>, all_selected: bool, active: &[String]) -> Option<String> {
    if displays.is_empty() {
        ui.weak("No displays");
        return None;
    }
    let bounds = virtual_bounds(displays);
    let height = 46.0;
    let scale = (height / bounds.h.max(1) as f32).min(220.0 / bounds.w.max(1) as f32);
    let size = egui::vec2(bounds.w as f32 * scale + 4.0, height + 4.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let mut clicked = None;
    for (i, d) in displays.iter().enumerate() {
        let r = egui::Rect::from_min_size(
            rect.min + egui::vec2((d.rect.x - bounds.x) as f32 * scale + 2.0, (d.rect.y - bounds.y) as f32 * scale + 2.0),
            egui::vec2(d.rect.w as f32 * scale, d.rect.h as f32 * scale),
        )
        .shrink(1.5);
        let resp = ui.interact(r, ui.id().with(("display", i)), egui::Sense::click());
        let is_selected = all_selected || selected == Some(d.id.as_str());
        let visuals = ui.visuals();
        let fill = if active.contains(&d.id) { visuals.selection.bg_fill.gamma_multiply(0.55) } else { visuals.widgets.inactive.bg_fill };
        let stroke = if is_selected {
            egui::Stroke::new(2.0, visuals.selection.stroke.color)
        } else if resp.hovered() {
            egui::Stroke::new(1.5, visuals.widgets.hovered.fg_stroke.color)
        } else {
            egui::Stroke::new(1.0, visuals.widgets.inactive.fg_stroke.color)
        };
        painter.rect(r, 3.0, fill, stroke, egui::StrokeKind::Inside);
        painter.text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(12.0), visuals.strong_text_color());
        let tip = format!("{}{}\n{}x{}", d.name, if d.primary { " (primary)" } else { "" }, d.rect.w, d.rect.h);
        if resp.on_hover_text(tip).clicked() {
            clicked = Some(d.id.clone());
        }
    }
    clicked
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<(String, bool, Instant)>,
}

impl Toasts {
    pub fn error(&mut self, text: impl Into<String>) {
        self.items.push((text.into(), true, Instant::now()));
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.items.push((text.into(), false, Instant::now()));
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.items.retain(|(_, err, t)| t.elapsed() < Duration::from_secs(if *err { 8 } else { 4 }));
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(500));
        egui::Area::new(egui::Id::new("toasts")).anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_max_width(420.0);
            for (text, err, _) in self.items.iter().rev().take(4) {
                let color = if *err { egui::Color32::from_rgb(200, 70, 70) } else { ui.visuals().widgets.active.bg_fill };
                egui::Frame::popup(ui.style()).fill(color.gamma_multiply(0.9)).show(ui, |ui| {
                    ui.label(egui::RichText::new(text).color(egui::Color32::WHITE));
                });
                ui.add_space(4.0);
            }
        });
    }
}

/// A labeled row inside a settings form.
pub fn row(ui: &mut egui::Ui, label: &str, help: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        let l = ui.label(label);
        if !help.is_empty() {
            l.on_hover_text(help);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(title).strong().size(15.0));
    ui.separator();
}
