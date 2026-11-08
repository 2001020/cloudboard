//! Draw a [`Page`] into a tiny-skia pixmap at any scale.

use crate::model::*;
use cosmic_text::{CacheKey, FontSystem};
use std::collections::HashMap;
use swash::scale::image::{Content as SwashContent, Image as SwashImage};
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::{Format, Vector};
use std::collections::HashSet;
use tiny_skia::{FillRule, FilterQuality, GradientStop, LinearGradient, Pattern, PathBuilder, Pixmap, PixmapMut, Point, RadialGradient, Rect, SpreadMode, Stroke, StrokeDash, Transform};

/// Which items to draw.
#[derive(Default)]
pub struct Filter<'a> {
    /// Items belonging to any of these shape ids are skipped.
    pub hidden: Option<&'a HashSet<u32>>,
    /// If set, only items belonging to one of these ids are drawn.
    pub only: Option<&'a [u32]>,
    pub background: bool,
}

/// Glyph rasterizer without hinting: document text scales exactly with the
/// zoom, and some CJK fonts' hinting programs distort glyphs in swash.
pub struct Renderer {
    ctx: ScaleContext,
    glyphs: HashMap<CacheKey, Option<SwashImage>>,
}

impl Renderer {
    fn glyph(&mut self, fs: &mut FontSystem, key: CacheKey) -> Option<&SwashImage> {
        if self.glyphs.len() > 4096 {
            self.glyphs.clear();
        }
        if !self.glyphs.contains_key(&key) {
            let img = fs.get_font(key.font_id).and_then(|font| {
                let mut scaler = self.ctx.builder(font.as_swash()).size(f32::from_bits(key.font_size_bits)).hint(false).build();
                let offset = Vector::new(key.x_bin.as_float(), key.y_bin.as_float());
                Render::new(&[Source::ColorOutline(0), Source::Outline]).format(Format::Alpha).offset(offset).render(&mut scaler, key.glyph_id)
            });
            self.glyphs.insert(key, img);
        }
        self.glyphs.get(&key).and_then(|g| g.as_ref())
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Renderer {
        Renderer { ctx: ScaleContext::new(), glyphs: HashMap::new() }
    }

    /// Draw `page` with `base` mapping points to device pixels.
    pub fn draw_page(&mut self, fs: &mut FontSystem, page: &Page, pm: &mut PixmapMut, base: Transform, filter: &Filter) {
        if filter.background {
            match &page.background {
                Some(p) => {
                    let r = Rect::from_xywh(0.0, 0.0, page.width, page.height).unwrap();
                    fill_rect_paint(pm, r, p, base, 1.0);
                }
                None => {}
            }
        }
        for it in &page.items {
            if let Some(h) = filter.hidden {
                if it.ids.iter().any(|i| h.contains(i)) {
                    continue;
                }
            }
            if let Some(only) = filter.only {
                if !it.ids.iter().any(|i| only.contains(i)) {
                    continue;
                }
            }
            self.draw_item(fs, it, pm, base);
        }
    }

    pub fn draw_item(&mut self, fs: &mut FontSystem, it: &Item, pm: &mut PixmapMut, base: Transform) {
        let t = base.pre_concat(it.xf);
        match &it.kind {
            Kind::Shape { geom, fill, line } => {
                for sp in geom {
                    if sp.fill {
                        if let Some(f) = fill {
                            let f = if (sp.shade - 1.0).abs() > 0.01 { shade_paint(f, sp.shade) } else { f.clone() };
                            fill_path_paint(pm, &sp.path, &f, t, it.w, it.h, 1.0);
                        }
                    }
                }
                if let Some(l) = line {
                    for sp in geom.iter().filter(|s| s.stroke) {
                        stroke_path(pm, &sp.path, l, t);
                    }
                }
            }
            Kind::Image { img, crop, alpha } => draw_image(pm, img, *crop, *alpha, t, it.w, it.h),
            Kind::Text(blocks) => {
                for b in blocks {
                    self.draw_text(fs, pm, b, t, it.w, it.h);
                }
            }
            Kind::Placeholder(label) => {
                if let Some(r) = Rect::from_xywh(0.0, 0.0, it.w.max(1.0), it.h.max(1.0)) {
                    let path = PathBuilder::from_rect(r);
                    let mut p = tiny_skia::Paint::default();
                    p.set_color_rgba8(120, 130, 150, 28);
                    pm.fill_path(&path, &p, FillRule::Winding, t, None);
                    p.set_color_rgba8(120, 130, 150, 180);
                    let st = Stroke { width: 1.0, dash: StrokeDash::new(vec![4.0, 3.0], 0.0), ..Default::default() };
                    pm.stroke_path(&path, &p, &st, t, None);
                }
                let _ = label;
            }
        }
    }

    fn draw_text(&mut self, fs: &mut FontSystem, pm: &mut PixmapMut, b: &TextBlock, t: Transform, w: f32, h: f32) {
        let t = t.pre_translate(b.dx, b.dy);
        let upright = t.kx.abs() < 1e-4 && t.ky.abs() < 1e-4 && t.sx > 0.0 && (t.sx - t.sy).abs() < 1e-3;
        if upright {
            self.blit_text(fs, pm, b, t.sx, t.tx, t.ty);
            return;
        }
        // Rotated/flipped text: draw upright into a temporary pixmap, then transform it.
        let scale = (t.sx * t.sx + t.ky * t.ky).sqrt().max(0.01);
        let (tw, th) = (((w - b.dx).max(1.0) * scale).ceil() as u32 + 4, ((h - b.dy).max(1.0) * scale).ceil() as u32 + 4);
        let (tw, th) = (tw.min(8192), th.min(8192));
        let Some(mut tmp) = Pixmap::new(tw, th) else { return };
        self.blit_text(fs, &mut tmp.as_mut(), b, scale, 0.0, 0.0);
        let back = t.pre_scale(1.0 / scale, 1.0 / scale);
        let paint = tiny_skia::PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
        pm.draw_pixmap(0, 0, tmp.as_ref(), &paint, back, None);
    }

    /// Upright text at `scale` px per pt with the buffer origin at (ox, oy) px.
    fn blit_text(&mut self, fs: &mut FontSystem, pm: &mut PixmapMut, b: &TextBlock, scale: f32, ox: f32, oy: f32) {
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        let mut decos: Vec<(f32, f32, f32, f32, Rgba)> = Vec::new();
        let mut highlights: Vec<(f32, f32, f32, f32, Rgba)> = Vec::new();
        let runs: Vec<_> = b.buf.layout_runs().enumerate().filter(|(i, _)| *i >= b.lines.0 && *i < b.lines.1).map(|(_, r)| r).collect();
        let first_top = runs.first().map(|r| r.line_top).unwrap_or(0.0);
        for run in &runs {
            let base_y = run.line_y - if b.lines.0 > 0 { first_top } else { 0.0 };
            for g in run.glyphs.iter() {
                let color = g.color_opt.map(|c| Rgba { r: c.r(), g: c.g(), b: c.b(), a: c.a() }).unwrap_or(Rgba::BLACK);
                if let Some(d) = b.spans.get(g.metadata) {
                    let x0 = ox + g.x * scale;
                    let x1 = ox + (g.x + g.w) * scale;
                    if let Some(hl) = d.highlight {
                        highlights.push((x0, oy + (base_y - g.font_size * 0.92) * scale, x1, oy + (base_y + g.font_size * 0.28) * scale, hl));
                    }
                    if d.underline {
                        let y = oy + (base_y + g.font_size * 0.13) * scale;
                        decos.push((x0, y, x1, (g.font_size * 0.06 * scale).max(1.0), color));
                    }
                    if d.strike {
                        let y = oy + (base_y - g.font_size * 0.3) * scale;
                        decos.push((x0, y, x1, (g.font_size * 0.06 * scale).max(1.0), color));
                    }
                }
            }
        }
        for (x0, y0, x1, y1, c) in highlights {
            if let Some(r) = Rect::from_ltrb(x0, y0, x1, y1) {
                let mut p = tiny_skia::Paint::default();
                p.set_color(c.to_skia());
                pm.fill_rect(r, &p, Transform::identity(), None);
            }
        }
        for run in &runs {
            let base_y = run.line_y - if b.lines.0 > 0 { first_top } else { 0.0 };
            for g in run.glyphs.iter() {
                let color = g.color_opt.map(|c| Rgba { r: c.r(), g: c.g(), b: c.b(), a: c.a() }).unwrap_or(Rgba::BLACK);
                let pg = g.physical((ox, oy + base_y * scale), scale);
                let Some(img) = self.glyph(fs, pg.cache_key) else { continue };
                let data = pm.data_mut();
                let gx = pg.x + img.placement.left;
                let gy = pg.y - img.placement.top;
                let (iw, ih) = (img.placement.width as i32, img.placement.height as i32);
                for yy in 0..ih {
                    let py = gy + yy;
                    if py < 0 || py >= ph {
                        continue;
                    }
                    for xx in 0..iw {
                        let px = gx + xx;
                        if px < 0 || px >= pw {
                            continue;
                        }
                        let i = ((py * pw + px) * 4) as usize;
                        match img.content {
                            SwashContent::Mask => {
                                let a = img.data[(yy * iw + xx) as usize];
                                if a > 0 {
                                    blend(data, i, color, a);
                                }
                            }
                            SwashContent::Color => {
                                let k = ((yy * iw + xx) * 4) as usize;
                                let a = img.data[k + 3];
                                if a > 0 {
                                    blend(data, i, Rgba { r: img.data[k], g: img.data[k + 1], b: img.data[k + 2], a: 255 }, a);
                                }
                            }
                            SwashContent::SubpixelMask => {}
                        }
                    }
                }
            }
        }
        for (x0, y, x1, th, c) in decos {
            if let Some(r) = Rect::from_ltrb(x0, y, x1, y + th) {
                let mut p = tiny_skia::Paint::default();
                p.set_color(c.to_skia());
                pm.fill_rect(r, &p, Transform::identity(), None);
            }
        }
    }
}

#[inline]
fn blend(data: &mut [u8], i: usize, c: Rgba, cov: u8) {
    let a = (cov as u32 * c.a as u32 + 127) / 255;
    if a == 0 {
        return;
    }
    let inv = 255 - a;
    let mix = |src: u8, dst: u8| -> u8 { ((src as u32 * a + dst as u32 * inv + 127) / 255) as u8 };
    data[i] = mix(c.r, data[i]);
    data[i + 1] = mix(c.g, data[i + 1]);
    data[i + 2] = mix(c.b, data[i + 2]);
    data[i + 3] = (a + (data[i + 3] as u32 * inv + 127) / 255) as u8;
}

fn shade_paint(p: &Paint, k: f32) -> Paint {
    let f = |c: Rgba| {
        let m = |v: u8| if k < 1.0 { (v as f32 * k) as u8 } else { (v as f32 + (255.0 - v as f32) * (k - 1.0)).min(255.0) as u8 };
        Rgba { r: m(c.r), g: m(c.g), b: m(c.b), a: c.a }
    };
    match p {
        Paint::Solid(c) => Paint::Solid(f(*c)),
        Paint::Linear { angle, stops } => Paint::Linear { angle: *angle, stops: stops.iter().map(|(p, c)| (*p, f(*c))).collect() },
        Paint::Radial { stops } => Paint::Radial { stops: stops.iter().map(|(p, c)| (*p, f(*c))).collect() },
        other => other.clone(),
    }
}

fn stops(v: &[(f32, Rgba)]) -> Vec<GradientStop> {
    v.iter().map(|(p, c)| GradientStop::new(p.clamp(0.0, 1.0), c.to_skia())).collect()
}

/// Build a tiny-skia paint for `p` over a local box of w×h.
fn skia_paint(p: &Paint, w: f32, h: f32, opacity: f32) -> Option<tiny_skia::Paint<'_>> {
    let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
    match p {
        Paint::Solid(c) => paint.set_color(c.with_alpha((c.a as f32 * opacity) as u8).to_skia()),
        Paint::Linear { angle, stops: s } => {
            if s.len() < 2 {
                paint.set_color(s.first().map(|x| x.1).unwrap_or(Rgba::WHITE).to_skia());
            } else {
                let a = angle.to_radians();
                let (dx, dy) = (a.cos(), a.sin());
                let half = (w * 0.5 * dx).abs() + (h * 0.5 * dy).abs();
                let (cx, cy) = (w * 0.5, h * 0.5);
                paint.shader = LinearGradient::new(Point::from_xy(cx - dx * half, cy - dy * half), Point::from_xy(cx + dx * half, cy + dy * half), stops(s), SpreadMode::Pad, Transform::identity())?;
            }
        }
        Paint::Radial { stops: s } => {
            if s.len() < 2 {
                paint.set_color(s.first().map(|x| x.1).unwrap_or(Rgba::WHITE).to_skia());
            } else {
                let c = Point::from_xy(w * 0.5, h * 0.5);
                paint.shader = RadialGradient::new(c, c, (w.max(h) * 0.71).max(1.0), stops(s), SpreadMode::Pad, Transform::identity())?;
            }
        }
        Paint::Image { img } => {
            let sx = w / img.width() as f32;
            let sy = h / img.height() as f32;
            paint.shader = Pattern::new((**img).as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, opacity, Transform::from_scale(sx, sy));
        }
    }
    Some(paint)
}

fn fill_path_paint(pm: &mut PixmapMut, path: &tiny_skia::Path, p: &Paint, t: Transform, w: f32, h: f32, opacity: f32) {
    if let Some(paint) = skia_paint(p, w, h, opacity) {
        pm.fill_path(path, &paint, FillRule::Winding, t, None);
    }
}

fn fill_rect_paint(pm: &mut PixmapMut, r: Rect, p: &Paint, t: Transform, opacity: f32) {
    let path = PathBuilder::from_rect(r);
    fill_path_paint(pm, &path, p, t, r.width(), r.height(), opacity);
}

fn draw_image(pm: &mut PixmapMut, img: &Pixmap, crop: [f32; 4], alpha: f32, t: Transform, w: f32, h: f32) {
    let (iw, ih) = (img.width() as f32, img.height() as f32);
    let cw = (1.0 - crop[0] - crop[2]).max(0.001) * iw;
    let ch = (1.0 - crop[1] - crop[3]).max(0.001) * ih;
    let it = Transform::from_scale(w / cw, h / ch).pre_translate(-crop[0] * iw, -crop[1] * ih);
    let paint = tiny_skia::Paint { shader: Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bicubic, alpha.clamp(0.0, 1.0), it), anti_alias: true, ..Default::default() };
    if let Some(r) = Rect::from_xywh(0.0, 0.0, w.max(0.01), h.max(0.01)) {
        pm.fill_path(&PathBuilder::from_rect(r), &paint, FillRule::Winding, t, None);
    }
}

fn stroke_path(pm: &mut PixmapMut, path: &tiny_skia::Path, l: &Line, t: Transform) {
    if l.width <= 0.0 || l.color.a == 0 {
        return;
    }
    let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
    paint.set_color(l.color.to_skia());
    // Keep hairlines visible when zoomed out.
    let scale = (t.sx * t.sx + t.ky * t.ky).sqrt().max(1e-3);
    let width = l.width.max(0.75 / scale);
    let w = width;
    let dash = match l.dash {
        Dash::Solid => None,
        Dash::Dash => StrokeDash::new(vec![4.0 * w, 3.0 * w], 0.0),
        Dash::Dot => StrokeDash::new(vec![1.0 * w, 2.0 * w], 0.0),
        Dash::DashDot => StrokeDash::new(vec![4.0 * w, 3.0 * w, 1.0 * w, 3.0 * w], 0.0),
        Dash::LongDash => StrokeDash::new(vec![8.0 * w, 3.0 * w], 0.0),
    };
    let st = Stroke { width, line_cap: if matches!(l.dash, Dash::Dot) { tiny_skia::LineCap::Round } else { tiny_skia::LineCap::Butt }, line_join: tiny_skia::LineJoin::Round, dash, ..Default::default() };
    pm.stroke_path(path, &paint, &st, t, None);
    if l.head != Arrow::None || l.tail != Arrow::None {
        let pts = endpoints(path);
        if let Some((a0, a1, b0, b1)) = pts {
            if l.head != Arrow::None {
                arrow(pm, a1, a0, l, width, t);
            }
            if l.tail != Arrow::None {
                arrow(pm, b0, b1, l, width, t);
            }
        }
    }
}

/// (first point, second point, second-to-last point, last point) of a path.
fn endpoints(path: &tiny_skia::Path) -> Option<(Point, Point, Point, Point)> {
    let pts = path.points();
    if pts.len() < 2 {
        return None;
    }
    Some((pts[0], pts[1], pts[pts.len() - 2], pts[pts.len() - 1]))
}

fn arrow(pm: &mut PixmapMut, from: Point, tip: Point, l: &Line, w: f32, t: Transform) {
    let (dx, dy) = (tip.x - from.x, tip.y - from.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let size = (w * 3.0).max(4.0);
    let (bx, by) = (tip.x - ux * size, tip.y - uy * size);
    let (nx, ny) = (-uy * size * 0.5, ux * size * 0.5);
    let mut pb = PathBuilder::new();
    let style = if l.tail == Arrow::Oval && l.head == Arrow::Oval { Arrow::Oval } else { Arrow::Triangle };
    match style {
        Arrow::Oval => pb.push_circle(tip.x, tip.y, size * 0.4),
        _ => {
            pb.move_to(tip.x, tip.y);
            pb.line_to(bx + nx, by + ny);
            pb.line_to(bx - nx, by - ny);
            pb.close();
        }
    }
    if let Some(p) = pb.finish() {
        let mut paint = tiny_skia::Paint { anti_alias: true, ..Default::default() };
        paint.set_color(l.color.to_skia());
        pm.fill_path(&p, &paint, FillRule::Winding, t, None);
    }
}
