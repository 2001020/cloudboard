//! Drawing helpers over tiny-skia.

use cb_core::geom::{Rect, Vec2};
use cb_core::Color;
use tiny_skia::{FillRule, GradientStop, LinearGradient, Paint, Path, PathBuilder, PixmapMut, Point, Shader, SpreadMode, Stroke, StrokeDash, Transform};

pub fn paint(c: Color) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, c.a);
    p.anti_alias = true;
    p
}

pub fn rounded_rect_path(r: Rect, radius: f32) -> Option<Path> {
    let rad = radius.min(r.w() * 0.5).min(r.h() * 0.5).max(0.0);
    let mut pb = PathBuilder::new();
    if rad <= 0.01 {
        pb.push_rect(tiny_skia::Rect::from_ltrb(r.x0, r.y0, r.x1, r.y1)?);
        return pb.finish();
    }
    let k = 0.5523 * rad;
    pb.move_to(r.x0 + rad, r.y0);
    pb.line_to(r.x1 - rad, r.y0);
    pb.cubic_to(r.x1 - rad + k, r.y0, r.x1, r.y0 + rad - k, r.x1, r.y0 + rad);
    pb.line_to(r.x1, r.y1 - rad);
    pb.cubic_to(r.x1, r.y1 - rad + k, r.x1 - rad + k, r.y1, r.x1 - rad, r.y1);
    pb.line_to(r.x0 + rad, r.y1);
    pb.cubic_to(r.x0 + rad - k, r.y1, r.x0, r.y1 - rad + k, r.x0, r.y1 - rad);
    pb.line_to(r.x0, r.y0 + rad);
    pb.cubic_to(r.x0, r.y0 + rad - k, r.x0 + rad - k, r.y0, r.x0 + rad, r.y0);
    pb.close();
    pb.finish()
}

pub fn fill_rect(pm: &mut PixmapMut, r: Rect, c: Color) {
    if let Some(rr) = tiny_skia::Rect::from_ltrb(r.x0, r.y0, r.x1, r.y1) {
        let mut p = paint(c);
        p.anti_alias = false;
        pm.fill_rect(rr, &p, Transform::identity(), None);
    }
}

pub fn fill_rrect(pm: &mut PixmapMut, r: Rect, radius: f32, c: Color) {
    if c.a == 0 {
        return;
    }
    if let Some(p) = rounded_rect_path(r, radius) {
        pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
    }
}

pub fn stroke_rrect(pm: &mut PixmapMut, r: Rect, radius: f32, width: f32, c: Color) {
    if let Some(p) = rounded_rect_path(r, radius) {
        let st = Stroke { width, ..Default::default() };
        pm.stroke_path(&p, &paint(c), &st, Transform::identity(), None);
    }
}

pub fn fill_rrect_vgradient(pm: &mut PixmapMut, r: Rect, radius: f32, top: Color, bottom: Color) {
    let Some(p) = rounded_rect_path(r, radius) else { return };
    let sh = LinearGradient::new(
        Point::from_xy(0.0, r.y0),
        Point::from_xy(0.0, r.y1),
        vec![GradientStop::new(0.0, tcolor(top)), GradientStop::new(1.0, tcolor(bottom))],
        SpreadMode::Pad,
        Transform::identity(),
    );
    let mut pt = Paint::default();
    pt.anti_alias = true;
    pt.shader = sh.unwrap_or(Shader::SolidColor(tcolor(top)));
    pm.fill_path(&p, &pt, FillRule::Winding, Transform::identity(), None);
}

pub fn tcolor(c: Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Soft drop shadow approximated with stacked translucent rounded rects.
pub fn shadow(pm: &mut PixmapMut, r: Rect, radius: f32, blur: f32, dy: f32, c: Color) {
    let steps = 6;
    for i in 0..steps {
        let t = (i as f32 + 1.0) / steps as f32;
        let grow = blur * (1.0 - t);
        let a = (c.a as f32 * t * t / steps as f32 * 1.6).round().min(255.0) as u8;
        fill_rrect(pm, r.translate(Vec2 { x: 0.0, y: dy }).inflate(grow), radius + grow, c.with_alpha(a));
    }
}

pub fn fill_circle(pm: &mut PixmapMut, c: Vec2, r: f32, col: Color) {
    if let Some(p) = PathBuilder::from_circle(c.x, c.y, r.max(0.1)) {
        pm.fill_path(&p, &paint(col), FillRule::Winding, Transform::identity(), None);
    }
}

pub fn stroke_circle(pm: &mut PixmapMut, c: Vec2, r: f32, w: f32, col: Color) {
    if let Some(p) = PathBuilder::from_circle(c.x, c.y, r.max(0.1)) {
        pm.stroke_path(&p, &paint(col), &Stroke { width: w, ..Default::default() }, Transform::identity(), None);
    }
}

pub fn line(pm: &mut PixmapMut, a: Vec2, b: Vec2, w: f32, col: Color) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    if let Some(p) = pb.finish() {
        let st = Stroke { width: w, line_cap: tiny_skia::LineCap::Round, ..Default::default() };
        pm.stroke_path(&p, &paint(col), &st, Transform::identity(), None);
    }
}

pub fn dashed_line(pm: &mut PixmapMut, a: Vec2, b: Vec2, w: f32, dash: f32, col: Color) {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.line_to(b.x, b.y);
    if let Some(p) = pb.finish() {
        let st = Stroke { width: w, dash: StrokeDash::new(vec![dash, dash], 0.0), ..Default::default() };
        pm.stroke_path(&p, &paint(col), &st, Transform::identity(), None);
    }
}

pub fn polygon_path(pts: &[Vec2]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let first = pts.first()?;
    pb.move_to(first.x, first.y);
    for p in &pts[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.close();
    pb.finish()
}

/// Copy `src` into `dst` at (x, y) (no blending).
pub fn blit_copy(dst: &mut PixmapMut, src: &tiny_skia::Pixmap, x: i32, y: i32) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + sw).min(dw);
    let y1 = (y + sh).min(dh);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let d = dst.data_mut();
    let s = src.data();
    let row = ((x1 - x0) * 4) as usize;
    for yy in y0..y1 {
        let di = ((yy * dw + x0) * 4) as usize;
        let si = (((yy - y) * sw + (x0 - x)) * 4) as usize;
        d[di..di + row].copy_from_slice(&s[si..si + row]);
    }
}
