//! Visual language shared by every page: spacing, rounding, type sizes, cards and glyphs.

use crate::model::Kind;
use eframe::egui::{self, Color32, CornerRadius, FontId, Frame, Margin, RichText, TextStyle};

pub const LOGO_URI: &str = "bytes://deadlywp-logo.png";

pub fn logo() -> egui::Image<'static> {
    egui::Image::from_bytes(LOGO_URI, include_bytes!("../../assets/icon.png"))
}

pub fn install(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size = egui::vec2(40.0, 28.0);
        style.spacing.window_margin = Margin::same(16);
        style.spacing.menu_margin = Margin::same(8);
        style.spacing.combo_width = 170.0;
        style.spacing.slider_width = 170.0;
        style.spacing.text_edit_width = 240.0;
        for w in [
            &mut style.visuals.widgets.noninteractive,
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(6);
        }
        style.visuals.window_corner_radius = CornerRadius::same(12);
        style.visuals.menu_corner_radius = CornerRadius::same(8);
        style.text_styles.insert(TextStyle::Heading, FontId::proportional(24.0));
        style.text_styles.insert(TextStyle::Body, FontId::proportional(14.5));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(14.5));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(13.0));
    });
}

pub fn accent(ui: &egui::Ui) -> Color32 {
    ui.visuals().selection.bg_fill
}

/// A raised surface for grouped content.
pub fn card(ui: &egui::Ui) -> Frame {
    Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(16))
}

pub fn page_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).text_style(TextStyle::Heading).strong());
}

pub fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).size(16.0).strong());
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(ui.visuals().weak_text_color()));
}

pub fn kind_glyph(kind: Kind) -> &'static str {
    match kind {
        Kind::Video => "🎬",
        Kind::Gif => "🎞",
        Kind::Picture => "🖼",
        Kind::VideoStream => "📡",
        Kind::Web | Kind::Url => "🌐",
        Kind::WebAudio => "🎵",
        Kind::Program => "⚙",
    }
}

/// Placeholder tile colour for wallpapers without a thumbnail.
pub fn kind_color(kind: Kind) -> Color32 {
    match kind {
        Kind::Video => Color32::from_rgb(58, 76, 110),
        Kind::Gif => Color32::from_rgb(96, 70, 110),
        Kind::Picture => Color32::from_rgb(62, 98, 84),
        Kind::VideoStream => Color32::from_rgb(110, 76, 58),
        Kind::Web | Kind::Url => Color32::from_rgb(52, 88, 108),
        Kind::WebAudio => Color32::from_rgb(104, 62, 92),
        Kind::Program => Color32::from_rgb(80, 80, 88),
    }
}
