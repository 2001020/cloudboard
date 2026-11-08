//! Text shaping (cosmic-text, with system CJK fallback) and glyph blitting
//! into tiny-skia pixmaps.

use cb_core::Color;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Weight, Wrap};
use std::collections::HashMap;
use tiny_skia::PixmapMut;

/// Font family name used by CloudBoard 26 / early 27 builds for text items;
/// treated as "the UI font" when loading old documents.
pub const LEGACY_UI_FAMILY: &str = "Anthropic Sans Text";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in physical pixels.
    pub size: f32,
    pub weight: u16,
    pub color: Color,
    pub align: Align,
}

impl TextStyle {
    pub fn new(size: f32, color: Color) -> TextStyle {
        TextStyle { size, weight: 400, color, align: Align::Left }
    }
    pub fn bold(mut self) -> Self {
        self.weight = 700;
        self
    }
    pub fn weight(mut self, w: u16) -> Self {
        self.weight = w;
        self
    }
    pub fn center(mut self) -> Self {
        self.align = Align::Center;
        self
    }
    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct Key {
    text: String,
    size: u32,
    weight: u16,
    family: Option<String>,
}

struct Laid {
    buffer: Buffer,
    width: f32,
    height: f32,
    used: u64,
}

pub struct Fonts {
    pub fs: FontSystem,
    swash: SwashCache,
    layouts: HashMap<Key, Laid>,
    clock: u64,
    ui_family: Option<String>,
}

impl Fonts {
    /// Create with the system fonts. The UI uses the first of `preferred`
    /// that is installed (normally the OS UI font); otherwise the system
    /// sans-serif. CJK text falls back to system fonts automatically.
    pub fn new(preferred: &[String]) -> Fonts {
        let fs = FontSystem::new();
        let ui_family = preferred.iter().find(|name| fs.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))).cloned();
        Fonts { fs, swash: SwashCache::new(), layouts: HashMap::new(), clock: 0, ui_family }
    }

    /// The family used for UI text, if a preferred one was found.
    pub fn ui_family(&self) -> Option<&str> {
        self.ui_family.as_deref()
    }

    fn layout(&mut self, text: &str, size: f32, weight: u16, family: Option<&str>) -> &Laid {
        self.clock += 1;
        let key = Key { text: text.to_string(), size: (size * 16.0).round() as u32, weight, family: family.map(|s| s.to_string()) };
        if !self.layouts.contains_key(&key) {
            if self.layouts.len() > 600 {
                // Drop the least recently used half.
                let mut v: Vec<(u64, Key)> = self.layouts.iter().map(|(k, l)| (l.used, k.clone())).collect();
                v.sort_by_key(|x| x.0);
                for (_, k) in v.into_iter().take(300) {
                    self.layouts.remove(&k);
                }
            }
            let size = size.max(1.0);
            let mut buffer = Buffer::new(&mut self.fs, Metrics::new(size, (size * 1.28).ceil()));
            buffer.set_wrap(&mut self.fs, Wrap::None);
            buffer.set_size(&mut self.fs, None, None);
            let ui = self.ui_family.clone();
            let fam = match (family, ui.as_deref()) {
                (Some(f), _) => Family::Name(f),
                (None, Some(u)) => Family::Name(u),
                (None, None) => Family::SansSerif,
            };
            let attrs = Attrs::new().family(fam).weight(Weight(weight));
            buffer.set_text(&mut self.fs, text, attrs, Shaping::Advanced);
            buffer.shape_until_scroll(&mut self.fs, false);
            let mut width: f32 = 0.0;
            let mut height: f32 = 0.0;
            for run in buffer.layout_runs() {
                width = width.max(run.line_w);
                height = height.max(run.line_top + run.line_height);
            }
            self.layouts.insert(key.clone(), Laid { buffer, width, height, used: 0 });
        }
        let clock = self.clock;
        let l = self.layouts.get_mut(&key).unwrap();
        l.used = clock;
        l
    }

    /// Measure text (physical px): (width, height).
    pub fn measure(&mut self, text: &str, style: TextStyle) -> (f32, f32) {
        let l = self.layout(text, style.size, style.weight, None);
        (l.width, l.height)
    }

    pub fn measure_family(&mut self, text: &str, size: f32, weight: u16, family: &str) -> (f32, f32) {
        let l = self.layout(text, size, weight, Some(family));
        (l.width, l.height)
    }

    /// Draw text with its top edge at `y`; x is the left/centre/right anchor.
    pub fn draw(&mut self, pm: &mut PixmapMut, text: &str, x: f32, y: f32, style: TextStyle) -> (f32, f32) {
        self.draw_family(pm, text, x, y, style, None)
    }

    /// Draw text vertically centred on `cy`.
    pub fn draw_vcenter(&mut self, pm: &mut PixmapMut, text: &str, x: f32, cy: f32, style: TextStyle) -> (f32, f32) {
        let (_, h) = self.measure(text, style);
        self.draw(pm, text, x, cy - h * 0.5, style)
    }

    pub fn draw_family(&mut self, pm: &mut PixmapMut, text: &str, x: f32, y: f32, style: TextStyle, family: Option<&str>) -> (f32, f32) {
        if text.is_empty() {
            return (0.0, 0.0);
        }
        // Split borrow: take layout out temporarily.
        let (w, h) = {
            let l = self.layout(text, style.size, style.weight, family);
            (l.width, l.height)
        };
        let ox = match style.align {
            Align::Left => x,
            Align::Center => x - w * 0.5,
            Align::Right => x - w,
        };
        let key = Key { text: text.to_string(), size: (style.size.max(1.0) * 16.0).round() as u32, weight: style.weight, family: family.map(|s| s.to_string()) };
        let Some(laid) = self.layouts.get(&key) else { return (w, h) };
        let (pw, ph) = (pm.width() as i32, pm.height() as i32);
        let data = pm.data_mut();
        for run in laid.buffer.layout_runs() {
            for g in run.glyphs.iter() {
                let pg = g.physical((ox, y), 1.0);
                let Some(img) = self.swash.get_image(&mut self.fs, pg.cache_key) else { continue };
                let gx = pg.x + img.placement.left;
                let gy = run.line_y.round() as i32 + pg.y - img.placement.top;
                let (iw, ih) = (img.placement.width as i32, img.placement.height as i32);
                match img.content {
                    SwashContent::Mask => {
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
                                let a = img.data[(yy * iw + xx) as usize];
                                if a == 0 {
                                    continue;
                                }
                                blend(data, ((py * pw + px) * 4) as usize, style.color, a);
                            }
                        }
                    }
                    SwashContent::Color => {
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
                                let i = ((yy * iw + xx) * 4) as usize;
                                let c = Color::rgba(img.data[i], img.data[i + 1], img.data[i + 2], 255);
                                let a = img.data[i + 3];
                                if a > 0 {
                                    blend(data, ((py * pw + px) * 4) as usize, c, a);
                                }
                            }
                        }
                    }
                    SwashContent::SubpixelMask => {}
                }
            }
        }
        (w, h)
    }
}

/// Source-over blend of `c` with coverage `cov` into premultiplied RGBA8.
#[inline]
fn blend(data: &mut [u8], i: usize, c: Color, cov: u8) {
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
