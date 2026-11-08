//! Document model: pages containing strokes, texts and images.
//!
//! Coordinates are page-local "board units". One board unit is one logical
//! pixel at 100% zoom. Blank pages are unbounded; PDF pages have a fixed size
//! (`PageBackground::Pdf`) where 1 unit = 1/96 inch (pt × 96/72).

use crate::geom::{v2, Rect, Vec2};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type Uid = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }
    pub fn to_argb(self) -> u32 {
        (self.a as u32) << 24 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }
    pub fn from_argb(v: u32) -> Color {
        Color { a: (v >> 24) as u8, r: (v >> 16) as u8, g: (v >> 8) as u8, b: v as u8 }
    }
    pub fn with_alpha(self, a: u8) -> Color {
        Color { a, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineStyle {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl LineStyle {
    pub fn from_i32(v: i32) -> LineStyle {
        match v {
            1 => LineStyle::Dashed,
            2 => LineStyle::Dotted,
            _ => LineStyle::Solid,
        }
    }
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Rect,
    Ellipse,
    Line,
    Arrow,
    Triangle,
}

impl ShapeKind {
    pub fn from_i32(v: i32) -> Option<ShapeKind> {
        Some(match v {
            0 => ShapeKind::Rect,
            1 => ShapeKind::Ellipse,
            2 => ShapeKind::Line,
            3 => ShapeKind::Arrow,
            4 => ShapeKind::Triangle,
            _ => return None,
        })
    }
    pub fn to_i32(s: Option<ShapeKind>) -> i32 {
        match s {
            Some(k) => k as i32,
            None => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct InkPoint {
    pub x: f32,
    pub y: f32,
    /// 0..=1 (1.0 when the device has no pressure)
    pub pressure: f32,
    /// milliseconds since the previous point (saturating)
    pub dt_ms: u16,
}

impl InkPoint {
    pub fn new(x: f32, y: f32) -> InkPoint {
        InkPoint { x, y, pressure: 1.0, dt_ms: 0 }
    }
    pub fn pos(&self) -> Vec2 {
        v2(self.x, self.y)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub uid: Uid,
    pub points: Vec<InkPoint>,
    pub color: Color,
    pub width: f32,
    pub style: LineStyle,
    pub highlighter: bool,
    /// When set, the stroke is a geometric shape spanning `a`..`b`.
    pub shape: Option<ShapeKind>,
    pub a: Vec2,
    pub b: Vec2,
}

impl Stroke {
    pub fn new(uid: Uid, color: Color, width: f32) -> Stroke {
        Stroke { uid, points: Vec::new(), color, width, style: LineStyle::Solid, highlighter: false, shape: None, a: Vec2::ZERO, b: Vec2::ZERO }
    }

    /// Polyline describing the stroke (shape strokes are tessellated).
    pub fn outline(&self) -> Vec<Vec2> {
        match self.shape {
            None => self.points.iter().map(|p| p.pos()).collect(),
            Some(k) => shape_outline(k, self.a, self.b),
        }
    }

    pub fn bounds(&self) -> Rect {
        let mut r = Rect::EMPTY;
        for p in self.outline() {
            r.include(p);
        }
        if self.shape == Some(ShapeKind::Arrow) {
            r = r.inflate(self.width * 4.0 + 12.0);
        }
        r.inflate(self.width * 0.5 + 1.0)
    }

    pub fn translate(&mut self, d: Vec2) {
        for p in &mut self.points {
            p.x += d.x;
            p.y += d.y;
        }
        self.a += d;
        self.b += d;
    }
}

/// Tessellate a geometric shape into a polyline.
pub fn shape_outline(kind: ShapeKind, a: Vec2, b: Vec2) -> Vec<Vec2> {
    let r = Rect::from_points(a, b);
    match kind {
        ShapeKind::Rect => vec![v2(r.x0, r.y0), v2(r.x1, r.y0), v2(r.x1, r.y1), v2(r.x0, r.y1), v2(r.x0, r.y0)],
        ShapeKind::Triangle => vec![v2(r.center().x, r.y0), v2(r.x1, r.y1), v2(r.x0, r.y1), v2(r.center().x, r.y0)],
        ShapeKind::Line | ShapeKind::Arrow => vec![a, b],
        ShapeKind::Ellipse => {
            let c = r.center();
            let (rx, ry) = (r.w() * 0.5, r.h() * 0.5);
            let n = (((rx + ry) * 0.5).sqrt() * 6.0).clamp(24.0, 180.0) as usize;
            (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32 * std::f32::consts::TAU;
                    v2(c.x + rx * t.cos(), c.y + ry * t.sin())
                })
                .collect()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextItem {
    pub uid: Uid,
    pub text: String,
    pub pos: Vec2,
    pub color: Color,
    pub font_size: f32,
    pub family: String,
    pub bold: bool,
}

impl TextItem {
    /// Approximate bounds (the renderer has exact metrics; this is for hit testing).
    pub fn approx_bounds(&self) -> Rect {
        let lines: Vec<&str> = self.text.split('\n').collect();
        let maxw = lines
            .iter()
            .map(|l| l.chars().map(|c| if (c as u32) > 0x2E80 { 1.0 } else { 0.56 }).sum::<f32>())
            .fold(1.0f32, f32::max);
        Rect::xywh(self.pos.x, self.pos.y, maxw * self.font_size, lines.len().max(1) as f32 * self.font_size * 1.3)
    }
}

/// Encoded image (PNG or JPEG bytes). Decoding/caching is the renderer's job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageItem {
    pub uid: Uid,
    pub pos: Vec2,
    pub w: f32,
    pub h: f32,
    pub rotate_deg: f32,
    #[serde(skip)]
    pub data: Arc<Vec<u8>>,
}

impl ImageItem {
    pub fn bounds(&self) -> Rect {
        Rect::xywh(self.pos.x, self.pos.y, self.w, self.h)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub enum PageBackground {
    #[default]
    Blank,
    /// Page `index` of the document's PDF; size in board units.
    Pdf { index: u32, width: f32, height: f32 },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub background: PageBackground,
    pub strokes: Vec<Stroke>,
    pub texts: Vec<TextItem>,
    pub images: Vec<ImageItem>,
}

impl Page {
    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty() && self.texts.is_empty() && self.images.is_empty()
    }
    pub fn size(&self) -> Option<Vec2> {
        match self.background {
            PageBackground::Blank => None,
            PageBackground::Pdf { width, height, .. } => Some(v2(width, height)),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PdfSource {
    pub file_name: String,
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemKind {
    Stroke,
    Text,
    Image,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Stroke(Stroke),
    Text(TextItem),
    Image(ImageItem),
}

impl Item {
    pub fn uid(&self) -> Uid {
        match self {
            Item::Stroke(s) => s.uid,
            Item::Text(t) => t.uid,
            Item::Image(i) => i.uid,
        }
    }
    pub fn kind(&self) -> ItemKind {
        match self {
            Item::Stroke(_) => ItemKind::Stroke,
            Item::Text(_) => ItemKind::Text,
            Item::Image(_) => ItemKind::Image,
        }
    }
    pub fn bounds(&self) -> Rect {
        match self {
            Item::Stroke(s) => s.bounds(),
            Item::Text(t) => t.approx_bounds(),
            Item::Image(i) => i.bounds(),
        }
    }
    pub fn translate(&mut self, d: Vec2) {
        match self {
            Item::Stroke(s) => s.translate(d),
            Item::Text(t) => t.pos += d,
            Item::Image(i) => i.pos += d,
        }
    }
    pub fn set_color(&mut self, c: Color) {
        match self {
            Item::Stroke(s) => s.color = if s.highlighter { c.with_alpha(s.color.a) } else { c },
            Item::Text(t) => t.color = c,
            Item::Image(_) => {}
        }
    }
}

pub const MAX_PAGES: usize = 200;

#[derive(Debug, Clone)]
pub struct Document {
    pub pages: Vec<Page>,
    pub next_uid: Uid,
    pub pdf: Option<PdfSource>,
    /// Bumped on every mutation; renderers use it to invalidate caches.
    pub revision: u64,
}

impl Default for Document {
    fn default() -> Self {
        Document { pages: vec![Page::default()], next_uid: 1, pdf: None, revision: 0 }
    }
}

impl Document {
    pub fn new() -> Document {
        Document::default()
    }
    pub fn alloc_uid(&mut self) -> Uid {
        let u = self.next_uid;
        self.next_uid += 1;
        u
    }
    pub fn is_pdf(&self) -> bool {
        self.pdf.is_some()
    }
    pub fn has_ink(&self) -> bool {
        self.pages.iter().any(|p| !p.is_empty())
    }
    pub fn touch(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn find(&self, page: usize, uid: Uid) -> Option<(ItemKind, usize)> {
        let p = self.pages.get(page)?;
        if let Some(i) = p.strokes.iter().position(|s| s.uid == uid) {
            return Some((ItemKind::Stroke, i));
        }
        if let Some(i) = p.texts.iter().position(|s| s.uid == uid) {
            return Some((ItemKind::Text, i));
        }
        if let Some(i) = p.images.iter().position(|s| s.uid == uid) {
            return Some((ItemKind::Image, i));
        }
        None
    }

    pub fn get_item(&self, page: usize, uid: Uid) -> Option<Item> {
        let (k, i) = self.find(page, uid)?;
        let p = &self.pages[page];
        Some(match k {
            ItemKind::Stroke => Item::Stroke(p.strokes[i].clone()),
            ItemKind::Text => Item::Text(p.texts[i].clone()),
            ItemKind::Image => Item::Image(p.images[i].clone()),
        })
    }

    /// Insert item at z-index `idx` (clamped) within its list.
    pub fn insert_item(&mut self, page: usize, idx: usize, item: Item) {
        let p = &mut self.pages[page];
        match item {
            Item::Stroke(s) => {
                let i = idx.min(p.strokes.len());
                p.strokes.insert(i, s)
            }
            Item::Text(t) => {
                let i = idx.min(p.texts.len());
                p.texts.insert(i, t)
            }
            Item::Image(im) => {
                let i = idx.min(p.images.len());
                p.images.insert(i, im)
            }
        }
    }

    /// Remove item by uid, returning its index and value.
    pub fn remove_item(&mut self, page: usize, uid: Uid) -> Option<(usize, Item)> {
        let (k, i) = self.find(page, uid)?;
        let p = &mut self.pages[page];
        Some(match k {
            ItemKind::Stroke => (i, Item::Stroke(p.strokes.remove(i))),
            ItemKind::Text => (i, Item::Text(p.texts.remove(i))),
            ItemKind::Image => (i, Item::Image(p.images.remove(i))),
        })
    }

    /// Replace an item with the same uid in place.
    pub fn replace_item(&mut self, page: usize, item: Item) -> bool {
        let Some((k, i)) = self.find(page, item.uid()) else { return false };
        let p = &mut self.pages[page];
        match (k, item) {
            (ItemKind::Stroke, Item::Stroke(s)) => p.strokes[i] = s,
            (ItemKind::Text, Item::Text(t)) => p.texts[i] = t,
            (ItemKind::Image, Item::Image(im)) => p.images[i] = im,
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn argb_roundtrip() {
        let c = Color::rgba(1, 2, 3, 4);
        assert_eq!(Color::from_argb(c.to_argb()), c);
    }
    #[test]
    fn insert_remove() {
        let mut d = Document::new();
        let uid = d.alloc_uid();
        d.insert_item(0, 0, Item::Stroke(Stroke::new(uid, Color::rgb(0, 0, 0), 2.0)));
        assert!(d.find(0, uid).is_some());
        let (i, _) = d.remove_item(0, uid).unwrap();
        assert_eq!(i, 0);
        assert!(d.pages[0].is_empty());
    }
}
