//! Startup splash: "Cloudboard 27" with animated entrance, loading status and
//! progress. Drawn with the same renderer; sized in logical px × DPI scale.

use cb_core::geom::{v2, Rect};
use cb_core::Color;
use cb_render::paint::{fill_circle, fill_rect, fill_rrect, paint, stroke_circle};
use cb_render::text::{Fonts, TextStyle};
use cb_render::tiny_skia::{FillRule, GradientStop, LinearGradient, PathBuilder, PixmapMut, Point, SpreadMode, Stroke, Transform};

pub const SPLASH_W: f32 = 520.0;
pub const SPLASH_H: f32 = 300.0;
/// Minimum time the splash stays visible (lets the entrance animation finish).
pub const MIN_VISIBLE_MS: u64 = 900;

pub fn size(scale: f32) -> (u32, u32) {
    ((SPLASH_W * scale).round() as u32, (SPLASH_H * scale).round() as u32)
}

fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    t * t * t + 1.0
}

fn tc(c: Color) -> cb_render::tiny_skia::Color {
    cb_render::tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Draw the splash. `t` = seconds since shown, `progress` 0..1.
pub fn draw(pm: &mut PixmapMut, fonts: &mut Fonts, scale: f32, t: f32, progress: f32, status: &str) {
    let s = scale;
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    let full = Rect::new(0.0, 0.0, w, h);
    // Background gradient (deep blue → indigo).
    let top = Color::rgb(28, 78, 186);
    let bottom = Color::rgb(46, 42, 140);
    let mut p = cb_render::tiny_skia::Paint::default();
    p.shader = LinearGradient::new(Point::from_xy(0.0, 0.0), Point::from_xy(w, h), vec![GradientStop::new(0.0, tc(top)), GradientStop::new(1.0, tc(bottom))], SpreadMode::Pad, Transform::identity()).unwrap();
    if let Some(r) = cb_render::tiny_skia::Rect::from_ltrb(0.0, 0.0, w, h) {
        pm.fill_rect(r, &p, Transform::identity(), None);
    }

    // Decorative line art: compass arcs, set square, ruler ticks (slow drift).
    let drift = (t * 0.35).sin() * 6.0 * s;
    let line = Color::rgba(255, 255, 255, 34);
    let stroke = Stroke { width: 1.4 * s, ..Default::default() };
    // Concentric arcs (compass) on the right.
    let c = v2(w * 0.86 + drift, h * 0.28);
    for (i, r) in [70.0f32, 110.0, 150.0].iter().enumerate() {
        let mut pb = PathBuilder::new();
        let r = r * s;
        let a0 = 2.2 + i as f32 * 0.15;
        let steps = 40;
        for k in 0..=steps {
            let a = a0 + k as f32 / steps as f32 * 2.6;
            let q = v2(c.x + r * a.cos(), c.y + r * a.sin());
            if k == 0 {
                pb.move_to(q.x, q.y);
            } else {
                pb.line_to(q.x, q.y);
            }
        }
        if let Some(path) = pb.finish() {
            pm.stroke_path(&path, &paint(line), &stroke, Transform::identity(), None);
        }
    }
    fill_circle(pm, c, 4.0 * s, Color::rgba(255, 255, 255, 60));
    // Set square bottom-left.
    let o = v2(-20.0 * s - drift, h + 30.0 * s);
    let mut pb = PathBuilder::new();
    pb.move_to(o.x, o.y);
    pb.line_to(o.x + 210.0 * s, o.y);
    pb.line_to(o.x, o.y - 160.0 * s);
    pb.close();
    pb.move_to(o.x + 36.0 * s, o.y - 26.0 * s);
    pb.line_to(o.x + 120.0 * s, o.y - 26.0 * s);
    pb.line_to(o.x + 36.0 * s, o.y - 90.0 * s);
    pb.close();
    if let Some(path) = pb.finish() {
        pm.fill_path(&path, &paint(Color::rgba(255, 255, 255, 14)), FillRule::EvenOdd, Transform::identity(), None);
        pm.stroke_path(&path, &paint(line), &stroke, Transform::identity(), None);
    }
    // Ruler ticks along the top.
    for i in 0..40 {
        let x = 24.0 * s + i as f32 * 12.0 * s + drift * 0.5;
        let l = if i % 5 == 0 { 12.0 } else { 6.0 } * s;
        fill_rect(pm, Rect::new(x, 0.0, x + 1.0 * s, l), Color::rgba(255, 255, 255, 40));
    }
    stroke_circle(pm, v2(w * 0.12, h * 0.22), 22.0 * s, 1.2 * s, Color::rgba(255, 255, 255, 30));

    // Title: slides in and fades (ease-out cubic over 600 ms).
    let a = ease_out_cubic(t / 0.6);
    let alpha = (a * 255.0) as u8;
    let title = cb_core::version::splash_title();
    let x = 40.0 * s - (1.0 - a) * 80.0 * s;
    let ty = h * 0.34;
    fonts.draw(pm, &title, x, ty, TextStyle::new(44.0 * s, Color::rgba(255, 255, 255, alpha)).bold());
    let a2 = ease_out_cubic((t - 0.15) / 0.6);
    fonts.draw(pm, "云板 · 智慧课堂白板", 42.0 * s - (1.0 - a2) * 60.0 * s, ty + 62.0 * s, TextStyle::new(16.0 * s, Color::rgba(220, 230, 255, (a2 * 220.0) as u8)).weight(500));

    // Footer.
    let fy = h - 34.0 * s;
    fonts.draw_vcenter(pm, &format!("v{}", cb_core::version::APP_VERSION), 40.0 * s, fy, TextStyle::new(12.0 * s, Color::rgba(220, 230, 255, 200)));
    fonts.draw_vcenter(pm, status, w - 40.0 * s, fy, TextStyle::new(12.0 * s, Color::rgba(220, 230, 255, 200)).right());
    // Progress bar.
    let track = Rect::new(40.0 * s, h - 16.0 * s, w - 40.0 * s, h - 12.0 * s);
    fill_rrect(pm, track, 2.0 * s, Color::rgba(255, 255, 255, 40));
    let pw = track.w() * progress.clamp(0.0, 1.0);
    if pw > 0.5 {
        fill_rrect(pm, Rect::new(track.x0, track.y0, track.x0 + pw, track.y1), 2.0 * s, Color::rgba(255, 255, 255, 230));
    }
    let _ = full;
}
