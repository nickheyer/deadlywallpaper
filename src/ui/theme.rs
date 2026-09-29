use crate::model::Kind;
use eframe::egui::style::{HandleShape, ScrollStyle, Style, WidgetVisuals};
use eframe::egui::{self, Button, Color32, CornerRadius, CursorIcon, FontId, Frame, Margin, RichText, Shadow, Stroke, TextStyle, Theme};

pub const LOGO_URI: &str = "bytes://deadlywp-logo.png";

pub fn logo() -> egui::Image<'static> {
    egui::Image::from_bytes(LOGO_URI, include_bytes!("../../assets/icon.png"))
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub elevated: Color32,
    pub stroke: Color32,
    pub stroke_strong: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    pub input: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_weak: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub on_accent: Color32,
    pub accent_soft: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
    /// Scrim drawn over thumbnails under overlay controls.
    pub overlay: Color32,
}

impl Palette {
    pub fn dark() -> Palette {
        Palette {
            dark: true,
            bg: Color32::from_rgb(19, 20, 23),
            sidebar: Color32::from_rgb(24, 25, 29),
            surface: Color32::from_rgb(30, 31, 36),
            surface_hover: Color32::from_rgb(36, 37, 43),
            elevated: Color32::from_rgb(38, 39, 45),
            stroke: Color32::from_rgb(45, 47, 54),
            stroke_strong: Color32::from_rgb(70, 72, 82),
            control: Color32::from_rgb(44, 46, 53),
            control_hover: Color32::from_rgb(54, 56, 65),
            control_active: Color32::from_rgb(62, 64, 74),
            input: Color32::from_rgb(15, 16, 19),
            text: Color32::from_rgb(225, 226, 231),
            text_strong: Color32::from_rgb(248, 248, 250),
            text_weak: Color32::from_rgb(150, 153, 163),
            text_faint: Color32::from_rgb(100, 103, 114),
            accent: Color32::from_rgb(86, 140, 255),
            accent_hover: Color32::from_rgb(112, 160, 255),
            on_accent: Color32::WHITE,
            accent_soft: Color32::from_rgba_unmultiplied(86, 140, 255, 44),
            success: Color32::from_rgb(72, 190, 120),
            warning: Color32::from_rgb(247, 168, 66),
            danger: Color32::from_rgb(236, 92, 92),
            overlay: Color32::from_black_alpha(150),
        }
    }

    pub fn light() -> Palette {
        Palette {
            dark: false,
            bg: Color32::from_rgb(244, 244, 247),
            sidebar: Color32::from_rgb(235, 236, 240),
            surface: Color32::WHITE,
            surface_hover: Color32::from_rgb(247, 248, 251),
            elevated: Color32::WHITE,
            stroke: Color32::from_rgb(222, 224, 230),
            stroke_strong: Color32::from_rgb(190, 193, 203),
            control: Color32::from_rgb(234, 235, 240),
            control_hover: Color32::from_rgb(224, 226, 233),
            control_active: Color32::from_rgb(212, 215, 225),
            input: Color32::WHITE,
            text: Color32::from_rgb(38, 39, 45),
            text_strong: Color32::from_rgb(12, 12, 16),
            text_weak: Color32::from_rgb(98, 101, 112),
            text_faint: Color32::from_rgb(150, 153, 163),
            accent: Color32::from_rgb(44, 106, 235),
            accent_hover: Color32::from_rgb(34, 92, 214),
            on_accent: Color32::WHITE,
            accent_soft: Color32::from_rgba_unmultiplied(44, 106, 235, 34),
            success: Color32::from_rgb(36, 150, 88),
            warning: Color32::from_rgb(200, 122, 18),
            danger: Color32::from_rgb(210, 58, 58),
            overlay: Color32::from_black_alpha(130),
        }
    }
}

pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { Palette::dark() } else { Palette::light() }
}

pub fn palette_of(ctx: &egui::Context) -> Palette {
    match ctx.theme() {
        Theme::Dark => Palette::dark(),
        Theme::Light => Palette::light(),
    }
}

pub fn install(ctx: &egui::Context) {
    ctx.style_mut_of(Theme::Dark, |s| apply(s, &Palette::dark()));
    ctx.style_mut_of(Theme::Light, |s| apply(s, &Palette::light()));
}

fn apply(style: &mut Style, p: &Palette) {
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size = egui::vec2(36.0, 30.0);
    style.spacing.window_margin = Margin::same(20);
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.combo_width = 190.0;
    style.spacing.slider_width = 190.0;
    style.spacing.slider_rail_height = 6.0;
    style.spacing.text_edit_width = 240.0;
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.tooltip_width = 380.0;
    style.spacing.menu_width = 200.0;
    style.spacing.scroll = ScrollStyle::thin();
    style.interaction.selectable_labels = false;
    style.interaction.tooltip_delay = 0.35;
    style.animation_time = 0.12;
    style.text_styles = [
        (TextStyle::Heading, FontId::proportional(22.0)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(14.0)),
        (TextStyle::Small, FontId::proportional(12.0)),
        (TextStyle::Monospace, FontId::monospace(12.5)),
    ]
    .into_iter()
    .collect();

    let v = &mut style.visuals;
    v.window_fill = p.elevated;
    v.panel_fill = p.bg;
    v.faint_bg_color = p.surface;
    v.extreme_bg_color = p.input;
    v.text_edit_bg_color = Some(p.input);
    v.code_bg_color = p.control;
    v.hyperlink_color = p.accent;
    v.weak_text_color = Some(p.text_weak);
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, p.stroke);
    v.window_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(if p.dark { 90 } else { 30 }) };
    v.window_highlight_topmost = false;
    v.slider_trailing_fill = true;
    v.handle_shape = HandleShape::Circle;
    v.striped = false;
    v.image_loading_spinners = false;
    v.button_frame = true;
    v.interact_cursor = Some(CursorIcon::PointingHand);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    let radius = CornerRadius::same(6);
    v.widgets.noninteractive = WidgetVisuals { weak_bg_fill: p.bg, bg_fill: p.bg, bg_stroke: Stroke::new(1.0, p.stroke), fg_stroke: Stroke::new(1.0, p.text), corner_radius: radius, expansion: 0.0 };
    v.widgets.inactive = WidgetVisuals { weak_bg_fill: p.control, bg_fill: p.control, bg_stroke: Stroke::new(1.0, p.stroke), fg_stroke: Stroke::new(1.0, p.text), corner_radius: radius, expansion: 0.0 };
    v.widgets.hovered = WidgetVisuals { weak_bg_fill: p.control_hover, bg_fill: p.control_hover, bg_stroke: Stroke::new(1.0, p.stroke_strong), fg_stroke: Stroke::new(1.5, p.text_strong), corner_radius: radius, expansion: 0.0 };
    v.widgets.active = WidgetVisuals { weak_bg_fill: p.control_active, bg_fill: p.control_active, bg_stroke: Stroke::new(1.0, p.accent), fg_stroke: Stroke::new(2.0, p.text_strong), corner_radius: radius, expansion: 0.0 };
    v.widgets.open = WidgetVisuals { weak_bg_fill: p.control_hover, bg_fill: p.elevated, bg_stroke: Stroke::new(1.0, p.stroke_strong), fg_stroke: Stroke::new(1.0, p.text), corner_radius: radius, expansion: 0.0 };
}

pub fn card(ui: &egui::Ui) -> Frame {
    let p = palette(ui);
    Frame::new().fill(p.surface).stroke(Stroke::new(1.0, p.stroke)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(18))
}

pub fn card_compact(ui: &egui::Ui) -> Frame {
    card(ui).inner_margin(Margin::symmetric(16, 12))
}

pub fn popover_frame(ui: &egui::Ui) -> Frame {
    let p = palette(ui);
    Frame::new().fill(p.elevated).stroke(Stroke::new(1.0, p.stroke_strong)).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(14)).shadow(ui.visuals().popup_shadow)
}

pub fn dialog_frame(ctx: &egui::Context) -> Frame {
    let p = palette_of(ctx);
    Frame::new()
        .fill(p.elevated)
        .stroke(Stroke::new(1.0, p.stroke_strong))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(22))
        .shadow(Shadow { offset: [0, 12], blur: 36, spread: 0, color: Color32::from_black_alpha(if p.dark { 140 } else { 50 }) })
}

/// Page title on the left, `right` laid out right-to-left on the same row.
pub fn page_header(ui: &mut egui::Ui, title: &str, subtitle: Option<&str>, right: impl FnOnce(&mut egui::Ui)) {
    let p = palette(ui);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(title).text_style(TextStyle::Heading).strong().color(p.text_strong));
            if let Some(s) = subtitle {
                ui.label(RichText::new(s).small().color(p.text_weak));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
}

pub fn section_title(ui: &mut egui::Ui, title: &str) {
    let p = palette(ui);
    ui.label(RichText::new(title).size(15.0).strong().color(p.text_strong));
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    let p = palette(ui);
    ui.label(RichText::new(text).small().color(p.text_weak));
}

pub fn weak(ui: &mut egui::Ui, text: &str) {
    let p = palette(ui);
    ui.label(RichText::new(text).color(p.text_weak));
}

pub struct Filled {
    text: String,
    min_size: egui::Vec2,
    danger: bool,
    small: bool,
}

impl Filled {
    pub fn min_size(mut self, size: egui::Vec2) -> Filled {
        self.min_size = size;
        self
    }

    pub fn small(mut self) -> Filled {
        self.small = true;
        self.min_size = egui::vec2(0.0, 28.0);
        self
    }
}

impl egui::Widget for Filled {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let p = palette(ui);
        let font = FontId::proportional(if self.small { 13.0 } else { 14.0 });
        let galley = ui.painter().layout_no_wrap(self.text, font, p.on_accent);
        let pad = if self.small { egui::vec2(12.0, 5.0) } else { egui::vec2(16.0, 7.0) };
        let size = (galley.size() + pad * 2.0).max(self.min_size);
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if ui.is_rect_visible(rect) {
            let base = if self.danger { p.danger } else { p.accent };
            let hover = if self.danger { p.danger.lerp_to_gamma(Color32::WHITE, 0.12) } else { p.accent_hover };
            let fill = if response.is_pointer_button_down_on() {
                base.lerp_to_gamma(Color32::BLACK, 0.15)
            } else if response.hovered() {
                hover
            } else {
                base
            };
            ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.on_accent);
        }
        response.on_hover_cursor(CursorIcon::PointingHand)
    }
}

pub fn primary(text: &str) -> Filled {
    Filled { text: text.to_owned(), min_size: egui::vec2(0.0, 32.0), danger: false, small: false }
}

pub fn danger(text: &str) -> Filled {
    Filled { text: text.to_owned(), min_size: egui::vec2(0.0, 32.0), danger: true, small: false }
}

pub fn secondary_button(text: &str) -> Button<'static> {
    Button::new(RichText::new(text)).corner_radius(CornerRadius::same(8)).min_size(egui::vec2(0.0, 32.0))
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
pub fn kind_color(kind: Kind, dark: bool) -> Color32 {
    if dark {
        match kind {
            Kind::Video => Color32::from_rgb(52, 76, 118),
            Kind::Gif => Color32::from_rgb(96, 64, 120),
            Kind::Picture => Color32::from_rgb(54, 102, 86),
            Kind::VideoStream => Color32::from_rgb(118, 80, 54),
            Kind::Web | Kind::Url => Color32::from_rgb(46, 94, 116),
            Kind::WebAudio => Color32::from_rgb(108, 56, 94),
            Kind::Program => Color32::from_rgb(72, 74, 86),
        }
    } else {
        match kind {
            Kind::Video => Color32::from_rgb(188, 204, 236),
            Kind::Gif => Color32::from_rgb(214, 196, 232),
            Kind::Picture => Color32::from_rgb(190, 222, 206),
            Kind::VideoStream => Color32::from_rgb(236, 210, 188),
            Kind::Web | Kind::Url => Color32::from_rgb(186, 214, 228),
            Kind::WebAudio => Color32::from_rgb(230, 194, 220),
            Kind::Program => Color32::from_rgb(208, 210, 220),
        }
    }
}

pub fn kind_glyph_color(dark: bool) -> Color32 {
    if dark { Color32::from_white_alpha(210) } else { Color32::from_black_alpha(130) }
}

#[cfg(test)]
mod tests {
    use eframe::egui::{Color32, FontDefinitions, FontId};
    use eframe::epaint::text::{Fonts, TextOptions};

    // Compare atlas rectangles: has_glyph misreports bundled emoji glyphs.
    #[test]
    fn glyphs_exist_in_default_fonts() {
        let mut fonts = Fonts::new(TextOptions::default(), FontDefinitions::default());
        let mut view = fonts.with_pixels_per_point(1.0);
        let font = FontId::proportional(14.0);
        let uv_of = |view: &mut eframe::epaint::text::FontsView<'_>, c: char| view.layout_no_wrap(c.to_string(), font.clone(), Color32::WHITE).rows[0].row.glyphs[0].uv_rect;
        let replacement = uv_of(&mut view, '\u{25FB}');
        let glyphs = "🖼🖥⚙ℹ🔍✖▶⏸⏮↻🔀🎬🎞📡🌐🎵📥⏳✔⚠🔇🔉🔊·…+×";
        let missing: Vec<char> = glyphs.chars().filter(|c| uv_of(&mut view, *c) == replacement).collect();
        assert!(missing.is_empty(), "glyphs without a font: {missing:?}");
        // A fullwidth plus is in none of the bundled fonts: the check must notice.
        assert_eq!(uv_of(&mut view, '\u{FF0B}'), replacement, "missing glyphs must map to the replacement square");
    }
}
