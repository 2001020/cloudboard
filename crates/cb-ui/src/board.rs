//! Board rendering with a persistent cache of committed content.
//!
//! The cache is only rebuilt when the document, view or PDF tiles change.
//! Pure scrolls shift the cached pixels and render just the exposed strips,
//! which keeps scrolling cheap on low-end CPUs.

use crate::view::{Layout, Viewport};
use cb_core::doc::{Document, Page, Uid};
use cb_core::geom::{v2, Rect, Vec2};
use cb_core::Color;
use cb_pdf::cache::VisiblePage;
use cb_pdf::TileCache;
use cb_render::images::{draw_image, ImageCache};
use cb_render::paint::{blit_copy, fill_rect, shadow};
use cb_render::text::{Fonts, TextStyle};
use cb_render::theme::Theme;
use cb_render::tiny_skia::{Pixmap, PixmapMut, Transform};

pub const PT_TO_UNITS: f32 = 96.0 / 72.0;

#[derive(Debug, Clone, PartialEq)]
struct Key {
    rev: u64,
    zoom: u32,
    scale: u32,
    scroll: (i32, i32),
    size: (u32, u32),
    page: usize,
    tiles: u64,
    dark: bool,
    pressure: bool,
    hidden: Vec<Uid>,
}

pub struct BoardInput<'a> {
    pub doc: &'a Document,
    pub layout: &'a Layout,
    pub vp: Viewport,
    pub page: usize,
    pub theme: &'a Theme,
    pub pressure: bool,
    pub tiles: Option<&'a mut TileCache>,
    pub hidden: &'a [Uid],
}

#[derive(Default)]
pub struct BoardRenderer {
    cache: Option<Pixmap>,
    key: Option<Key>,
    pub images: ImageCache,
    /// Stats for the last frame (for diagnostics/benchmarks).
    pub last_full: bool,
    pub last_region_px: u64,
}

fn strip_bg(th: &Theme) -> Color {
    if th.dark {
        Color::rgb(44, 48, 60)
    } else {
        Color::rgb(208, 213, 222)
    }
}

impl BoardRenderer {
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    pub fn cache(&self) -> Option<&Pixmap> {
        self.cache.as_ref()
    }

    /// Bring the cache up to date and blit it into `frame` at the board rect.
    pub fn render(&mut self, mut inp: BoardInput, frame: &mut PixmapMut, fonts: &mut Fonts) {
        let vp = inp.vp;
        let w = vp.rect.w().round().max(1.0) as u32;
        let h = vp.rect.h().round().max(1.0) as u32;
        let key = Key {
            rev: inp.doc.revision,
            zoom: vp.zoom.to_bits(),
            scale: vp.ui_scale.to_bits(),
            scroll: (vp.scroll.x as i32, vp.scroll.y as i32),
            size: (w, h),
            page: inp.page,
            tiles: inp.tiles.as_ref().map(|t| t.version).unwrap_or(0),
            dark: inp.theme.dark,
            pressure: inp.pressure,
            hidden: inp.hidden.to_vec(),
        };
        self.last_full = false;
        self.last_region_px = 0;
        if self.cache.as_ref().map(|c| (c.width(), c.height())) != Some((w, h)) {
            self.cache = Pixmap::new(w, h);
            self.key = None;
        }
        if self.key.as_ref() != Some(&key) {
            let shift = match &self.key {
                Some(old) if Key { scroll: key.scroll, ..old.clone() } == key => Some((old.scroll.0 - key.scroll.0, old.scroll.1 - key.scroll.1)),
                _ => None,
            };
            let mut cache = self.cache.take().unwrap();
            match shift {
                Some((dx, dy)) if dx.unsigned_abs() < w / 2 && dy.unsigned_abs() < h / 2 => {
                    shift_pixmap(&mut cache, dx, dy);
                    // Exposed strips.
                    if dy > 0 {
                        self.render_region(&mut cache, (0, 0, w, dy as u32), &mut inp, fonts);
                    } else if dy < 0 {
                        self.render_region(&mut cache, (0, (h as i32 + dy) as u32, w, (-dy) as u32), &mut inp, fonts);
                    }
                    if dx > 0 {
                        self.render_region(&mut cache, (0, 0, dx as u32, h), &mut inp, fonts);
                    } else if dx < 0 {
                        self.render_region(&mut cache, ((w as i32 + dx) as u32, 0, (-dx) as u32, h), &mut inp, fonts);
                    }
                }
                _ => {
                    self.last_full = true;
                    self.render_region(&mut cache, (0, 0, w, h), &mut inp, fonts);
                }
            }
            self.cache = Some(cache);
            self.key = Some(key);
        }
        if let Some(c) = &self.cache {
            blit_copy(frame, c, vp.rect.x0 as i32, vp.rect.y0 as i32);
        }
    }

    /// Render the board-local pixel region (x, y, w, h) into `cache`.
    fn render_region(&mut self, cache: &mut Pixmap, region: (u32, u32, u32, u32), inp: &mut BoardInput, fonts: &mut Fonts) {
        let (rx, ry, rw, rh) = region;
        if rw == 0 || rh == 0 {
            return;
        }
        self.last_region_px += (rw * rh) as u64;
        let Some(mut tmp) = Pixmap::new(rw, rh) else { return };
        {
            let mut pm = tmp.as_mut();
            self.draw_content(&mut pm, (rx as f32, ry as f32), inp, fonts);
        }
        blit_copy(&mut cache.as_mut(), &tmp, rx as i32, ry as i32);
    }

    /// Draw everything committed, with `off` = board-local pixel origin of `pm`.
    fn draw_content(&mut self, pm: &mut PixmapMut, off: (f32, f32), inp: &mut BoardInput, fonts: &mut Fonts) {
        let vp = inp.vp;
        let k = vp.k();
        let th = inp.theme;
        let (pw, ph) = (pm.width() as f32, pm.height() as f32);
        // Canvas rect covered by this region.
        let c0 = v2((off.0 + vp.scroll.x) / k, (off.1 + vp.scroll.y) / k);
        let c1 = v2((off.0 + pw + vp.scroll.x) / k, (off.1 + ph + vp.scroll.y) / k);
        let region = Rect::from_points(c0, c1);
        let tf = |origin: Vec2| Transform::from_row(k, 0.0, 0.0, k, origin.x * k - vp.scroll.x - off.0, origin.y * k - vp.scroll.y - off.1);

        if !inp.layout.is_strip() {
            fill_rect(pm, Rect::new(0.0, 0.0, pw, ph), th.board_bg);
            if let Some(page) = inp.doc.pages.get(inp.page) {
                self.draw_page_items(pm, page, tf(Vec2::ZERO), k, &region, inp.pressure, inp.hidden, fonts, Vec2::ZERO, (vp.scroll.x + off.0, vp.scroll.y + off.1));
            }
            return;
        }
        fill_rect(pm, Rect::new(0.0, 0.0, pw, ph), strip_bg(th));
        let range = inp.layout.visible_pages(&region.inflate(8.0));
        let px_per_pt = k * PT_TO_UNITS;
        for i in range {
            let pr = inp.layout.pages[i];
            let sr = Rect::new(pr.x0 * k - vp.scroll.x - off.0, pr.y0 * k - vp.scroll.y - off.1, pr.x1 * k - vp.scroll.x - off.0, pr.y1 * k - vp.scroll.y - off.1);
            shadow(pm, sr, 2.0, 10.0 * vp.ui_scale, 2.0 * vp.ui_scale, Color::rgba(0, 0, 0, if th.dark { 90 } else { 50 }));
            fill_rect(pm, sr, Color::rgb(255, 255, 255));
            let origin = v2(pr.x0, pr.y0);
            if let Some(tiles) = inp.tiles.as_deref_mut() {
                let vis = region.intersect(&pr);
                if !vis.is_empty() {
                    let (wpt, hpt) = (pr.w() / PT_TO_UNITS, pr.h() / PT_TO_UNITS);
                    let vpg = VisiblePage {
                        page: i as u32,
                        size_pt: (wpt, hpt),
                        x0: (vis.x0 - pr.x0) / PT_TO_UNITS,
                        y0: (vis.y0 - pr.y0) / PT_TO_UNITS,
                        x1: (vis.x1 - pr.x0) / PT_TO_UNITS,
                        y1: (vis.y1 - pr.y0) / PT_TO_UNITS,
                        distance: 0.0,
                    };
                    for t in tiles.draw_list(&vpg, px_per_pt) {
                        let sx = (origin.x + t.x * PT_TO_UNITS) * k - vp.scroll.x - off.0;
                        let sy = (origin.y + t.y * PT_TO_UNITS) * k - vp.scroll.y - off.1;
                        let exact = (t.pixmap.width() as f32 - t.w * px_per_pt).abs() < 1.01 && (t.pixmap.height() as f32 - t.h * px_per_pt).abs() < 1.01;
                        if exact {
                            blit_copy(pm, &t.pixmap, sx.round() as i32, sy.round() as i32);
                        } else {
                            let tt = Transform::from_row(px_per_pt, 0.0, 0.0, px_per_pt, sx, sy);
                            draw_image(pm, &t.pixmap, 0.0, 0.0, t.w, t.h, 0.0, tt);
                        }
                    }
                }
            }
            if let Some(page) = inp.doc.pages.get(i) {
                let local_region = region.translate(-origin);
                self.draw_page_items(pm, page, tf(origin), k, &local_region, inp.pressure, inp.hidden, fonts, origin, (vp.scroll.x + off.0, vp.scroll.y + off.1));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_page_items(&mut self, pm: &mut PixmapMut, page: &Page, t: Transform, k: f32, region: &Rect, pressure: bool, hidden: &[Uid], fonts: &mut Fonts, origin: Vec2, scroll_off: (f32, f32)) {
        for im in &page.images {
            if hidden.contains(&im.uid) || !im.bounds().intersects(region) {
                continue;
            }
            if let Some(px) = self.images.get(im.uid, &im.data) {
                draw_image(pm, &px, im.pos.x, im.pos.y, im.w, im.h, im.rotate_deg, t);
            }
        }
        for s in &page.strokes {
            if hidden.contains(&s.uid) || !s.bounds().intersects(region) {
                continue;
            }
            cb_render::ink::draw_stroke(pm, s, t, pressure);
        }
        for tx in &page.texts {
            if hidden.contains(&tx.uid) || !tx.approx_bounds().inflate(tx.font_size).intersects(region) {
                continue;
            }
            draw_text_item(pm, fonts, tx, origin, k, scroll_off);
        }
    }
}

/// Draw a board text item. `scroll_off` = scroll + region offset in px.
pub fn draw_text_item(pm: &mut PixmapMut, fonts: &mut Fonts, tx: &cb_core::doc::TextItem, origin: Vec2, k: f32, scroll_off: (f32, f32)) {
    let x = (origin.x + tx.pos.x) * k - scroll_off.0;
    let y = (origin.y + tx.pos.y) * k - scroll_off.1;
    let st = TextStyle::new(tx.font_size * k, tx.color).weight(if tx.bold { 700 } else { 400 });
    let fam = if tx.family.is_empty() || tx.family == cb_render::text::UI_FAMILY { None } else { Some(tx.family.as_str()) };
    fonts.draw_family(pm, &tx.text, x, y, st, fam);
}

/// Shift pixmap contents by (dx, dy) pixels in place (exposed area undefined).
pub fn shift_pixmap(pm: &mut Pixmap, dx: i32, dy: i32) {
    let w = pm.width() as i32;
    let h = pm.height() as i32;
    let stride = (w * 4) as usize;
    let data = pm.data_mut();
    let rows: Vec<i32> = if dy > 0 { (0..h).rev().collect() } else { (0..h).collect() };
    for y in rows {
        let sy = y - dy;
        if sy < 0 || sy >= h {
            continue;
        }
        let (x0, x1) = (dx.max(0), (w + dx).min(w));
        if x1 <= x0 {
            continue;
        }
        let len = ((x1 - x0) * 4) as usize;
        let src = sy as usize * stride + ((x0 - dx) * 4) as usize;
        let dst = y as usize * stride + (x0 * 4) as usize;
        data.copy_within(src..src + len, dst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shift() {
        let mut p = Pixmap::new(4, 3).unwrap();
        for (i, px) in p.data_mut().chunks_exact_mut(4).enumerate() {
            px[0] = i as u8;
            px[3] = 255;
        }
        shift_pixmap(&mut p, 1, 1);
        // pixel (1,1) now holds old (0,0) = 0; pixel (3,2) holds old (2,1) = 6
        assert_eq!(p.data()[(1 * 4 + 1) * 4], 0);
        assert_eq!(p.data()[(2 * 4 + 3) * 4], 6);
        shift_pixmap(&mut p, -1, -1);
        assert_eq!(p.data()[0], 0);
    }
}
