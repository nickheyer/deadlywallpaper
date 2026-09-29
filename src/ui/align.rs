//! Interactive alignment of a spanning wallpaper and its displays.

use crate::geom::Rect as GeomRect;
use crate::model::{Display, Kind, Layout, Pose};
use crate::ui::{theme, widgets};
use eframe::egui::load::{SizeHint, TexturePoll};
use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureOptions, Vec2};
use eframe::epaint::{Mesh, Vertex, WHITE_UV};

pub struct Scene<'a> {
    pub displays: &'a [Display],
    pub layout: &'a Layout,
    /// Thumbnail of the shared wallpaper, drawn as the image.
    pub thumbnail: Option<&'a str>,
    pub kind: Option<Kind>,
    pub selected: Option<&'a str>,
    /// Whether the running wallpaper can be scaled or turned on this desktop.
    pub scalable: bool,
    pub rotatable: bool,
}

pub enum Event {
    Clicked(String),
    Image(Pose),
    Display { id: String, pose: Pose },
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Target {
    Image,
    Display(usize),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Move,
    Scale,
    Rotate,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    target: Target,
    mode: Mode,
    /// Where the pointer went down, in screen pixels.
    start: Pos2,
    /// The target's pose when the drag began.
    pose: Pose,
}

/// A rectangle placed in span space: its base under a pose.
#[derive(Clone, Copy, Debug)]
struct Placed {
    cx: f64,
    cy: f64,
    w: f64,
    h: f64,
    scale: f64,
    rotation: f64,
}

impl Placed {
    fn new(base: GeomRect, pose: Pose) -> Placed {
        Placed { cx: base.x as f64 + base.w as f64 / 2.0 + pose.x, cy: base.y as f64 + base.h as f64 / 2.0 + pose.y, w: base.w as f64, h: base.h as f64, scale: pose.scale, rotation: pose.rotation }
    }

    /// Local (centred, unscaled, unturned) to span space.
    fn out(&self, lx: f64, ly: f64) -> (f64, f64) {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let (x, y) = (lx * self.scale, ly * self.scale);
        (self.cx + x * c - y * s, self.cy + x * s + y * c)
    }

    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        let (dx, dy) = (x - self.cx, y - self.cy);
        let (s, c) = (-self.rotation).to_radians().sin_cos();
        ((dx * c - dy * s) / self.scale, (dx * s + dy * c) / self.scale)
    }

    /// Corners clockwise from the top-left.
    fn corners(&self) -> [(f64, f64); 4] {
        let (hw, hh) = (self.w / 2.0, self.h / 2.0);
        [self.out(-hw, -hh), self.out(hw, -hh), self.out(hw, hh), self.out(-hw, hh)]
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        let (lx, ly) = self.local(x, y);
        lx.abs() <= self.w / 2.0 && ly.abs() <= self.h / 2.0
    }

    fn plain(&self) -> bool {
        self.scale == 1.0 && self.rotation == 0.0
    }
}

const HANDLE: f32 = 8.0;
const ROTATE_ARM: f32 = 28.0;
/// Snap distance in screen pixels.
const SNAP_PX: f64 = 12.0;

pub fn editor(ui: &mut egui::Ui, scene: &Scene<'_>, max_size: Vec2) -> Option<Event> {
    let p = theme::palette(ui);
    if scene.displays.is_empty() {
        theme::weak(ui, "No displays");
        return None;
    }
    let id = ui.id().with("span-editor");
    let drag: Option<Drag> = ui.data(|m| m.get_temp(id.with("drag")));
    let image_selected: bool = ui.data(|m| m.get_temp(id.with("image"))).unwrap_or(false);
    let bounds = Layout::span_bounds(scene.displays);

    // Committed placements set the scale of the scene, so nothing jumps while dragging.
    let image_home = Placed::new(bounds, scene.layout.image);
    let homes: Vec<Placed> = scene.displays.iter().map(|d| Placed::new(d.rect, scene.layout.pose(&d.id))).collect();
    let (mut min, mut max) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for (x, y) in homes.iter().chain(std::iter::once(&image_home)).flat_map(Placed::corners) {
        min = (min.0.min(x), min.1.min(y));
        max = (max.0.max(x), max.1.max(y));
    }
    let margin = 44.0_f32;
    let span_w = (max.0 - min.0).max(1.0);
    let span_h = (max.1 - min.1).max(1.0);
    let scale = (((max_size.x - 2.0 * margin) as f64) / span_w).min(((max_size.y - 2.0 * margin) as f64) / span_h);
    let size = egui::vec2((span_w * scale) as f32 + 2.0 * margin, (span_h * scale) as f32 + 2.0 * margin);
    let (area, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let origin = area.min + egui::vec2(margin, margin);
    let to_screen = move |x: f64, y: f64| Pos2::new(origin.x + ((x - min.0) * scale) as f32, origin.y + ((y - min.1) * scale) as f32);
    let to_span = move |pos: Pos2| (min.0 + (pos.x - origin.x) as f64 / scale, min.1 + (pos.y - origin.y) as f64 / scale);
    let pointer = resp.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));

    // Poses as shown this frame: the committed ones, with the dragged one following the pointer.
    let live = |drag: &Drag, pointer: Pos2| -> Pose {
        let home = match drag.target {
            Target::Image => image_home,
            Target::Display(i) => homes[i],
        };
        let center = to_screen(home.cx, home.cy);
        match drag.mode {
            Mode::Move => {
                let shift = (drag.pose.x + (pointer.x - drag.start.x) as f64 / scale, drag.pose.y + (pointer.y - drag.start.y) as f64 / scale);
                let (x, y) = snap_shift(drag.target, shift, scene, &homes, SNAP_PX / scale);
                Pose { x, y, ..drag.pose }
            }
            Mode::Scale => {
                let from = drag.start.distance(center).max(1.0);
                let mut k = drag.pose.scale * (pointer.distance(center) / from) as f64;
                if (k - 1.0).abs() < 0.04 {
                    k = 1.0;
                }
                Pose { scale: k, ..drag.pose }.normalized()
            }
            Mode::Rotate => {
                let angle = |q: Pos2| (q.y - center.y).atan2(q.x - center.x) as f64;
                let mut r = drag.pose.rotation + (angle(pointer) - angle(drag.start)).to_degrees();
                let step = (r / 15.0).round() * 15.0;
                if (r - step).abs() < 3.0 {
                    r = step;
                }
                Pose { rotation: r, ..drag.pose }.normalized()
            }
        }
    };
    let mut image_pose = scene.layout.image;
    let mut poses: Vec<Pose> = scene.displays.iter().map(|d| scene.layout.pose(&d.id)).collect();
    if let (Some(d), Some(pos)) = (drag, pointer) {
        let pose = live(&d, pos);
        match d.target {
            Target::Image => image_pose = pose,
            Target::Display(i) => poses[i] = pose,
        }
        ui.ctx().request_repaint();
    }
    let image = Placed::new(bounds, image_pose);
    let placed: Vec<Placed> = scene.displays.iter().zip(&poses).map(|(d, pose)| Placed::new(d.rect, *pose)).collect();
    let selected_target = if image_selected { Some(Target::Image) } else { scene.selected.and_then(|id| scene.displays.iter().position(|d| d.id == id)).map(Target::Display) };

    let handle_points = |placed: &Placed| -> ([Pos2; 4], Pos2) {
        let corners = placed.corners().map(|(x, y)| to_screen(x, y));
        let (tx, ty) = placed.out(0.0, -placed.h / 2.0);
        let top = to_screen(tx, ty);
        let center = to_screen(placed.cx, placed.cy);
        let dir = top - center;
        let dir = if dir.length() > 0.0 { dir.normalized() } else { egui::vec2(0.0, -1.0) };
        (corners, top + dir * ROTATE_ARM)
    };
    let hit = |pos: Pos2| -> Option<(Target, Mode)> {
        if let Some(t) = selected_target {
            let home = match t {
                Target::Image => image_home,
                Target::Display(i) => homes[i],
            };
            let (corners, rotate) = handle_points(&home);
            if scene.rotatable && pos.distance(rotate) <= HANDLE + 4.0 {
                return Some((t, Mode::Rotate));
            }
            if scene.scalable && corners.iter().any(|c| pos.distance(*c) <= HANDLE + 4.0) {
                return Some((t, Mode::Scale));
            }
        }
        let (x, y) = to_span(pos);
        for (i, d) in homes.iter().enumerate().rev() {
            if d.contains(x, y) {
                return Some((Target::Display(i), Mode::Move));
            }
        }
        if image_home.contains(x, y) {
            return Some((Target::Image, Mode::Move));
        }
        None
    };

    let mut event = None;
    if resp.drag_started() {
        if let Some(start) = ui.input(|i| i.pointer.press_origin()).or(pointer) {
            if let Some((target, mode)) = hit(start) {
                let pose = match target {
                    Target::Image => scene.layout.image,
                    Target::Display(i) => scene.layout.pose(&scene.displays[i].id),
                };
                ui.data_mut(|m| m.insert_temp(id.with("drag"), Drag { target, mode, start, pose }));
                ui.data_mut(|m| m.insert_temp(id.with("image"), target == Target::Image));
                if let Target::Display(i) = target {
                    event = Some(Event::Clicked(scene.displays[i].id.clone()));
                }
            }
        }
    } else if resp.drag_stopped() {
        if let (Some(d), Some(pos)) = (drag, pointer) {
            ui.data_mut(|m| m.remove::<Drag>(id.with("drag")));
            let pose = live(&d, pos);
            let committed = match d.target {
                Target::Image => scene.layout.image,
                Target::Display(i) => scene.layout.pose(&scene.displays[i].id),
            };
            if pose != committed {
                event = Some(match d.target {
                    Target::Image => Event::Image(pose),
                    Target::Display(i) => Event::Display { id: scene.displays[i].id.clone(), pose },
                });
            }
        }
    } else if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            match hit(pos) {
                Some((Target::Display(i), _)) => {
                    ui.data_mut(|m| m.insert_temp(id.with("image"), false));
                    event = Some(Event::Clicked(scene.displays[i].id.clone()));
                }
                Some((Target::Image, _)) => {
                    ui.data_mut(|m| m.insert_temp(id.with("image"), true));
                }
                None => {}
            }
        }
    }

    let painter = ui.painter();
    let texture = scene.thumbnail.and_then(|uri| match ui.ctx().try_load_texture(uri, TextureOptions::LINEAR, SizeHint::default()) {
        Ok(TexturePoll::Ready { texture }) => Some(texture),
        _ => None,
    });
    let uv_rect = texture.map(|t| widgets::cover_uv(t.size, egui::vec2(bounds.w as f32, bounds.h as f32))).unwrap_or(Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));
    let fill = theme::kind_color(scene.kind.unwrap_or(Kind::Picture), p.dark);
    let mesh_of = |points: &[(f64, f64)], brightness: f32| -> Mesh {
        let mut mesh = match texture {
            Some(t) => Mesh::with_texture(t.id),
            None => Mesh::default(),
        };
        for &(x, y) in points {
            let (uv, color) = match texture {
                Some(_) => {
                    let (lx, ly) = image.local(x, y);
                    let u = (lx / image.w + 0.5) as f32;
                    let v = (ly / image.h + 0.5) as f32;
                    (Pos2::new(uv_rect.min.x + u * uv_rect.width(), uv_rect.min.y + v * uv_rect.height()), Color32::from_gray((brightness * 255.0) as u8))
                }
                None => (WHITE_UV, dim(fill, brightness)),
            };
            mesh.vertices.push(Vertex { pos: to_screen(x, y), uv, color });
        }
        for i in 1..points.len().saturating_sub(1) {
            mesh.add_triangle(0, i as u32, i as u32 + 1);
        }
        mesh
    };
    let screen_poly = |placed: &Placed| -> Vec<Pos2> { placed.corners().iter().map(|&(x, y)| to_screen(x, y)).collect() };

    // Blank screens under everything, the whole image dimmed, then each screen's part bright.
    let blank = if p.dark { Color32::from_rgb(12, 12, 14) } else { Color32::from_rgb(210, 212, 220) };
    for d in &placed {
        painter.add(Shape::convex_polygon(screen_poly(d), blank, Stroke::NONE));
    }
    let image_corners = image.corners();
    painter.add(Shape::mesh(mesh_of(&image_corners, 0.35)));
    for d in &placed {
        let part = clip_convex(&image_corners, &d.corners());
        if part.len() >= 3 {
            painter.add(Shape::mesh(mesh_of(&part, 1.0)));
        }
    }
    if texture.is_none() {
        painter.text(to_screen(image.cx, image.cy), Align2::CENTER_CENTER, theme::kind_glyph(scene.kind.unwrap_or(Kind::Picture)), FontId::proportional(28.0), theme::kind_glyph_color(p.dark));
    }

    let hover = if drag.is_some() { None } else { pointer.filter(|pos| area.contains(*pos)).and_then(|pos| hit(pos)) };
    let image_stroke = if selected_target == Some(Target::Image) { Stroke::new(2.0, p.accent) } else { Stroke::new(1.0, p.stroke_strong) };
    painter.add(Shape::closed_line(screen_poly(&image), image_stroke));
    for (i, d) in placed.iter().enumerate() {
        let is_selected = selected_target == Some(Target::Display(i));
        let hovered = matches!(hover, Some((Target::Display(j), Mode::Move)) if j == i);
        let stroke = if is_selected {
            Stroke::new(2.5, p.accent)
        } else if hovered {
            Stroke::new(1.5, p.text_weak)
        } else {
            Stroke::new(1.5, p.stroke_strong)
        };
        painter.add(Shape::closed_line(screen_poly(d), stroke));
        let inset = 16.0 / scale / d.scale;
        let (bx, by) = d.out(-d.w / 2.0 + inset, -d.h / 2.0 + inset);
        widgets::badge(painter, to_screen(bx, by), Align2::CENTER_CENTER, &format!("{}", i + 1), if is_selected { p.accent } else { Color32::from_black_alpha(170) }, Color32::WHITE);
    }

    if let Some(t) = selected_target {
        let target = match t {
            Target::Image => image,
            Target::Display(i) => placed[i],
        };
        let (corners, rotate) = handle_points(&target);
        if scene.scalable {
            for c in corners {
                painter.rect(Rect::from_center_size(c, egui::vec2(HANDLE, HANDLE)), 2.0, p.elevated, Stroke::new(1.5, p.accent), StrokeKind::Inside);
            }
        }
        if scene.rotatable {
            let (tx, ty) = target.out(0.0, -target.h / 2.0);
            painter.line_segment([to_screen(tx, ty), rotate], Stroke::new(1.0, p.accent));
            painter.circle(rotate, HANDLE * 0.7, p.elevated, Stroke::new(1.5, p.accent));
        }
    }

    let cursor = match (drag, hover) {
        (Some(d), _) => match d.mode {
            Mode::Scale => CursorIcon::ResizeNwSe,
            Mode::Move | Mode::Rotate => CursorIcon::Grabbing,
        },
        (None, Some((_, Mode::Move))) => CursorIcon::Move,
        (None, Some((_, Mode::Scale))) => CursorIcon::ResizeNwSe,
        (None, Some((_, Mode::Rotate))) => CursorIcon::Grab,
        (None, None) => CursorIcon::Default,
    };
    if cursor != CursorIcon::Default {
        ui.ctx().set_cursor_icon(cursor);
    }
    match hover {
        Some((Target::Display(i), Mode::Move)) => {
            let d = &scene.displays[i];
            let mut tip = format!("{}{}\n{}×{}", d.name, if d.primary { " (primary)" } else { "" }, d.rect.w, d.rect.h);
            if !poses[i].is_identity() {
                tip.push_str(&format!("\n{}", describe(&poses[i])));
            }
            resp.clone().on_hover_text(tip);
        }
        Some((Target::Image, Mode::Move)) if !image_pose.is_identity() => {
            resp.clone().on_hover_text(describe(&image_pose));
        }
        _ => {}
    }
    event
}

fn describe(pose: &Pose) -> String {
    format!("{:+.0}, {:+.0} · ×{:.2} · {:.1}°", pose.x, pose.y, pose.scale, pose.rotation)
}

fn dim(color: Color32, factor: f32) -> Color32 {
    let f = |c: u8| (c as f32 * factor).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(color.r()), f(color.g()), f(color.b()))
}

/// Snap a moving target's shift: back to its own place, and a plain display onto the edges of
/// other plain displays.
fn snap_shift(target: Target, shift: (f64, f64), scene: &Scene<'_>, homes: &[Placed], threshold: f64) -> (f64, f64) {
    let near = |a: f64, b: f64| (a - b).abs() <= threshold;
    let (x, y) = shift;
    if let Target::Display(i) = target {
        if homes[i].plain() {
            let base = scene.displays[i].rect;
            let (wx, wy, w, h) = (base.x as f64 + x, base.y as f64 + y, base.w as f64, base.h as f64);
            let mut xs = vec![base.x as f64];
            let mut ys = vec![base.y as f64];
            for (j, o) in homes.iter().enumerate() {
                if j == i || !o.plain() {
                    continue;
                }
                let (l, t, r, b) = (o.cx - o.w / 2.0, o.cy - o.h / 2.0, o.cx + o.w / 2.0, o.cy + o.h / 2.0);
                xs.extend([l, r, l - w, r - w]);
                ys.extend([t, b, t - h, b - h]);
            }
            let nearest = |v: f64, candidates: &[f64]| candidates.iter().copied().filter(|c| near(*c, v)).min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs())).unwrap_or(v);
            return (nearest(wx, &xs) - base.x as f64, nearest(wy, &ys) - base.y as f64);
        }
    }
    if near(x, 0.0) && near(y, 0.0) { (0.0, 0.0) } else { (x, y) }
}

/// The part of convex `subject` inside convex `clip` (Sutherland–Hodgman), in either winding.
fn clip_convex(subject: &[(f64, f64)], clip: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let orientation = signed_area(clip).signum();
    let mut out: Vec<(f64, f64)> = subject.to_vec();
    for i in 0..clip.len() {
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let input = std::mem::take(&mut out);
        if input.is_empty() {
            break;
        }
        let side = |q: (f64, f64)| ((b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0)) * orientation;
        for j in 0..input.len() {
            let cur = input[j];
            let prev = input[(j + input.len() - 1) % input.len()];
            let (sc, sp) = (side(cur), side(prev));
            if sc >= 0.0 {
                if sp < 0.0 {
                    out.push(crossing(prev, cur, sp, sc));
                }
                out.push(cur);
            } else if sp >= 0.0 {
                out.push(crossing(prev, cur, sp, sc));
            }
        }
    }
    out
}

fn crossing(p: (f64, f64), q: (f64, f64), sp: f64, sq: f64) -> (f64, f64) {
    let t = sp / (sp - sq);
    (p.0 + t * (q.0 - p.0), p.1 + t * (q.1 - p.1))
}

fn signed_area(poly: &[(f64, f64)]) -> f64 {
    poly.iter()
        .enumerate()
        .map(|(i, &(x, y))| {
            let (nx, ny) = poly[(i + 1) % poly.len()];
            x * ny - nx * y
        })
        .sum::<f64>()
        / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convex_clip_keeps_the_overlap() {
        let a = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let b = [(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0)];
        let part = clip_convex(&a, &b);
        assert_eq!(part.len(), 4);
        assert!((signed_area(&part).abs() - 25.0).abs() < 1e-9);
        let far = [(20.0, 20.0), (30.0, 20.0), (30.0, 30.0), (20.0, 30.0)];
        assert!(clip_convex(&a, &far).is_empty());
        let reversed: Vec<_> = b.iter().rev().copied().collect();
        assert!((signed_area(&clip_convex(&a, &reversed)).abs() - 25.0).abs() < 1e-9, "winding must not matter");
        let inside = [(2.0, 2.0), (4.0, 2.0), (4.0, 4.0), (2.0, 4.0)];
        assert!((signed_area(&clip_convex(&inside, &a)).abs() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn placed_maps_both_ways() {
        let placed = Placed::new(GeomRect::new(100, 200, 1920, 1080), Pose { x: 10.0, y: -20.0, scale: 1.5, rotation: 30.0 });
        let (x, y) = placed.out(300.0, -100.0);
        let (lx, ly) = placed.local(x, y);
        assert!((lx - 300.0).abs() < 1e-9 && (ly + 100.0).abs() < 1e-9);
        assert!(placed.contains(placed.cx, placed.cy));
        assert!(!placed.contains(placed.cx + 5000.0, placed.cy));
        assert_eq!((placed.cx, placed.cy), (1070.0, 720.0));
    }
}
