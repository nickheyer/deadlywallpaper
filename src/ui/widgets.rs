use crate::model::{Display, Kind};
use crate::ui::theme;
use eframe::egui::load::{SizeHint, TexturePoll};
use eframe::egui::{
    self, Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, Painter, Pos2,
    Rect, RichText, Sense, Stroke, StrokeKind, TextEdit, TextStyle, TextureOptions, Vec2,
};
use eframe::epaint::RectShape;
use eframe::epaint::text::LayoutJob;
use std::path::Path;
use std::time::{Duration, Instant};

pub fn nav_item(
    ui: &mut egui::Ui,
    selected: bool,
    glyph: &str,
    label: &str,
    shortcut: &str,
) -> egui::Response {
    let p = theme::palette(ui);
    ui.scope(|ui| {
        ui.visuals_mut().selection.bg_fill = p.accent_soft;
        ui.visuals_mut().selection.stroke = Stroke::new(1.0, p.text_strong);
        ui.add(
            Button::new(format!("{glyph}   {label}"))
                .right_text("")
                .selected(selected)
                .frame_when_inactive(selected)
                .corner_radius(CornerRadius::same(6))
                .min_size(egui::vec2(ui.available_width(), 36.0)),
        )
        .on_hover_text(shortcut)
    })
    .inner
}

pub fn chip(ui: &mut egui::Ui, selected: bool, text: &str) -> egui::Response {
    let p = theme::palette(ui);
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, p.text);
    let pad = egui::vec2(12.0, 5.0);
    let (rect, response) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let (fill, stroke, fg) = if selected {
            (p.accent, Stroke::NONE, p.on_accent)
        } else if hovered {
            (
                p.control_hover,
                Stroke::new(1.0, p.stroke_strong),
                p.text_strong,
            )
        } else {
            (p.control, Stroke::new(1.0, p.stroke), p.text)
        };
        ui.painter()
            .rect(rect, rect.height() / 2.0, fill, stroke, StrokeKind::Inside);
        ui.painter().galley(rect.min + pad, galley, fg);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id_salt: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let p = theme::palette(ui);
    let mut changed = false;
    // `horizontal` runs right to left inside a right-to-left parent; walk the options
    // backwards there so they still read in array order.
    let reversed = ui.layout().main_dir() == egui::Direction::RightToLeft;
    ui.push_id(id_salt, |ui| {
        ui.visuals_mut().selection.bg_fill = p.accent_soft;
        ui.visuals_mut().selection.stroke = Stroke::new(1.0, p.text_strong);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let mut ordered: Vec<&(T, &str)> = options.iter().collect();
            if reversed {
                ordered.reverse();
            }
            for &&(option, label) in &ordered {
                if ui
                    .add(Button::new(label).selected(*value == option))
                    .clicked()
                    && *value != option
                {
                    *value = option;
                    changed = true;
                }
            }
        });
    });
    changed
}

pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = theme::palette(ui);
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(40.0, 22.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(response.id, *on);
        let enabled = ui.is_enabled();
        let off = if response.hovered() {
            p.control_active
        } else {
            p.control
        };
        let track = off.lerp_to_gamma(p.accent, t);
        let stroke = p.stroke_strong.lerp_to_gamma(p.accent, t);
        let radius = rect.height() / 2.0;
        let painter = ui.painter();
        painter.rect(
            rect,
            radius,
            track,
            Stroke::new(1.0, stroke),
            StrokeKind::Inside,
        );
        let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        let knob = if enabled {
            Color32::WHITE
        } else {
            Color32::from_gray(200)
        };
        painter.circle_filled(egui::pos2(knob_x, rect.center().y), radius - 4.0, knob);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub fn icon_button(ui: &mut egui::Ui, glyph: &str, tooltip: &str) -> egui::Response {
    let p = theme::palette(ui);
    let r = ui.add(
        Button::new(RichText::new(glyph).size(14.0).color(p.text_weak))
            .frame_when_inactive(false)
            .min_size(egui::vec2(28.0, 28.0))
            .corner_radius(CornerRadius::same(7)),
    );
    if tooltip.is_empty() {
        r
    } else {
        r.on_hover_text(tooltip)
    }
}

/// Consumes take_focus to focus the search field once.
pub fn search_box(
    ui: &mut egui::Ui,
    text: &mut String,
    take_focus: &mut bool,
    width: f32,
) -> egui::Response {
    let p = theme::palette(ui);
    let frame = Frame::new()
        .fill(p.input)
        .stroke(Stroke::new(1.0, p.stroke))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(10, 4));
    let mut prepared = frame.begin(ui);
    let response = {
        let ui = &mut prepared.content_ui;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new("🔍").color(p.text_faint));
            let edit = ui.add(
                TextEdit::singleline(text)
                    .hint_text(RichText::new("Search").color(p.text_faint))
                    .desired_width(width)
                    .frame(Frame::NONE)
                    .margin(Margin::symmetric(0, 4)),
            );
            if *take_focus {
                edit.request_focus();
                *take_focus = false;
            }
            if edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                text.clear();
                edit.surrender_focus();
            }
            if !text.is_empty() && icon_button(ui, "✖", "Clear").clicked() {
                text.clear();
            }
            edit
        })
        .inner
    };
    if response.has_focus() {
        prepared.frame.stroke = Stroke::new(1.0, p.accent);
    }
    prepared.end(ui);
    response
}

/// Paint one line of text, cut with an ellipsis when wider than `max_width`.
pub fn elided(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) -> Rect {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap.max_width = max_width.max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = painter.layout_job(job);
    let rect = align.anchor_size(pos, galley.size());
    painter.galley(rect.min, galley, color);
    rect
}

pub fn badge(
    painter: &Painter,
    pos: Pos2,
    align: Align2,
    text: &str,
    fill: Color32,
    fg: Color32,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), FontId::proportional(11.5), fg);
    let pad = egui::vec2(8.0, 4.0);
    let rect = align.anchor_size(pos, galley.size() + pad * 2.0);
    painter.rect_filled(rect, rect.height() / 2.0, fill);
    painter.galley(rect.min + pad, galley, fg);
    rect
}

/// UV rectangle that crops an image of `image` size to cover `target` without distortion.
pub fn cover_uv(image: Vec2, target: Vec2) -> Rect {
    if image.x <= 0.0 || image.y <= 0.0 || target.x <= 0.0 || target.y <= 0.0 {
        return Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    }
    let scale = (target.x / image.x).max(target.y / image.y);
    let uw = (target.x / (image.x * scale)).clamp(0.0, 1.0);
    let uh = (target.y / (image.y * scale)).clamp(0.0, 1.0);
    Rect::from_min_size(
        egui::pos2((1.0 - uw) * 0.5, (1.0 - uh) * 0.5),
        egui::vec2(uw, uh),
    )
}

pub fn thumbnail_uri(path: &Path) -> String {
    format!("file://{}", path.display())
}

/// Draw a cropped thumbnail or placeholder; return whether the image was available.
pub fn thumbnail(
    ui: &egui::Ui,
    rect: Rect,
    uri: Option<&str>,
    kind: Kind,
    corner: CornerRadius,
) -> bool {
    let p = theme::palette(ui);
    let painter = ui.painter();
    if let Some(uri) = uri {
        if let Ok(TexturePoll::Ready { texture }) =
            ui.ctx()
                .try_load_texture(uri, TextureOptions::LINEAR, SizeHint::default())
        {
            let uv = cover_uv(texture.size, rect.size());
            painter
                .add(RectShape::filled(rect, corner, Color32::WHITE).with_texture(texture.id, uv));
            return true;
        }
    }
    painter.rect_filled(rect, corner, theme::kind_color(kind, p.dark));
    let size = (rect.height() * 0.3).clamp(16.0, 40.0);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        theme::kind_glyph(kind),
        FontId::proportional(size),
        theme::kind_glyph_color(p.dark),
    );
    false
}

pub struct DiagramItem<'a> {
    pub display: &'a Display,
    pub thumbnail: Option<&'a Path>,
    pub kind: Option<Kind>,
    pub paused: bool,
}

/// Draw displays to scale; return the clicked display.
pub fn display_diagram(
    ui: &mut egui::Ui,
    items: &[DiagramItem<'_>],
    selected: Option<&str>,
    all_selected: bool,
    max_size: Vec2,
) -> Option<String> {
    let p = theme::palette(ui);
    if items.is_empty() {
        theme::weak(ui, "No displays");
        return None;
    }
    let bounds = crate::geom::Rect::bounds(items.iter().map(|i| &i.display.rect));
    let scale = (max_size.x / bounds.w.max(1) as f32).min(max_size.y / bounds.h.max(1) as f32);
    let size = egui::vec2(bounds.w as f32 * scale, bounds.h as f32 * scale);
    let (outer, _) = ui.allocate_exact_size(size + egui::vec2(8.0, 8.0), Sense::hover());
    let origin = outer.min + egui::vec2(4.0, 4.0);
    let mut clicked = None;
    for (i, item) in items.iter().enumerate() {
        let d = item.display;
        let rect = Rect::from_min_size(
            origin
                + egui::vec2(
                    (d.rect.x - bounds.x) as f32 * scale,
                    (d.rect.y - bounds.y) as f32 * scale,
                ),
            egui::vec2(d.rect.w as f32 * scale, d.rect.h as f32 * scale),
        )
        .shrink(3.0);
        let resp = ui
            .interact(rect, ui.id().with(("display", i)), Sense::click())
            .on_hover_cursor(CursorIcon::PointingHand);
        let is_selected = all_selected || selected == Some(d.id.as_str());
        let corner = CornerRadius::same(8);
        let painter = ui.painter_at(rect.expand(4.0));
        painter.rect_filled(
            rect,
            corner,
            if p.dark {
                Color32::from_rgb(12, 12, 14)
            } else {
                Color32::from_rgb(210, 212, 220)
            },
        );
        match (item.thumbnail, item.kind) {
            (Some(path), kind) => {
                let uri = thumbnail_uri(path);
                thumbnail(ui, rect, Some(&uri), kind.unwrap_or(Kind::Picture), corner);
            }
            (None, Some(kind)) => {
                thumbnail(ui, rect, None, kind, corner);
            }
            (None, None) => {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "No wallpaper",
                    FontId::proportional(12.0),
                    p.text_faint,
                );
            }
        }
        if item.paused {
            painter.rect_filled(rect, corner, Color32::from_black_alpha(90));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "⏸",
                FontId::proportional(22.0),
                Color32::from_white_alpha(220),
            );
        }
        let stroke = if is_selected {
            Stroke::new(2.5, p.accent)
        } else if resp.hovered() {
            Stroke::new(1.5, p.stroke_strong.lerp_to_gamma(p.text_weak, 0.5))
        } else {
            Stroke::new(1.0, p.stroke_strong)
        };
        painter.rect_stroke(rect, corner, stroke, StrokeKind::Inside);
        badge(
            &painter,
            rect.min + egui::vec2(8.0, 8.0),
            Align2::LEFT_TOP,
            &format!("{}", i + 1),
            if is_selected {
                p.accent
            } else {
                Color32::from_black_alpha(170)
            },
            Color32::WHITE,
        );
        if rect.width() > 90.0 {
            let strip = Rect::from_min_max(
                egui::pos2(rect.left(), rect.bottom() - 22.0),
                rect.right_bottom(),
            );
            painter.rect_filled(
                strip,
                CornerRadius {
                    nw: 0,
                    ne: 0,
                    sw: 8,
                    se: 8,
                },
                Color32::from_black_alpha(150),
            );
            elided(
                &painter,
                strip.left_center() + egui::vec2(8.0, 0.0),
                Align2::LEFT_CENTER,
                &d.name,
                FontId::proportional(11.5),
                Color32::from_white_alpha(230),
                strip.width() - 16.0,
            );
        }
        let tip = format!(
            "{}{}\n{}×{} at {},{} · scale {:.2}",
            d.name,
            if d.primary { " (primary)" } else { "" },
            d.rect.w,
            d.rect.h,
            d.rect.x,
            d.rect.y,
            d.scale
        );
        if resp.on_hover_text(tip).clicked() {
            clicked = Some(d.id.clone());
        }
    }
    clicked
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

struct Toast {
    text: String,
    kind: ToastKind,
    at: Instant,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn error(&mut self, text: impl Into<String>) {
        self.push(text.into(), ToastKind::Error);
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push(text.into(), ToastKind::Info);
    }

    pub fn success(&mut self, text: impl Into<String>) {
        self.push(text.into(), ToastKind::Success);
    }

    fn push(&mut self, text: String, kind: ToastKind) {
        self.items.retain(|t| t.text != text);
        self.items.push(Toast {
            text,
            kind,
            at: Instant::now(),
        });
        if self.items.len() > 5 {
            self.items.remove(0);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        self.items.retain(|t| {
            t.at.elapsed() < Duration::from_secs(if t.kind == ToastKind::Error { 9 } else { 4 })
        });
        if self.items.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let p = theme::palette_of(ctx);
        let mut dismiss = None;
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                for (i, toast) in self.items.iter().enumerate() {
                    let (accent, glyph) = match toast.kind {
                        ToastKind::Info => (p.accent, "ℹ"),
                        ToastKind::Success => (p.success, "✔"),
                        ToastKind::Error => (p.danger, "⚠"),
                    };
                    let frame = Frame::new()
                        .fill(p.elevated)
                        .stroke(Stroke::new(1.0, p.stroke_strong))
                        .corner_radius(CornerRadius::same(10))
                        .shadow(ui.visuals().popup_shadow)
                        .inner_margin(Margin {
                            left: 14,
                            right: 14,
                            top: 10,
                            bottom: 10,
                        });
                    let response = frame
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                ui.label(RichText::new(glyph).color(accent).size(15.0));
                                ui.add(
                                    egui::Label::new(RichText::new(&toast.text).color(p.text))
                                        .wrap(),
                                );
                            });
                        })
                        .response;
                    let rect = response.rect;
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            rect.left_top() + egui::vec2(0.0, 8.0),
                            egui::vec2(3.0, rect.height() - 16.0),
                        ),
                        CornerRadius::same(2),
                        accent,
                    );
                    if response
                        .interact(Sense::click())
                        .on_hover_cursor(CursorIcon::PointingHand)
                        .clicked()
                    {
                        dismiss = Some(i);
                    }
                }
            });
        if let Some(i) = dismiss {
            self.items.remove(i);
        }
    }
}

pub fn row(ui: &mut egui::Ui, label: &str, help: &str, add: impl FnOnce(&mut egui::Ui)) {
    let p = theme::palette(ui);
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        ui.spacing_mut().item_spacing.x = 20.0;
        let control_w = (ui.available_width() * 0.48).min(280.0);
        let label_w = ui.available_width() - control_w - 20.0;
        ui.allocate_ui_with_layout(
            egui::vec2(label_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(label_w);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(label).color(if ui.is_enabled() {
                    p.text
                } else {
                    p.text_weak
                }));
                if !help.is_empty() {
                    ui.add(egui::Label::new(RichText::new(help).small().color(p.text_weak)).wrap());
                }
            },
        );
        ui.allocate_ui_with_layout(
            egui::vec2(control_w, 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            add,
        );
    });
}

pub fn divider(ui: &mut egui::Ui) {
    let p = theme::palette(ui);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.stroke));
}

/// Returns true when the action is clicked.
pub fn empty_state(
    ui: &mut egui::Ui,
    glyph: &str,
    title: &str,
    body: &str,
    action: Option<&str>,
) -> bool {
    let p = theme::palette(ui);
    let mut clicked = false;
    ui.add_space((ui.available_height() * 0.22).max(24.0));
    ui.vertical_centered(|ui| {
        ui.set_max_width(440.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 72.0), Sense::hover());
        ui.painter().circle_filled(rect.center(), 36.0, p.control);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::proportional(30.0),
            p.text_weak,
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(title)
                .size(18.0)
                .strong()
                .color(p.text_strong),
        );
        if !body.is_empty() {
            ui.add(egui::Label::new(RichText::new(body).color(p.text_weak)).wrap());
        }
        if let Some(a) = action {
            ui.add_space(8.0);
            if ui
                .add(theme::primary(a).min_size(egui::vec2(180.0, 36.0)))
                .clicked()
            {
                clicked = true;
            }
        }
    });
    clicked
}
