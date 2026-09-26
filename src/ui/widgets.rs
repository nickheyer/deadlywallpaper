use crate::model::Display;
use crate::model::display::virtual_bounds;
use crate::ui::theme;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind};
use std::time::{Duration, Instant};

/// A pill-shaped toggle used for filters and targets.
pub fn chip(ui: &mut egui::Ui, selected: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Button::selectable(selected, text).corner_radius(CornerRadius::same(14)))
}

/// A row of mutually exclusive choices.
pub fn segmented<T: Copy + PartialEq>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (v, label) in options {
            if ui.add(egui::Button::selectable(*value == *v, *label).corner_radius(CornerRadius::same(6))).clicked() && *value != *v {
                *value = *v;
                changed = true;
            }
        }
    });
    changed
}

/// Monitor layout drawn to scale. Each display shows the thumbnail of the wallpaper it runs
/// (when there is one), its number and name. Returns the display that was clicked.
pub fn display_diagram(ui: &mut egui::Ui, displays: &[Display], selected: Option<&str>, all_selected: bool, thumbnails: &[(String, Option<std::path::PathBuf>)], max_size: egui::Vec2) -> Option<String> {
    if displays.is_empty() {
        ui.label(RichText::new("No displays detected").color(ui.visuals().weak_text_color()));
        return None;
    }
    let bounds = virtual_bounds(displays);
    let scale = (max_size.x / bounds.w.max(1) as f32).min(max_size.y / bounds.h.max(1) as f32);
    let size = egui::vec2(bounds.w as f32 * scale, bounds.h as f32 * scale);
    let (outer, _) = ui.allocate_exact_size(size + egui::vec2(8.0, 8.0), Sense::hover());
    let origin = outer.min + egui::vec2(4.0, 4.0);
    let mut clicked = None;
    let accent = theme::accent(ui);
    for (i, d) in displays.iter().enumerate() {
        let rect = Rect::from_min_size(origin + egui::vec2((d.rect.x - bounds.x) as f32 * scale, (d.rect.y - bounds.y) as f32 * scale), egui::vec2(d.rect.w as f32 * scale, d.rect.h as f32 * scale)).shrink(3.0);
        let resp = ui.interact(rect, ui.id().with(("display", i)), Sense::click());
        let is_selected = all_selected || selected == Some(d.id.as_str());
        let painter = ui.painter_at(rect.expand(3.0));
        painter.rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(28, 30, 34));
        let thumb = thumbnails.iter().find(|(id, _)| *id == d.id).and_then(|(_, t)| t.clone());
        match thumb {
            Some(path) => {
                egui::Image::from_uri(format!("file://{}", path.display())).corner_radius(CornerRadius::same(6)).paint_at(ui, rect);
            }
            None => {
                painter.text(rect.center(), Align2::CENTER_CENTER, "No wallpaper", FontId::proportional(12.0), Color32::from_gray(150));
            }
        }
        let stroke = if is_selected {
            Stroke::new(3.0, accent)
        } else if resp.hovered() {
            Stroke::new(2.0, ui.visuals().widgets.hovered.fg_stroke.color)
        } else {
            Stroke::new(1.0, Color32::from_gray(90))
        };
        painter.rect_stroke(rect, CornerRadius::same(6), stroke, StrokeKind::Inside);
        let badge = Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(24.0, 22.0));
        painter.rect_filled(badge, CornerRadius::same(5), Color32::from_black_alpha(170));
        painter.text(badge.center(), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(13.0), Color32::WHITE);
        let tip = format!("{}{}\n{}×{} · scale {:.2}", d.name, if d.primary { " (primary)" } else { "" }, d.rect.w, d.rect.h, d.scale);
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
        egui::Area::new(egui::Id::new("toasts")).anchor(Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.set_max_width(440.0);
            for (text, err, _) in self.items.iter().rev().take(4) {
                let (fill, fg) = if *err { (Color32::from_rgb(176, 58, 58), Color32::WHITE) } else { (Color32::from_rgb(40, 44, 50), Color32::from_gray(235)) };
                egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.label(RichText::new(text).color(fg));
                });
                ui.add_space(6.0);
            }
        });
    }
}

/// A labeled settings row: label and help on the left, the control on the right.
pub fn row(ui: &mut egui::Ui, label: &str, help: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(label);
            if !help.is_empty() {
                theme::hint(ui, help);
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
    });
}
