//! Stroke rendering.

use crate::paint::paint;
use cb_core::doc::{LineStyle, ShapeKind, Stroke};
use cb_core::geom::Vec2;
use cb_core::ink;
use tiny_skia::{FillRule, LineCap, LineJoin, PathBuilder, PixmapMut, StrokeDash, Transform};

/// Highlighter strokes are drawn with this alpha.
pub const HIGHLIGHTER_ALPHA: u8 = 96;

fn has_pressure(s: &Stroke) -> bool {
    s.points.iter().any(|p| (p.pressure - 1.0).abs() > 0.02)
}

/// Draw a stroke with page→screen transform `t` (uniform scale assumed).
pub fn draw_stroke(pm: &mut PixmapMut, s: &Stroke, t: Transform, use_pressure: bool) {
    let mut col = s.color;
    if s.highlighter {
        col = col.with_alpha(HIGHLIGHTER_ALPHA);
    }
    let pt = paint(col);
    let dash = |w: f32| match s.style {
        LineStyle::Solid => None,
        LineStyle::Dashed => StrokeDash::new(vec![w * 3.0, w * 2.2], 0.0),
        LineStyle::Dotted => StrokeDash::new(vec![0.01, w * 2.0], 0.0),
    };
    let cap = if s.highlighter { LineCap::Square } else { LineCap::Round };
    let st = tiny_skia::Stroke {
        width: s.width.max(0.3),
        line_cap: if s.style == LineStyle::Dotted { LineCap::Round } else { cap },
        line_join: LineJoin::Round,
        dash: dash(s.width.max(0.3)),
        ..Default::default()
    };

    if let Some(kind) = s.shape {
        let mut pb = PathBuilder::new();
        let pts = cb_core::doc::shape_outline(kind, s.a, s.b);
        if let Some(f) = pts.first() {
            pb.move_to(f.x, f.y);
            for p in &pts[1..] {
                pb.line_to(p.x, p.y);
            }
            if matches!(kind, ShapeKind::Rect | ShapeKind::Triangle | ShapeKind::Ellipse) {
                pb.close();
            }
        }
        if kind == ShapeKind::Arrow {
            let d = (s.b - s.a).normalized();
            let head = (s.width * 4.0).max(12.0);
            let n = d.perp();
            let base = s.b - d * head;
            let l = base + n * head * 0.55;
            let r = base - n * head * 0.55;
            pb.move_to(l.x, l.y);
            pb.line_to(s.b.x, s.b.y);
            pb.line_to(r.x, r.y);
        }
        if let Some(p) = pb.finish() {
            pm.stroke_path(&p, &pt, &st, t, None);
        }
        return;
    }

    let n = s.points.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        let p = s.points[0];
        let r = ink::pressure_width(s.width, p.pressure, use_pressure) * 0.5;
        if let Some(c) = PathBuilder::from_circle(p.x, p.y, r.max(0.3)) {
            pm.fill_path(&c, &pt, FillRule::Winding, t, None);
        }
        return;
    }
    let smooth = ink::smooth(&s.points, s.width);
    if use_pressure && has_pressure(s) && s.style == LineStyle::Solid && !s.highlighter {
        // Variable width: filled outline plus round caps at ends.
        let outline = ink::variable_outline(&smooth, s.width, true);
        if let Some(path) = crate::paint::polygon_path(&outline) {
            pm.fill_path(&path, &pt, FillRule::Winding, t, None);
        }
        for p in [smooth[0], smooth[smooth.len() - 1]] {
            let r = ink::pressure_width(s.width, p.pressure, true) * 0.5;
            if let Some(c) = PathBuilder::from_circle(p.x, p.y, r) {
                pm.fill_path(&c, &pt, FillRule::Winding, t, None);
            }
        }
        return;
    }
    let mut pb = PathBuilder::new();
    pb.move_to(smooth[0].x, smooth[0].y);
    for p in &smooth[1..] {
        pb.line_to(p.x, p.y);
    }
    if let Some(p) = pb.finish() {
        pm.stroke_path(&p, &pt, &st, t, None);
    }
}

/// Draw a straight preview line (ruler / shape preview).
pub fn draw_segment(pm: &mut PixmapMut, a: Vec2, b: Vec2, width: f32, col: cb_core::Color, t: Transform) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    if let Some(p) = pb.finish() {
        let st = tiny_skia::Stroke { width, line_cap: LineCap::Round, ..Default::default() };
        pm.stroke_path(&p, &paint(col), &st, t, None);
    }
}
