//! Teaching-aid rendering: translucent acrylic look with engraved scales.

use crate::icons::{self, Icon};
use crate::paint::{self, fill_circle, line, paint as mkpaint, polygon_path, stroke_circle};
use crate::text::{Fonts, TextStyle};
use cb_core::assist::{AssistKind, AssistTool, PX_PER_CM};
use cb_core::geom::{v2, Vec2};
use cb_core::Color;
use tiny_skia::{FillRule, PathBuilder, PixmapMut, Transform};

pub struct AssistColors {
    pub body: Color,
    pub edge: Color,
    pub tick: Color,
    pub label: Color,
    pub handle: Color,
    pub handle_icon: Color,
    pub highlight: Color,
}

impl AssistColors {
    pub fn new(accent: Color) -> AssistColors {
        AssistColors {
            body: Color::rgba(186, 222, 255, 92),
            edge: Color::rgba(40, 92, 160, 210),
            tick: Color::rgba(20, 52, 100, 230),
            label: Color::rgba(18, 42, 84, 240),
            handle: accent.with_alpha(235),
            handle_icon: Color::rgb(255, 255, 255),
            highlight: accent.with_alpha(200),
        }
    }
}

/// Screen mapping for board coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Mapper {
    pub zoom: f32,
    pub ox: f32,
    pub oy: f32,
}

impl Mapper {
    pub fn map(&self, p: Vec2) -> Vec2 {
        v2(p.x * self.zoom + self.ox, p.y * self.zoom + self.oy)
    }
}

fn fill_poly(pm: &mut PixmapMut, pts: &[Vec2], c: Color, hole: Option<&[Vec2]>) {
    let mut pb = PathBuilder::new();
    let mut push = |pts: &[Vec2]| {
        if let Some(f) = pts.first() {
            pb.move_to(f.x, f.y);
            for p in &pts[1..] {
                pb.line_to(p.x, p.y);
            }
            pb.close();
        }
    };
    push(pts);
    if let Some(h) = hole {
        if h.len() >= 3 {
            push(h);
        }
    }
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &mkpaint(c), FillRule::EvenOdd, Transform::identity(), None);
    }
}

fn stroke_poly(pm: &mut PixmapMut, pts: &[Vec2], w: f32, c: Color) {
    if let Some(p) = polygon_path(pts) {
        pm.stroke_path(&p, &mkpaint(c), &tiny_skia::Stroke { width: w, line_join: tiny_skia::LineJoin::Round, ..Default::default() }, Transform::identity(), None);
    }
}

fn handle(pm: &mut PixmapMut, at: Vec2, r: f32, icon: Icon, cols: &AssistColors) {
    fill_circle(pm, at + v2(0.0, r * 0.12), r * 1.05, Color::rgba(0, 0, 0, 40));
    fill_circle(pm, at, r, cols.handle);
    stroke_circle(pm, at, r, (r * 0.08).max(1.0), Color::rgba(255, 255, 255, 180));
    icons::draw(pm, icon, at.x, at.y, r * 1.15, cols.handle_icon);
}

/// Draw one tool. `ui` is the UI scale (for text/handle sizes); `active_edge`
/// highlights the edge currently used for ruling.
pub fn draw_tool(pm: &mut PixmapMut, fonts: &mut Fonts, t: &AssistTool, m: Mapper, ui: f32, cols: &AssistColors, active_edge: Option<usize>, readout: Option<&str>) {
    let z = m.zoom;
    // Numbers are engraved on the tool: they scale with it (and the zoom).
    let label_px = (t.label_size() * z).max(6.0);
    let hr = (t.handle_radius() * z).max(13.0 * ui);
    if t.kind == AssistKind::Compass {
        draw_compass(pm, fonts, t, m, ui, cols, readout);
        return;
    }
    let outline: Vec<Vec2> = t.outline().into_iter().map(|p| m.map(p)).collect();
    let hole: Vec<Vec2> = t.hole().into_iter().map(|p| m.map(p)).collect();
    // Shadow, body, bevel.
    let sh: Vec<Vec2> = outline.iter().map(|p| *p + v2(0.0, 3.0 * ui)).collect();
    let shh: Vec<Vec2> = hole.iter().map(|p| *p + v2(0.0, 3.0 * ui)).collect();
    fill_poly(pm, &sh, Color::rgba(0, 0, 0, 28), Some(&shh));
    fill_poly(pm, &outline, cols.body, Some(&hole));
    stroke_poly(pm, &outline, (1.6 * ui).max(1.0), cols.edge);
    if hole.len() >= 3 {
        stroke_poly(pm, &hole, (1.2 * ui).max(1.0), cols.edge.with_alpha(150));
    }
    if let Some(i) = active_edge {
        if let Some(e) = t.edges().get(i) {
            line(pm, m.map(e.a), m.map(e.b), (3.0 * ui).max(2.0), cols.highlight);
        }
    }
    // Scale.
    let tw = (1.0 * ui).max(0.8);
    for tk in t.ticks() {
        line(pm, m.map(tk.a), m.map(tk.b), if tk.major { tw * 1.3 } else { tw }, cols.tick);
        if let Some((s, p)) = &tk.label {
            let q = m.map(*p);
            fonts.draw_vcenter(pm, s, q.x, q.y, TextStyle::new(label_px, cols.label).weight(500).center());
        }
    }
    match t.kind {
        AssistKind::Ruler => {
            let q = m.map(t.to_world(v2(PX_PER_CM * t.scale * 1.0, PX_PER_CM * t.scale * 2.2)));
            fonts.draw_vcenter(pm, "cm", q.x, q.y, TextStyle::new(label_px * 0.9, cols.label).center());
        }
        AssistKind::Triangle45 | AssistKind::Triangle30 => {
            for (p, s) in t.angle_labels() {
                let q = m.map(p);
                let px = label_px * AssistTool::ANGLE_LABEL_CM / AssistTool::LABEL_CM;
                fonts.draw_vcenter(pm, &s, q.x, q.y, TextStyle::new(px, cols.label).bold().center());
            }
        }
        AssistKind::Protractor => {
            let c = m.map(t.pos);
            fill_circle(pm, c, 3.0 * ui, cols.tick);
            let tip = m.map(t.protractor_pointer_tip());
            line(pm, c, tip, (2.0 * ui).max(1.5), cols.highlight);
            fill_circle(pm, tip, 5.0 * ui, cols.highlight);
            let deg = format!("{:.0}°", t.reading_deg());
            let lp = m.map(t.to_world(v2(0.0, -PX_PER_CM * t.scale * 1.6)));
            fonts.draw_vcenter(pm, &deg, lp.x, lp.y, TextStyle::new(label_px * 1.5, cols.label).bold().center());
        }
        AssistKind::Compass => {}
    }
    handle(pm, m.map(t.rotate_handle()), hr, Icon::Rotate, cols);
    let cb = m.map(t.close_button());
    fill_circle(pm, cb, hr * 0.78, Color::rgba(255, 255, 255, 220));
    stroke_circle(pm, cb, hr * 0.78, 1.0, cols.edge);
    icons::draw(pm, Icon::Close, cb.x, cb.y, hr * 0.9, cols.edge);
    if let Some(s) = readout {
        draw_readout(pm, fonts, m.map(t.rotate_handle()) + v2(0.0, -hr * 2.2), s, ui);
    }
}

pub fn draw_readout(pm: &mut PixmapMut, fonts: &mut Fonts, at: Vec2, s: &str, ui: f32) {
    let st = TextStyle::new(13.0 * ui, Color::rgb(255, 255, 255)).weight(600).center();
    let (w, h) = fonts.measure(s, st);
    let r = cb_core::Rect::xywh(at.x - w * 0.5 - 8.0 * ui, at.y - h * 0.5 - 4.0 * ui, w + 16.0 * ui, h + 8.0 * ui);
    paint::fill_rrect(pm, r, 8.0 * ui, Color::rgba(20, 24, 34, 220));
    fonts.draw_vcenter(pm, s, at.x, at.y, st);
}

fn draw_compass(pm: &mut PixmapMut, fonts: &mut Fonts, t: &AssistTool, m: Mapper, ui: f32, cols: &AssistColors, readout: Option<&str>) {
    let z = m.zoom;
    let needle = m.map(t.compass_needle());
    let pencil = m.map(t.compass_pencil());
    let hinge = m.map(t.compass_hinge());
    let handle_p = m.map(t.compass_handle());
    let hr = (t.handle_radius() * z).max(13.0 * ui);
    // Ghost circle.
    let r = t.radius * z;
    let mut pb = PathBuilder::new();
    pb.push_circle(needle.x, needle.y, r);
    if let Some(p) = pb.finish() {
        let st = tiny_skia::Stroke { width: 1.0 * ui, dash: tiny_skia::StrokeDash::new(vec![6.0 * ui, 5.0 * ui], 0.0), ..Default::default() };
        pm.stroke_path(&p, &mkpaint(cols.edge.with_alpha(110)), &st, Transform::identity(), None);
    }
    // Radius line.
    paint::dashed_line(pm, needle, pencil, 1.0 * ui, 4.0 * ui, cols.edge.with_alpha(160));
    let leg_w = (0.42 * PX_PER_CM * z).max(6.0 * ui);
    let metal = Color::rgb(128, 138, 152);
    let metal_dark = Color::rgb(70, 78, 92);
    // Legs.
    line(pm, hinge, needle, leg_w + 2.0, metal_dark);
    line(pm, hinge, needle, leg_w, metal);
    line(pm, hinge, pencil, leg_w + 2.0, metal_dark);
    line(pm, hinge, pencil, leg_w, Color::rgb(232, 184, 72));
    // Needle tip and pencil lead.
    let nd = (needle - hinge).normalized();
    line(pm, needle - nd * (leg_w * 1.6), needle, (leg_w * 0.35).max(2.0), metal_dark);
    let pd = (pencil - hinge).normalized();
    line(pm, pencil - pd * (leg_w * 1.8), pencil - pd * (leg_w * 0.6), leg_w * 0.9, Color::rgb(238, 214, 170));
    line(pm, pencil - pd * (leg_w * 0.7), pencil, (leg_w * 0.35).max(2.0), Color::rgb(40, 40, 46));
    // Hinge + handle.
    line(pm, hinge, handle_p, leg_w * 0.6, metal_dark);
    fill_circle(pm, hinge, leg_w * 0.9, metal_dark);
    fill_circle(pm, hinge, leg_w * 0.55, Color::rgb(210, 214, 222));
    handle(pm, handle_p, hr, Icon::Rotate, cols);
    // Grab rings for needle/pencil.
    stroke_circle(pm, needle, hr * 0.9, 1.2 * ui, cols.handle.with_alpha(140));
    stroke_circle(pm, pencil, hr * 0.9, 1.2 * ui, cols.handle.with_alpha(140));
    let cb = m.map(t.close_button());
    fill_circle(pm, cb, hr * 0.78, Color::rgba(255, 255, 255, 220));
    stroke_circle(pm, cb, hr * 0.78, 1.0, cols.edge);
    icons::draw(pm, Icon::Close, cb.x, cb.y, hr * 0.9, cols.edge);
    let label = format!("r = {:.1} cm", t.radius / PX_PER_CM);
    let mid = needle.lerp(pencil, 0.5) + (pencil - needle).normalized().perp() * (14.0 * ui);
    fonts.draw_vcenter(pm, &label, mid.x, mid.y, TextStyle::new(12.0 * ui, cols.label).weight(600).center());
    if let Some(s) = readout {
        draw_readout(pm, fonts, handle_p + v2(0.0, -hr * 2.2), s, ui);
    }
}
