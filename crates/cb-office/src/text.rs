//! Paragraph layout with cosmic-text in point units. Runs pick the East
//! Asian or Latin typeface per character; missing fonts are mapped to
//! installed equivalents (e.g. 宋体 → Songti SC on macOS).

use crate::model::{Rgba, SpanDeco, TextBlock};
use cosmic_text::{Attrs, AttrsOwned, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight, Wrap};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub struct RunStyle {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Rgba,
    pub latin: Option<String>,
    pub ea: Option<String>,
    pub underline: bool,
    pub strike: bool,
    pub highlight: Option<Rgba>,
    /// Superscript (> 0) / subscript (< 0) shift as a fraction of the size.
    pub baseline: f32,
}

impl Default for RunStyle {
    fn default() -> Self {
        RunStyle { size: 18.0, bold: false, italic: false, color: Rgba::BLACK, latin: None, ea: None, underline: false, strike: false, highlight: None, baseline: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spacing {
    /// Multiple of the font's natural line height.
    Multiple(f32),
    Exact(f32),
    AtLeast(f32),
}

#[derive(Debug, Clone)]
pub struct Para {
    pub runs: Vec<(String, RunStyle)>,
    /// Style used for an empty paragraph's height.
    pub end_style: RunStyle,
    pub align: Align,
    pub spacing: Spacing,
    pub before: f32,
    pub after: f32,
    pub left: f32,
    pub right: f32,
    /// First-line offset relative to `left` (negative = hanging).
    pub first: f32,
    pub bullet: Option<(String, RunStyle)>,
    /// Line pitch of a document grid (Word "snap to grid"), if any.
    pub grid: Option<f32>,
    /// Natural line height as a multiple of the font size.
    pub natural: f32,
}

impl Default for Para {
    fn default() -> Self {
        Para { runs: Vec::new(), end_style: RunStyle::default(), align: Align::Left, spacing: Spacing::Multiple(1.0), before: 0.0, after: 0.0, left: 0.0, right: 0.0, first: 0.0, bullet: None, grid: None, natural: 1.2 }
    }
}

/// Maps requested family names to installed ones.
pub struct FontMap {
    cache: HashMap<String, Option<String>>,
}

/// Equivalents tried when a requested family is missing.
const SUBSTITUTES: &[(&[&str], &[&str])] = &[
    (&["宋体", "simsun", "新宋体", "nsimsun", "宋体-简"], &["SimSun", "Songti SC", "STSong", "Noto Serif CJK SC", "Source Han Serif SC", "AR PL UMing CN"]),
    (&["黑体", "simhei"], &["SimHei", "Heiti SC", "PingFang SC", "Noto Sans CJK SC", "WenQuanYi Zen Hei"]),
    (&["微软雅黑", "microsoft yahei", "microsoft yahei ui"], &["Microsoft YaHei", "PingFang SC", "Hiragino Sans GB", "Noto Sans CJK SC", "WenQuanYi Zen Hei"]),
    (&["等线", "dengxian", "等线 light", "dengxian light"], &["DengXian", "PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", "WenQuanYi Zen Hei"]),
    (&["楷体", "kaiti", "楷体_gb2312", "kaiti_gb2312", "华文楷体", "stkaiti"], &["KaiTi", "STKaiti", "Kaiti SC", "AR PL UKai CN", "Noto Serif CJK SC"]),
    (&["仿宋", "fangsong", "仿宋_gb2312", "fangsong_gb2312", "华文仿宋", "stfangsong"], &["FangSong", "STFangsong", "Songti SC", "Noto Serif CJK SC"]),
    (&["calibri", "calibri light", "arial", "helvetica", "segoe ui", "verdana", "tahoma"], &["Calibri", "Arial", "Helvetica Neue", "Helvetica", "Liberation Sans", "DejaVu Sans"]),
    (&["times new roman", "cambria", "georgia"], &["Times New Roman", "Cambria", "Times", "Liberation Serif", "DejaVu Serif"]),
    (&["consolas", "courier new"], &["Consolas", "Courier New", "Menlo", "Liberation Mono", "DejaVu Sans Mono"]),
];

impl Default for FontMap {
    fn default() -> Self {
        Self::new()
    }
}

impl FontMap {
    pub fn new() -> FontMap {
        FontMap { cache: HashMap::new() }
    }

    fn installed(fs: &FontSystem, name: &str) -> bool {
        fs.db().faces().any(|f| f.families.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)))
    }

    pub fn resolve(&mut self, fs: &FontSystem, name: &str) -> Option<String> {
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(v) = self.cache.get(&key) {
            return v.clone();
        }
        let found = if Self::installed(fs, name.trim()) {
            Some(name.trim().to_string())
        } else {
            SUBSTITUTES.iter().find(|(names, _)| names.contains(&key.as_str())).and_then(|(_, subs)| subs.iter().find(|s| Self::installed(fs, s)).map(|s| s.to_string()))
        };
        self.cache.insert(key, found.clone());
        found
    }
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// A laid-out paragraph.
pub struct Laid {
    pub buf: Arc<Buffer>,
    pub spans: Arc<Vec<SpanDeco>>,
    /// Top of each line (relative to the buffer) and its height.
    pub lines: Vec<(f32, f32)>,
    pub height: f32,
    pub bullet: Option<(Arc<Buffer>, Arc<Vec<SpanDeco>>, f32, f32)>,
}

impl Laid {
    /// Text blocks for lines [l0, l1) placed with the buffer origin at (x, y).
    pub fn blocks(&self, l0: usize, l1: usize, x: f32, y: f32) -> Vec<TextBlock> {
        let mut v = vec![TextBlock { buf: self.buf.clone(), lines: (l0, l1), dx: x, dy: y, spans: self.spans.clone() }];
        if l0 == 0 {
            if let Some((b, s, bx, by)) = &self.bullet {
                v.push(TextBlock { buf: b.clone(), lines: (0, 1), dx: x + bx, dy: y + by, spans: s.clone() });
            }
        }
        v
    }
}

fn line_height(p: &Para, size: f32) -> f32 {
    let natural = size * p.natural;
    let h = match p.spacing {
        Spacing::Multiple(m) => natural * m,
        Spacing::Exact(v) => v,
        Spacing::AtLeast(v) => natural.max(v),
    };
    match (p.grid, p.spacing) {
        (Some(g), Spacing::Multiple(m)) if g > 1.0 => {
            // Snap to the document grid: whole multiples of the pitch.
            let lines = (natural / g).ceil().max(1.0) * m.max(0.5);
            (g * lines).max(h * 0.0)
        }
        _ => h,
    }
    .max(1.0)
}

fn attrs_for<'a>(fs: &FontSystem, map: &mut FontMap, st: &RunStyle, cjk: bool, meta: usize, lh: f32, family_buf: &'a mut Option<String>) -> Attrs<'a> {
    let want = if cjk { st.ea.as_deref().or(st.latin.as_deref()) } else { st.latin.as_deref().or(st.ea.as_deref()) };
    *family_buf = want.and_then(|w| map.resolve(fs, w));
    let mut a = Attrs::new().color(st.color.to_cosmic()).weight(if st.bold { Weight::BOLD } else { Weight::NORMAL }).metadata(meta);
    // CJK fonts have no italic faces; asking for one falls back to fonts without the glyphs.
    if st.italic && !cjk {
        a = a.style(Style::Italic);
    }
    a = a.family(match family_buf.as_deref() {
        Some(f) => Family::Name(f),
        None => Family::SansSerif,
    });
    let size = if st.baseline != 0.0 { st.size * 0.65 } else { st.size };
    a.metrics(Metrics::new(size.max(1.0), lh))
}

/// Lay out a paragraph to `width` points (None = no wrapping).
pub fn layout(fs: &mut FontSystem, map: &mut FontMap, p: &Para, width: Option<f32>) -> Laid {
    let max_size = p.runs.iter().map(|(_, s)| s.size).fold(0.0f32, f32::max).max(if p.runs.iter().all(|(t, _)| t.is_empty()) { p.end_style.size } else { 0.0 }).max(1.0);
    let lh = line_height(p, max_size);
    // Owned attributes per segment (families must outlive the span list).
    let mut segs: Vec<(String, AttrsOwned)> = Vec::new();
    let mut decos: Vec<SpanDeco> = Vec::new();
    // Positive first-line indent: a leading ideographic-space spacer.
    if p.first > 0.5 && p.bullet.is_none() {
        let n = (p.first / max_size).round().max(1.0) as usize;
        let size = p.first / n as f32;
        let st = RunStyle { size, color: Rgba { a: 0, ..Rgba::BLACK }, ..p.runs.first().map(|r| r.1.clone()).unwrap_or_default() };
        let mut fam = None;
        let a = attrs_for(fs, map, &st, true, decos.len(), lh, &mut fam);
        segs.push(("\u{3000}".repeat(n), AttrsOwned::new(a)));
        decos.push(SpanDeco::default());
    }
    for (text, st) in &p.runs {
        if text.is_empty() {
            continue;
        }
        let meta = decos.len();
        decos.push(SpanDeco { underline: st.underline, strike: st.strike, highlight: st.highlight });
        // Split into CJK / non-CJK segments so each gets the right typeface.
        let mut cur = String::new();
        let mut cur_cjk = None;
        for c in text.chars() {
            let k = is_cjk(c) || (cur_cjk == Some(true) && (c == ' ' || c.is_ascii_punctuation()));
            if cur_cjk.is_some() && cur_cjk != Some(k) && !cur.is_empty() {
                let mut fam = None;
                let a = attrs_for(fs, map, st, cur_cjk.unwrap(), meta, lh, &mut fam);
                segs.push((std::mem::take(&mut cur), AttrsOwned::new(a)));
            }
            cur_cjk = Some(k);
            cur.push(c);
        }
        if !cur.is_empty() {
            let mut fam = None;
            let a = attrs_for(fs, map, st, cur_cjk.unwrap_or(false), meta, lh, &mut fam);
            segs.push((cur, AttrsOwned::new(a)));
        }
    }
    let text_left = p.left;
    let wrap_w = width.map(|w| (w - text_left - p.right).max(4.0));
    let mut buf = Buffer::new(fs, Metrics::new(max_size, lh));
    buf.set_wrap(fs, if wrap_w.is_some() { Wrap::WordOrGlyph } else { Wrap::None });
    buf.set_size(fs, wrap_w, None);
    let default = Attrs::new().metrics(Metrics::new(p.end_style.size.max(1.0), lh));
    if segs.is_empty() {
        buf.set_rich_text(fs, [("", default)], default, Shaping::Advanced);
    } else {
        buf.set_rich_text(fs, segs.iter().map(|(t, a)| (t.as_str(), a.as_attrs())), default, Shaping::Advanced);
    }
    let align = match p.align {
        Align::Left => None,
        Align::Center => Some(cosmic_text::Align::Center),
        Align::Right => Some(cosmic_text::Align::Right),
        Align::Justify => Some(cosmic_text::Align::Justified),
    };
    for l in buf.lines.iter_mut() {
        l.set_align(align);
    }
    buf.shape_until_scroll(fs, false);
    let mut lines = Vec::new();
    for run in buf.layout_runs() {
        lines.push((run.line_top, run.line_height));
    }
    if lines.is_empty() {
        lines.push((0.0, lh));
    }
    let height = lines.last().map(|(t, h)| t + h).unwrap_or(lh);
    // Bullet: laid out on its own, baseline-aligned with the first line.
    let bullet = p.bullet.as_ref().map(|(sym, st)| {
        let mut b = Buffer::new(fs, Metrics::new(st.size.max(1.0), lh));
        b.set_wrap(fs, Wrap::None);
        b.set_size(fs, None, None);
        let mut fam = None;
        let a = attrs_for(fs, map, st, sym.chars().any(is_cjk), 0, lh, &mut fam);
        b.set_rich_text(fs, [(sym.as_str(), a)], a, Shaping::Advanced);
        b.shape_until_scroll(fs, false);
        let first_y = buf.layout_runs().next().map(|r| r.line_y).unwrap_or(lh * 0.8);
        let by = b.layout_runs().next().map(|r| r.line_y).unwrap_or(lh * 0.8);
        (Arc::new(b), Arc::new(vec![SpanDeco::default()]), p.first, first_y - by)
    });
    Laid { buf: Arc::new(buf), spans: Arc::new(decos), lines, height, bullet }
}
