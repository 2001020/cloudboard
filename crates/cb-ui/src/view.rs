//! Viewport: maps canvas coordinates (board units) to physical screen pixels.
//!
//! screen = canvas × (zoom × ui_scale) − scroll
//!
//! Blank boards show one page at a time (unbounded canvas). PDF documents are
//! laid out as a vertical strip of pages centred on canvas x = 0.

use cb_core::doc::{Document, PageBackground};
use cb_core::geom::{v2, Rect, Vec2};

pub const PAGE_GAP: f32 = 20.0;
pub const ZOOM_MIN: f32 = 0.25;
pub const ZOOM_MAX: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub zoom: f32,
    pub ui_scale: f32,
    pub scroll: Vec2,
    /// Board area in physical px.
    pub rect: Rect,
}

impl Viewport {
    pub fn k(&self) -> f32 {
        self.zoom * self.ui_scale
    }
    pub fn to_screen(&self, p: Vec2) -> Vec2 {
        v2(p.x * self.k() - self.scroll.x + self.rect.x0, p.y * self.k() - self.scroll.y + self.rect.y0)
    }
    pub fn to_canvas(&self, s: Vec2) -> Vec2 {
        v2((s.x - self.rect.x0 + self.scroll.x) / self.k(), (s.y - self.rect.y0 + self.scroll.y) / self.k())
    }
    /// Visible canvas rectangle.
    pub fn visible(&self) -> Rect {
        let a = self.to_canvas(v2(self.rect.x0, self.rect.y0));
        let b = self.to_canvas(v2(self.rect.x1, self.rect.y1));
        Rect::from_points(a, b)
    }
    /// Zoom about a screen point, keeping it fixed.
    pub fn zoom_about(&mut self, new_zoom: f32, at: Vec2) {
        let nz = new_zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        let c = self.to_canvas(at);
        self.zoom = nz;
        let s = self.to_screen(c);
        self.scroll += s - at;
    }
    /// Integer-snapped scroll for pixel-exact blits.
    pub fn snapped(&self) -> Viewport {
        Viewport { scroll: v2(self.scroll.x.round(), self.scroll.y.round()), ..*self }
    }
}

/// Page placement in canvas space.
#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// Canvas rectangle of each page (PDF mode); empty for blank boards.
    pub pages: Vec<Rect>,
    pub total: Rect,
}

impl Layout {
    pub fn build(doc: &Document) -> Layout {
        if !doc.is_pdf() {
            return Layout::default();
        }
        let mut y = PAGE_GAP;
        let mut pages = Vec::with_capacity(doc.pages.len());
        let mut total = Rect::EMPTY;
        for p in &doc.pages {
            let (w, h) = match p.background {
                PageBackground::Pdf { width, height, .. } => (width, height),
                PageBackground::Blank => (794.0, 1123.0),
            };
            let r = Rect::xywh(-w * 0.5, y, w, h);
            total = total.union(&r);
            pages.push(r);
            y += h + PAGE_GAP;
        }
        if !pages.is_empty() {
            total.y1 += PAGE_GAP;
            total.y0 = 0.0;
        }
        Layout { pages, total }
    }

    pub fn is_strip(&self) -> bool {
        !self.pages.is_empty()
    }

    /// Page under a canvas point (nearest page for gaps).
    pub fn page_at(&self, p: Vec2) -> Option<usize> {
        if self.pages.is_empty() {
            return None;
        }
        let i = self.pages.partition_point(|r| r.y1 + PAGE_GAP * 0.5 < p.y);
        Some(i.min(self.pages.len() - 1))
    }

    /// Canvas origin of page i (top-left); zero for blank boards.
    pub fn origin(&self, i: usize) -> Vec2 {
        self.pages.get(i).map(|r| v2(r.x0, r.y0)).unwrap_or(Vec2::ZERO)
    }

    /// Range of pages intersecting a canvas rect.
    pub fn visible_pages(&self, vis: &Rect) -> std::ops::Range<usize> {
        if self.pages.is_empty() {
            return 0..0;
        }
        let a = self.pages.partition_point(|r| r.y1 < vis.y0);
        let b = self.pages.partition_point(|r| r.y0 <= vis.y1);
        a..b.max(a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_core::doc::Page;

    #[test]
    fn roundtrip_and_zoom_anchor() {
        let mut v = Viewport { zoom: 1.0, ui_scale: 1.5, scroll: v2(10.0, 20.0), rect: Rect::new(0.0, 0.0, 800.0, 600.0) };
        let p = v2(123.0, 45.0);
        assert!(v.to_canvas(v.to_screen(p)).dist(p) < 1e-3);
        let at = v2(400.0, 300.0);
        let c = v.to_canvas(at);
        v.zoom_about(2.0, at);
        assert!(v.to_screen(c).dist(at) < 1e-3);
    }

    #[test]
    fn strip_layout() {
        let mut d = Document::new();
        d.pdf = Some(cb_core::doc::PdfSource { file_name: "a.pdf".into(), bytes: std::sync::Arc::new(vec![]) });
        d.pages = (0..3).map(|i| Page { background: PageBackground::Pdf { index: i, width: 800.0, height: 1000.0 }, ..Default::default() }).collect();
        let l = Layout::build(&d);
        assert_eq!(l.pages.len(), 3);
        assert_eq!(l.page_at(v2(0.0, 1035.0)), Some(1));
        assert_eq!(l.page_at(v2(0.0, 1025.0)), Some(0));
        assert_eq!(l.page_at(v2(0.0, -50.0)), Some(0));
        assert_eq!(l.visible_pages(&Rect::new(-10.0, 900.0, 10.0, 1100.0)), 0..2);
        assert_eq!(l.origin(1), v2(-400.0, 2.0 * PAGE_GAP + 1000.0));
    }
}
