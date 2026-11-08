//! Office documents as a page source for the tiled page engine (the same
//! thread, cache and scheduling as PDFs).

use crate::{load, sniff, Document, Filter, FontMap, Renderer};
use cb_pdf::{PageSource, PdfError};
use cosmic_text::FontSystem;
use std::sync::Arc;
use tiny_skia::{Pixmap, Transform};

pub struct OfficeSource {
    fs: FontSystem,
    map: FontMap,
    renderer: Renderer,
    doc: Option<Document>,
}

impl OfficeSource {
    pub fn new() -> OfficeSource {
        OfficeSource { fs: FontSystem::new(), map: FontMap::new(), renderer: Renderer::new(), doc: None }
    }
}

impl Default for OfficeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl PageSource for OfficeSource {
    fn open(&mut self, bytes: Arc<Vec<u8>>) -> Result<Vec<(f32, f32)>, PdfError> {
        let fmt = sniff(&bytes).ok_or_else(|| PdfError::Other("不是有效的 PowerPoint / Word 文件".into()))?;
        let doc = load(&bytes, fmt, &mut self.fs, &mut self.map).map_err(PdfError::Other)?;
        let sizes = doc.pages.iter().map(|p| (p.width, p.height)).collect();
        self.doc = Some(doc);
        Ok(sizes)
    }

    fn close(&mut self) {
        self.doc = None;
    }

    fn render(&mut self, page: u32, scale: f32, x0: u32, y0: u32, w: u32, h: u32) -> Option<Pixmap> {
        let p = self.doc.as_ref()?.pages.get(page as usize)?;
        let mut pm = Pixmap::new(w, h)?;
        pm.fill(tiny_skia::Color::WHITE);
        let base = Transform::from_row(scale, 0.0, 0.0, scale, -(x0 as f32), -(y0 as f32));
        self.renderer.draw_page(&mut self.fs, p, &mut pm.as_mut(), base, &Filter { background: true, ..Default::default() });
        Some(pm)
    }
}
