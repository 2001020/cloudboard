//! Decoded image cache for ImageItems (keyed by uid).

use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::{Pixmap, PixmapPaint, PixmapMut, Transform};

#[derive(Default)]
pub struct ImageCache {
    map: HashMap<u64, Option<Arc<Pixmap>>>,
}

/// Decode encoded bytes into a premultiplied pixmap.
pub fn decode(bytes: &[u8]) -> Option<Pixmap> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut pm = Pixmap::new(w, h)?;
    for (dst, src) in pm.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pm)
}

/// Encode RGBA (non-premultiplied) to PNG.
pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let enc = image::codecs::png::PngEncoder::new(&mut out);
        use image::ImageEncoder;
        enc.write_image(rgba, w, h, image::ExtendedColorType::Rgba8).ok()?;
    }
    Some(out)
}

/// Pixmap (premultiplied) → PNG bytes.
pub fn pixmap_to_png(pm: &Pixmap) -> Option<Vec<u8>> {
    let mut rgba = Vec::with_capacity(pm.data().len());
    for p in pm.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    encode_png(pm.width(), pm.height(), &rgba)
}

impl ImageCache {
    pub fn get(&mut self, uid: u64, bytes: &[u8]) -> Option<Arc<Pixmap>> {
        self.map.entry(uid).or_insert_with(|| decode(bytes).map(Arc::new)).clone()
    }
    pub fn retain(&mut self, live: &std::collections::HashSet<u64>) {
        self.map.retain(|k, _| live.contains(k));
    }
}

/// Draw `img` into the rectangle (x, y, w, h) under transform `t`.
pub fn draw_image(pm: &mut PixmapMut, img: &Pixmap, x: f32, y: f32, w: f32, h: f32, rotate_deg: f32, t: Transform) {
    let sx = w / img.width() as f32;
    let sy = h / img.height() as f32;
    let mut tr = t.pre_translate(x, y);
    if rotate_deg.abs() > 0.01 {
        tr = tr.pre_translate(w * 0.5, h * 0.5).pre_rotate(rotate_deg).pre_translate(-w * 0.5, -h * 0.5);
    }
    let tr = tr.pre_scale(sx, sy);
    let paint = PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..Default::default() };
    pm.draw_pixmap(0, 0, img.as_ref(), &paint, tr, None);
}
