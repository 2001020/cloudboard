//! `.cloudboard` project files.
//!
//! * v1 – written by CloudBoard 26.x (C++). UTF-16 strings, legacy coordinates.
//!   Read-only support; see `docs/legacy-ui-inventory.md` §8.
//! * v2 – written by CloudBoard 27+. Same block structure, UTF-8 strings,
//!   CRC32 per block, page-local coordinates and explicit page backgrounds.

use crate::assist::AssistState;
use crate::doc::{Color, Document, ImageItem, InkPoint, LineStyle, Page, PageBackground, PdfSource, ShapeKind, Stroke, TextItem};
use crate::geom::v2;
use std::sync::Arc;

pub const MAGIC: u32 = 0x4244_4C43; // "CLDB"
pub const ENDIAN: u32 = 0x0102_0304;
pub const VERSION_V1: u32 = 1;
pub const VERSION_V2: u32 = 2;
const COORD_SCALE: f32 = 100.0;

const BLK_META: u32 = 1;
const BLK_PDF: u32 = 2;
const BLK_PAGES: u32 = 3;
const BLK_ASSIST: u32 = 4;

/// Legacy blank-page units were physical pixels at "100%" zoom level 2.0.
pub const LEGACY_BOARD_SCALE: f32 = 2.0;

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("不是 CloudBoard 工程文件")]
    BadMagic,
    #[error("不支持的文件版本 {0}")]
    Version(u32),
    #[error("文件已损坏（{0}）")]
    Corrupt(&'static str),
    #[error("数据块校验失败")]
    Checksum,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewState {
    pub current_page: usize,
    pub zoom: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
}

/// Legacy PDF-mode files store annotations in absolute coordinates of the
/// vertically stacked page layout. The caller must convert them once page
/// sizes are known via [`fixup_legacy_pdf`].
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyPdfInfo {
    pub page_width_px: f32,
}

#[derive(Debug)]
pub struct Loaded {
    pub doc: Document,
    pub view: ViewState,
    pub assist: Option<AssistState>,
    pub legacy_pdf: Option<LegacyPdfInfo>,
    pub version: u32,
}

// ---------------------------------------------------------------- writer

#[derive(Default)]
struct W {
    b: Vec<u8>,
}
impl W {
    fn u8(&mut self, v: u8) {
        self.b.push(v)
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes())
    }
    fn i32(&mut self, v: i32) {
        self.b.extend_from_slice(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes())
    }
    fn f32(&mut self, v: f32) {
        self.b.extend_from_slice(&v.to_le_bytes())
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.b.extend_from_slice(s.as_bytes());
    }
    fn blob(&mut self, d: &[u8]) {
        self.u64(d.len() as u64);
        self.b.extend_from_slice(d);
    }
}

// ---------------------------------------------------------------- reader

struct R<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn new(d: &'a [u8]) -> R<'a> {
        R { d, p: 0 }
    }
    fn left(&self) -> usize {
        self.d.len() - self.p
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if self.left() < n {
            return Err(FormatError::Corrupt("意外的文件结尾"));
        }
        let s = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, FormatError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, FormatError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, FormatError> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
        Ok(if v.is_finite() { v } else { 0.0 })
    }
    fn count(&mut self, min_item: usize) -> Result<usize, FormatError> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_item.max(1)) > self.left() {
            return Err(FormatError::Corrupt("计数越界"));
        }
        Ok(n)
    }
    fn str_utf16(&mut self) -> Result<String, FormatError> {
        let n = self.count(2)?;
        let raw = self.take(n * 2)?;
        let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units))
    }
    fn str_utf8(&mut self) -> Result<String, FormatError> {
        let n = self.count(1)?;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn blob(&mut self) -> Result<Vec<u8>, FormatError> {
        let n = self.u64()?;
        if n > self.left() as u64 {
            return Err(FormatError::Corrupt("数据块越界"));
        }
        Ok(self.take(n as usize)?.to_vec())
    }
}

// ---------------------------------------------------------------- strokes

fn write_points(w: &mut W, pts: &[InkPoint]) {
    w.u32(pts.len() as u32);
    let (mut px, mut py) = (0i32, 0i32);
    for p in pts {
        let x = (p.x * COORD_SCALE).round() as i32;
        let y = (p.y * COORD_SCALE).round() as i32;
        w.i32(x.wrapping_sub(px));
        w.i32(y.wrapping_sub(py));
        w.u16(((p.pressure.clamp(0.0, 3.99)) * 1024.0).round() as u16);
        w.u16(p.dt_ms);
        px = x;
        py = y;
    }
}

fn read_points(r: &mut R, scale: f32) -> Result<Vec<InkPoint>, FormatError> {
    let n = r.count(12)?;
    let mut v = Vec::with_capacity(n);
    let (mut x, mut y) = (0i32, 0i32);
    for _ in 0..n {
        x = x.wrapping_add(r.i32()?);
        y = y.wrapping_add(r.i32()?);
        let pr = r.u16()?;
        let dt = r.u16()?;
        let pressure = if pr == 0 { 1.0 } else { (pr as f32 / 1024.0).min(4.0) };
        v.push(InkPoint { x: x as f32 / COORD_SCALE * scale, y: y as f32 / COORD_SCALE * scale, pressure, dt_ms: dt });
    }
    Ok(v)
}

fn write_stroke(w: &mut W, s: &Stroke) {
    w.u64(s.uid);
    w.u32(s.color.to_argb());
    w.f32(s.width);
    w.i32(s.style.to_i32());
    w.u32((s.highlighter as u32) | ((s.shape.is_some() as u32) << 1));
    w.i32(ShapeKind::to_i32(s.shape));
    w.f32(s.a.x);
    w.f32(s.a.y);
    w.f32(s.b.x);
    w.f32(s.b.y);
    write_points(w, &s.points);
}

fn read_stroke(r: &mut R, scale: f32) -> Result<Stroke, FormatError> {
    let uid = r.u64()?;
    let color = Color::from_argb(r.u32()?);
    let width = r.f32()? * scale;
    let style = LineStyle::from_i32(r.i32()?);
    let flags = r.u32()?;
    let shape_i = r.i32()?;
    let a = v2(r.f32()? * scale, r.f32()? * scale);
    let b = v2(r.f32()? * scale, r.f32()? * scale);
    let points = read_points(r, scale)?;
    let shape = if flags & 2 != 0 { ShapeKind::from_i32(shape_i) } else { None };
    Ok(Stroke { uid, points, color, width: width.max(0.1), style, highlighter: flags & 1 != 0, shape, a, b })
}

// ---------------------------------------------------------------- v2 write

fn block(out: &mut W, ty: u32, payload: &[u8]) {
    out.u32(ty);
    out.u64(payload.len() as u64);
    out.u32(crc32fast::hash(payload));
    out.b.extend_from_slice(payload);
}

/// Serialize a document to the v2 format.
pub fn save(doc: &Document, view: &ViewState, assist: Option<&AssistState>, app_version: &str) -> Vec<u8> {
    let mut out = W::default();
    out.u32(MAGIC);
    out.u32(VERSION_V2);
    out.u32(ENDIAN);

    let mut m = W::default();
    m.str(app_version);
    m.u8(doc.pdf.is_some() as u8);
    m.u32(view.current_page as u32);
    m.f32(view.zoom);
    m.f32(view.scroll_x);
    m.f32(view.scroll_y);
    m.u64(doc.next_uid);
    block(&mut out, BLK_META, &m.b);

    if let Some(pdf) = &doc.pdf {
        let mut p = W::default();
        p.str(&pdf.file_name);
        p.blob(&pdf.bytes);
        block(&mut out, BLK_PDF, &p.b);
    }

    let mut p = W::default();
    p.u32(doc.pages.len() as u32);
    for pg in &doc.pages {
        match pg.background {
            PageBackground::Blank => {
                p.u8(0);
                p.u32(0);
                p.f32(0.0);
                p.f32(0.0);
            }
            PageBackground::Pdf { index, width, height } => {
                p.u8(1);
                p.u32(index);
                p.f32(width);
                p.f32(height);
            }
        }
        p.u32(pg.strokes.len() as u32);
        for s in &pg.strokes {
            write_stroke(&mut p, s);
        }
        p.u32(pg.texts.len() as u32);
        for t in &pg.texts {
            p.u64(t.uid);
            p.str(&t.text);
            p.f32(t.pos.x);
            p.f32(t.pos.y);
            p.u32(t.color.to_argb());
            p.f32(t.font_size);
            p.str(&t.family);
            p.u8(t.bold as u8);
        }
        p.u32(pg.images.len() as u32);
        for im in &pg.images {
            p.u64(im.uid);
            p.f32(im.pos.x);
            p.f32(im.pos.y);
            p.f32(im.w);
            p.f32(im.h);
            p.f32(im.rotate_deg);
            p.blob(&im.data);
        }
    }
    block(&mut out, BLK_PAGES, &p.b);

    if let Some(a) = assist {
        let mut w = W::default();
        w.str(&serde_json::to_string(a).unwrap_or_default());
        block(&mut out, BLK_ASSIST, &w.b);
    }
    out.b
}

// ---------------------------------------------------------------- load

pub fn load(data: &[u8]) -> Result<Loaded, FormatError> {
    let mut r = R::new(data);
    if r.u32().map_err(|_| FormatError::BadMagic)? != MAGIC {
        return Err(FormatError::BadMagic);
    }
    let version = r.u32()?;
    if r.u32()? != ENDIAN {
        return Err(FormatError::Corrupt("字节序标记"));
    }
    match version {
        VERSION_V1 => load_v1(r),
        VERSION_V2 => load_v2(r),
        v => Err(FormatError::Version(v)),
    }
}

fn load_v2(mut r: R) -> Result<Loaded, FormatError> {
    let mut doc = Document { pages: vec![], ..Document::default() };
    let mut view = ViewState { zoom: 1.0, ..Default::default() };
    let mut assist = None;
    let mut have_pages = false;
    while r.left() > 0 {
        let ty = r.u32()?;
        let len = r.u64()?;
        let crc = r.u32()?;
        if len > r.left() as u64 {
            return Err(FormatError::Corrupt("数据块长度"));
        }
        let payload = r.take(len as usize)?;
        if crc32fast::hash(payload) != crc {
            return Err(FormatError::Checksum);
        }
        let mut b = R::new(payload);
        match ty {
            BLK_META => {
                let _ver = b.str_utf8()?;
                let _pdf = b.u8()?;
                view.current_page = b.u32()? as usize;
                view.zoom = b.f32()?;
                view.scroll_x = b.f32()?;
                view.scroll_y = b.f32()?;
                doc.next_uid = b.u64()?;
            }
            BLK_PDF => {
                let file_name = b.str_utf8()?;
                let bytes = b.blob()?;
                doc.pdf = Some(PdfSource { file_name, bytes: Arc::new(bytes) });
            }
            BLK_PAGES => {
                let n = b.count(13)?;
                for _ in 0..n {
                    let kind = b.u8()?;
                    let index = b.u32()?;
                    let (w, h) = (b.f32()?, b.f32()?);
                    let background = if kind == 1 { PageBackground::Pdf { index, width: w, height: h } } else { PageBackground::Blank };
                    let mut pg = Page { background, ..Default::default() };
                    read_items(&mut b, &mut pg, 1.0, false)?;
                    doc.pages.push(pg);
                }
                have_pages = true;
            }
            BLK_ASSIST => {
                let s = b.str_utf8()?;
                assist = serde_json::from_str(&s).ok();
            }
            _ => {}
        }
    }
    if !have_pages {
        return Err(FormatError::Corrupt("缺少页面数据"));
    }
    finish(&mut doc, &mut view);
    Ok(Loaded { doc, view, assist, legacy_pdf: None, version: VERSION_V2 })
}

fn read_items(b: &mut R, pg: &mut Page, scale: f32, utf16: bool) -> Result<(), FormatError> {
    let ns = b.count(56)?;
    for _ in 0..ns {
        pg.strokes.push(read_stroke(b, scale)?);
    }
    let nt = b.count(20)?;
    for _ in 0..nt {
        let uid = b.u64()?;
        let text = if utf16 { b.str_utf16()? } else { b.str_utf8()? };
        let pos = v2(b.f32()? * scale, b.f32()? * scale);
        let color = Color::from_argb(b.u32()?);
        let font_size = b.f32()? * scale;
        let family = if utf16 { b.str_utf16()? } else { b.str_utf8()? };
        let bold = b.u8()? != 0;
        pg.texts.push(TextItem { uid, text, pos, color, font_size: font_size.max(4.0), family, bold });
    }
    let ni = b.count(36)?;
    for _ in 0..ni {
        let uid = b.u64()?;
        let pos = v2(b.f32()? * scale, b.f32()? * scale);
        let (w, h) = (b.f32()? * scale, b.f32()? * scale);
        let rotate_deg = b.f32()?;
        let data = b.blob()?;
        pg.images.push(ImageItem { uid, pos, w, h, rotate_deg, data: Arc::new(data) });
    }
    Ok(())
}

fn load_v1(mut r: R) -> Result<Loaded, FormatError> {
    let mut doc = Document { pages: vec![], ..Document::default() };
    let mut view = ViewState { zoom: 1.0, ..Default::default() };
    let mut assist = None;
    let mut pdf_mode = false;
    let mut page_width_px = 0.0;
    let mut pages_payload: Option<&[u8]> = None;
    while r.left() > 0 {
        let ty = r.u32()?;
        let len = r.u64()?;
        if len > r.left() as u64 {
            return Err(FormatError::Corrupt("数据块长度"));
        }
        let payload = r.take(len as usize)?;
        let mut b = R::new(payload);
        match ty {
            BLK_META => {
                let _ver = b.str_utf16()?;
                let _path = b.str_utf16()?;
                pdf_mode = b.u8()? != 0;
                view.current_page = b.i32()?.max(0) as usize;
                let zoom = b.f32()?;
                view.zoom = (zoom / 2.0).clamp(0.25, 3.5);
                view.scroll_x = b.f32()?;
                view.scroll_y = b.f32()?;
                doc.next_uid = b.u64()?;
                page_width_px = b.f32().unwrap_or(0.0);
            }
            BLK_PDF => {
                let path = b.str_utf16()?;
                let bytes = b.blob()?;
                let file_name = path.rsplit(['\\', '/']).next().unwrap_or("document.pdf").to_string();
                doc.pdf = Some(PdfSource { file_name, bytes: Arc::new(bytes) });
            }
            BLK_PAGES => pages_payload = Some(payload),
            BLK_ASSIST => {
                let _kind = b.i32()?;
                let tk = b.i32()?;
                let mut vals = [0f32; 8];
                for v in &mut vals {
                    *v = b.f32()?;
                }
                let _ = tk;
                // Positions are in legacy units; tools start hidden anyway.
                assist = Some(AssistState::default());
            }
            _ => {}
        }
    }
    let pages = pages_payload.ok_or(FormatError::Corrupt("缺少页面数据"))?;
    let pdf_mode = pdf_mode && doc.pdf.is_some();
    let scale = if pdf_mode { 1.0 } else { LEGACY_BOARD_SCALE };
    let mut b = R::new(pages);
    let n = b.count(12)?;
    for _ in 0..n {
        let mut pg = Page::default();
        read_items(&mut b, &mut pg, scale, true)?;
        doc.pages.push(pg);
    }
    if !pdf_mode {
        view.scroll_x *= 1.0; // legacy scroll was in screen pixels; keep as is
    }
    finish(&mut doc, &mut view);
    Ok(Loaded {
        doc,
        view,
        assist,
        legacy_pdf: if pdf_mode { Some(LegacyPdfInfo { page_width_px }) } else { None },
        version: VERSION_V1,
    })
}

fn finish(doc: &mut Document, view: &mut ViewState) {
    if doc.pages.is_empty() {
        doc.pages.push(Page::default());
    }
    let max_uid = doc
        .pages
        .iter()
        .flat_map(|p| p.strokes.iter().map(|s| s.uid).chain(p.texts.iter().map(|t| t.uid)).chain(p.images.iter().map(|i| i.uid)))
        .max()
        .unwrap_or(0);
    doc.next_uid = doc.next_uid.max(max_uid + 1);
    // Ensure unique uids (legacy files may contain 0).
    let mut seen = std::collections::HashSet::new();
    let mut next = doc.next_uid;
    for p in &mut doc.pages {
        for s in &mut p.strokes {
            if s.uid == 0 || !seen.insert(s.uid) {
                s.uid = next;
                next += 1;
                seen.insert(s.uid);
            }
        }
        for t in &mut p.texts {
            if t.uid == 0 || !seen.insert(t.uid) {
                t.uid = next;
                next += 1;
                seen.insert(t.uid);
            }
        }
        for i in &mut p.images {
            if i.uid == 0 || !seen.insert(i.uid) {
                i.uid = next;
                next += 1;
                seen.insert(i.uid);
            }
        }
    }
    doc.next_uid = next;
    view.current_page = view.current_page.min(doc.pages.len() - 1);
    if !(view.zoom > 0.0) {
        view.zoom = 1.0;
    }
}

/// Convert legacy PDF-mode annotations (absolute stacked coordinates) into
/// page-local coordinates. `page_sizes_pt` are the PDF page sizes in points.
/// Returns new pages with PDF backgrounds (unit = pt × 96/72).
pub fn fixup_legacy_pdf(doc: &mut Document, info: &LegacyPdfInfo, page_sizes_pt: &[(f32, f32)]) {
    if page_sizes_pt.is_empty() {
        return;
    }
    let w0 = page_sizes_pt[0].0.max(1.0);
    let px_per_pt = if info.page_width_px > 0.0 { info.page_width_px / w0 } else { 96.0 / 72.0 };
    let k = (96.0 / 72.0) / px_per_pt;
    let mut offsets = Vec::with_capacity(page_sizes_pt.len());
    let mut y = 0.0;
    for (_, h) in page_sizes_pt {
        offsets.push(y);
        y += h * px_per_pt;
    }
    // Legacy kept per-page lists; items already belong to their page index.
    let old = std::mem::take(&mut doc.pages);
    let mut pages: Vec<Page> = page_sizes_pt
        .iter()
        .enumerate()
        .map(|(i, (w, h))| Page { background: PageBackground::Pdf { index: i as u32, width: w * 96.0 / 72.0, height: h * 96.0 / 72.0 }, ..Default::default() })
        .collect();
    for (i, mut pg) in old.into_iter().enumerate() {
        let Some(dst) = pages.get_mut(i) else { break };
        let off = offsets[i];
        let map = |x: f32, y: f32| v2(x * k, (y - off) * k);
        for s in &mut pg.strokes {
            for p in &mut s.points {
                let q = map(p.x, p.y);
                p.x = q.x;
                p.y = q.y;
            }
            s.a = map(s.a.x, s.a.y);
            s.b = map(s.b.x, s.b.y);
            s.width *= k;
        }
        for t in &mut pg.texts {
            t.pos = map(t.pos.x, t.pos.y);
            t.font_size *= k;
        }
        for im in &mut pg.images {
            im.pos = map(im.pos.x, im.pos.y);
            im.w *= k;
            im.h *= k;
        }
        dst.strokes = pg.strokes;
        dst.texts = pg.texts;
        dst.images = pg.images;
    }
    doc.pages = pages;
    doc.touch();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        let mut d = Document::new();
        let mut s = Stroke::new(d.alloc_uid(), Color::rgb(200, 10, 10), 4.0);
        s.points = vec![InkPoint::new(1.25, 2.5), InkPoint { x: 10.0, y: -3.0, pressure: 0.5, dt_ms: 7 }];
        s.style = LineStyle::Dashed;
        d.pages[0].strokes.push(s);
        let mut sh = Stroke::new(d.alloc_uid(), Color::rgb(0, 0, 0), 2.0);
        sh.shape = Some(ShapeKind::Ellipse);
        sh.a = v2(0.0, 0.0);
        sh.b = v2(50.0, 30.0);
        d.pages[0].strokes.push(sh);
        let uid = d.alloc_uid();
        d.pages[0].texts.push(TextItem { uid, text: "你好 CloudBoard\n第二行".into(), pos: v2(5.0, 6.0), color: Color::rgb(1, 2, 3), font_size: 36.0, family: "Sans".into(), bold: true });
        let uid = d.alloc_uid();
        d.pages.push(Page::default());
        d.pages[1].images.push(ImageItem { uid, pos: v2(1.0, 1.0), w: 10.0, h: 20.0, rotate_deg: 0.0, data: Arc::new(vec![1, 2, 3]) });
        d
    }

    #[test]
    fn v2_roundtrip() {
        let d = sample();
        let view = ViewState { current_page: 1, zoom: 1.5, scroll_x: 3.0, scroll_y: 4.0 };
        let bytes = save(&d, &view, Some(&AssistState::default()), "27.0.0");
        let l = load(&bytes).unwrap();
        assert_eq!(l.version, 2);
        assert_eq!(l.view, view);
        assert_eq!(l.doc.pages.len(), 2);
        assert_eq!(l.doc.pages[0].texts, d.pages[0].texts);
        assert_eq!(l.doc.pages[1].images, d.pages[1].images);
        let s = &l.doc.pages[0].strokes[0];
        assert_eq!(s.points[0], d.pages[0].strokes[0].points[0]);
        assert!((s.points[1].pressure - 0.5).abs() < 1e-3);
        assert_eq!(l.doc.pages[0].strokes[1].shape, Some(ShapeKind::Ellipse));
    }

    #[test]
    fn checksum_detects_corruption() {
        let mut bytes = save(&sample(), &ViewState::default(), None, "27.0.0");
        let n = bytes.len();
        bytes[n - 3] ^= 0xFF;
        assert!(matches!(load(&bytes), Err(FormatError::Checksum)));
        assert!(matches!(load(b"nope"), Err(FormatError::BadMagic)));
        assert!(load(&bytes[..20]).is_err());
    }

    /// Build a v1 file the way CloudBoard 26 wrote it.
    fn legacy_v1() -> Vec<u8> {
        fn s16(w: &mut W, s: &str) {
            let u: Vec<u16> = s.encode_utf16().collect();
            w.u32(u.len() as u32);
            for c in u {
                w.u16(c);
            }
        }
        let mut f = W::default();
        f.u32(MAGIC);
        f.u32(1);
        f.u32(ENDIAN);
        let mut m = W::default();
        s16(&mut m, "26.2.0");
        s16(&mut m, "");
        m.u8(0);
        m.i32(0);
        m.f32(2.0);
        m.f32(0.0);
        m.f32(0.0);
        m.u64(10);
        m.f32(0.0);
        m.f32(0.0);
        s16(&mut m, "user");
        f.u32(BLK_META);
        f.u64(m.b.len() as u64);
        f.b.extend_from_slice(&m.b);
        let mut p = W::default();
        p.u32(1);
        p.u32(1); // strokes
        p.u64(3);
        p.u32(0xFF1E1E1E);
        p.f32(4.0);
        p.i32(0);
        p.u32(0);
        p.i32(5);
        for _ in 0..4 {
            p.f32(0.0);
        }
        p.u32(2);
        p.i32(1000);
        p.i32(2000);
        p.u16(1024);
        p.u16(0);
        p.i32(500);
        p.i32(-100);
        p.u16(1024);
        p.u16(16);
        p.u32(1); // texts
        p.u64(4);
        s16(&mut p, "中文");
        p.f32(10.0);
        p.f32(20.0);
        p.u32(0xFF000000);
        p.f32(36.0);
        s16(&mut p, "Anthropic Sans Text");
        p.u8(0);
        p.u32(0); // images
        f.u32(BLK_PAGES);
        f.u64(p.b.len() as u64);
        f.b.extend_from_slice(&p.b);
        f.b
    }

    #[test]
    fn reads_legacy_v1() {
        let l = load(&legacy_v1()).unwrap();
        assert_eq!(l.version, 1);
        assert_eq!(l.view.zoom, 1.0);
        let s = &l.doc.pages[0].strokes[0];
        assert_eq!(s.points.len(), 2);
        assert!((s.points[0].x - 20.0).abs() < 1e-4 && (s.points[1].x - 30.0).abs() < 1e-4);
        assert!((s.points[1].y - 38.0).abs() < 1e-4);
        assert_eq!(s.width, 8.0);
        assert_eq!(l.doc.pages[0].texts[0].text, "中文");
        assert_eq!(l.doc.pages[0].texts[0].font_size, 72.0);
        assert!(l.doc.next_uid >= 10);
    }

    #[test]
    fn legacy_pdf_fixup() {
        let mut d = Document::new();
        d.pages.push(Page::default());
        let mut s = Stroke::new(1, Color::rgb(0, 0, 0), 2.0);
        // page width 600pt saved as 800px -> px_per_pt = 4/3 (== 96/72): k = 1
        s.points = vec![InkPoint::new(100.0, 800.0 * 4.0 / 3.0 + 50.0)];
        d.pages[1].strokes.push(s);
        fixup_legacy_pdf(&mut d, &LegacyPdfInfo { page_width_px: 800.0 }, &[(600.0, 800.0), (600.0, 800.0)]);
        let p = d.pages[1].strokes[0].points[0];
        assert!((p.x - 100.0).abs() < 1e-3 && (p.y - 50.0).abs() < 1e-3);
        assert!(matches!(d.pages[1].background, PageBackground::Pdf { index: 1, .. }));
    }
}
