//! PowerPoint (.pptx) and Word (.docx) rendering without Office: the
//! OOXML package is parsed into a display model (`model`) that is laid out
//! with cosmic-text and drawn with tiny-skia.

pub mod anim;
pub mod dml;
pub mod docx;
pub mod geom;
pub mod model;
pub mod opc;
pub mod pptx;
pub mod render;
pub mod source;
pub mod text;

pub use model::{Document, Effect, Page, Step};
pub use render::{Filter, Renderer};

pub use text::FontMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pptx,
    Docx,
}

/// Office format for a file name, if supported.
pub fn format_for(name: &str) -> Option<Format> {
    let l = name.to_ascii_lowercase();
    if l.ends_with(".pptx") || l.ends_with(".ppsx") || l.ends_with(".pptm") {
        Some(Format::Pptx)
    } else if l.ends_with(".docx") || l.ends_with(".docm") {
        Some(Format::Docx)
    } else {
        None
    }
}

/// Old binary formats we cannot read (the user is asked to save as .pptx/.docx).
pub fn is_legacy_binary(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.ends_with(".ppt") || l.ends_with(".pps") || l.ends_with(".doc")
}

/// Parse and lay out a document.
pub fn load(bytes: &[u8], format: Format, fs: &mut cosmic_text::FontSystem, map: &mut FontMap) -> Result<Document, String> {
    match format {
        Format::Pptx => pptx::load(bytes, fs, map),
        Format::Docx => docx::load(bytes, fs, map),
    }
}

/// Render a whole page at `scale` device pixels per point.
pub fn render_page(r: &mut Renderer, fs: &mut cosmic_text::FontSystem, page: &Page, scale: f32) -> Option<tiny_skia::Pixmap> {
    let (w, h) = ((page.width * scale).ceil().max(1.0) as u32, (page.height * scale).ceil().max(1.0) as u32);
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    pm.fill(tiny_skia::Color::WHITE);
    r.draw_page(fs, page, &mut pm.as_mut(), tiny_skia::Transform::from_scale(scale, scale), &Filter { background: true, ..Default::default() });
    Some(pm)
}

/// Detect the format of an Office package from its contents.
pub fn sniff(bytes: &[u8]) -> Option<Format> {
    let z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let names: Vec<&str> = z.file_names().collect();
    if names.iter().any(|n| n.starts_with("ppt/")) {
        Some(Format::Pptx)
    } else if names.iter().any(|n| n.starts_with("word/")) {
        Some(Format::Docx)
    } else {
        None
    }
}
