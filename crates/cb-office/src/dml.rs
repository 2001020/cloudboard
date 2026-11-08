//! DrawingML: theme, colours (with modifiers), fills, lines, style references.

use crate::model::{Arrow, Dash, Line, Paint, Rgba};
use crate::opc::{emu, NodeExt};
use roxmltree::Node;
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::Pixmap;

pub struct Theme<'a> {
    pub colors: HashMap<String, Rgba>,
    pub major: (String, String),
    pub minor: (String, String),
    pub fills: Vec<Node<'a, 'a>>,
    pub lines: Vec<Node<'a, 'a>>,
    pub bg_fills: Vec<Node<'a, 'a>>,
}

impl<'a> Theme<'a> {
    pub fn empty() -> Theme<'a> {
        let mut colors = HashMap::new();
        for (k, v) in [("dk1", 0x000000), ("lt1", 0xFFFFFF), ("dk2", 0x44546A), ("lt2", 0xE7E6E6), ("accent1", 0x4472C4), ("accent2", 0xED7D31), ("accent3", 0xA5A5A5), ("accent4", 0xFFC000), ("accent5", 0x5B9BD5), ("accent6", 0x70AD47), ("hlink", 0x0563C1), ("folHlink", 0x954F72)] {
            colors.insert(k.to_string(), hex(v));
        }
        Theme { colors, major: ("Calibri Light".into(), String::new()), minor: ("Calibri".into(), String::new()), fills: Vec::new(), lines: Vec::new(), bg_fills: Vec::new() }
    }

    pub fn parse(doc: &'a roxmltree::Document<'a>) -> Theme<'a> {
        let mut t = Theme::empty();
        let root = doc.root_element();
        let Some(el) = root.child("themeElements") else { return t };
        if let Some(cs) = el.child("clrScheme") {
            for c in cs.children().filter(|c| c.is_element()) {
                let name = c.tag_name().name().to_string();
                if let Some(v) = c.child("srgbClr").and_then(|x| x.attr("val")).and_then(parse_hex) {
                    t.colors.insert(name, v);
                } else if let Some(v) = c.child("sysClr").and_then(|x| x.attr("lastClr")).and_then(parse_hex) {
                    t.colors.insert(name, v);
                }
            }
        }
        if let Some(fs) = el.child("fontScheme") {
            let read = |n: Option<Node>| -> (String, String) {
                let Some(n) = n else { return (String::new(), String::new()) };
                let latin = n.child("latin").and_then(|x| x.attr("typeface")).unwrap_or("").to_string();
                let mut ea = n.child("ea").and_then(|x| x.attr("typeface")).unwrap_or("").to_string();
                if ea.is_empty() {
                    ea = n.children_named("font").find(|f| f.attr("script") == Some("Hans")).and_then(|f| f.attr("typeface")).unwrap_or("").to_string();
                }
                (latin, ea)
            };
            t.major = read(fs.child("majorFont"));
            t.minor = read(fs.child("minorFont"));
        }
        if let Some(fm) = el.child("fmtScheme") {
            let list = |name: &str| fm.child(name).map(|l| l.children().filter(|c| c.is_element()).collect::<Vec<_>>()).unwrap_or_default();
            t.fills = list("fillStyleLst");
            t.lines = list("lnStyleLst");
            t.bg_fills = list("bgFillStyleLst");
        }
        t
    }
}

pub fn hex(v: u32) -> Rgba {
    Rgba::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok().map(hex)
}

/// Colour context: theme plus the master/slide colour map (bg1 → lt1 …).
pub struct ColorCtx<'t, 'a> {
    pub theme: &'t Theme<'a>,
    pub map: HashMap<String, String>,
}

impl<'t, 'a> ColorCtx<'t, 'a> {
    pub fn new(theme: &'t Theme<'a>) -> Self {
        let mut map = HashMap::new();
        for (k, v) in [("bg1", "lt1"), ("tx1", "dk1"), ("bg2", "lt2"), ("tx2", "dk2")] {
            map.insert(k.to_string(), v.to_string());
        }
        ColorCtx { theme, map }
    }

    pub fn with_map(theme: &'t Theme<'a>, clr_map: Option<Node>) -> Self {
        let mut c = Self::new(theme);
        if let Some(m) = clr_map {
            for a in m.attributes() {
                c.map.insert(a.name().to_string(), a.value().to_string());
            }
        }
        c
    }

    pub fn scheme(&self, name: &str) -> Option<Rgba> {
        let n = self.map.get(name).map(|s| s.as_str()).unwrap_or(name);
        self.theme.colors.get(n).copied().or_else(|| self.theme.colors.get(name).copied())
    }

    /// Resolve the colour element inside `parent` (e.g. a solidFill).
    pub fn color(&self, parent: Node, ph: Option<Rgba>) -> Option<Rgba> {
        let c = parent.children().find(|c| c.is_element() && matches!(c.tag_name().name(), "srgbClr" | "schemeClr" | "sysClr" | "prstClr" | "scrgbClr" | "hslClr"))?;
        let mut col = match c.tag_name().name() {
            "srgbClr" => c.attr("val").and_then(parse_hex)?,
            "schemeClr" => {
                let v = c.attr("val").unwrap_or("tx1");
                if v == "phClr" {
                    ph.unwrap_or(Rgba::BLACK)
                } else {
                    self.scheme(v).unwrap_or(Rgba::BLACK)
                }
            }
            "sysClr" => c.attr("lastClr").and_then(parse_hex).unwrap_or(if c.attr("val") == Some("window") { Rgba::WHITE } else { Rgba::BLACK }),
            "prstClr" => preset_color(c.attr("val").unwrap_or("black")),
            "scrgbClr" => {
                let f = |k: &str| ((c.attr_f32(k).unwrap_or(0.0) / 100_000.0).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
                Rgba::rgb(f("r"), f("g"), f("b"))
            }
            _ => {
                let h = c.attr_f32("hue").unwrap_or(0.0) / 60_000.0;
                let s = c.attr_f32("sat").unwrap_or(0.0) / 100_000.0;
                let l = c.attr_f32("lum").unwrap_or(0.0) / 100_000.0;
                from_hsl(h, s, l, 255)
            }
        };
        for m in c.children().filter(|m| m.is_element()) {
            let v = m.attr_f32("val").unwrap_or(100_000.0) / 100_000.0;
            col = match m.tag_name().name() {
                "lumMod" => map_hsl(col, |h, s, l| (h, s, l * v)),
                "lumOff" => map_hsl(col, |h, s, l| (h, s, l + v)),
                "satMod" => map_hsl(col, |h, s, l| (h, s * v, l)),
                "satOff" => map_hsl(col, |h, s, l| (h, s + v, l)),
                "hueOff" => map_hsl(col, |h, s, l| (h + m.attr_f32("val").unwrap_or(0.0) / 60_000.0, s, l)),
                // tint/shade work in linear light (as PowerPoint does).
                "tint" => map_linear(col, |c| c * v + (1.0 - v)),
                "shade" => map_linear(col, |c| c * v),
                "alpha" => Rgba { a: (v * 255.0).clamp(0.0, 255.0) as u8, ..col },
                "alphaMod" => Rgba { a: (col.a as f32 * v).clamp(0.0, 255.0) as u8, ..col },
                "inv" => Rgba { r: 255 - col.r, g: 255 - col.g, b: 255 - col.b, a: col.a },
                "gray" => {
                    let y = (0.3 * col.r as f32 + 0.59 * col.g as f32 + 0.11 * col.b as f32) as u8;
                    Rgba { r: y, g: y, b: y, a: col.a }
                }
                _ => col,
            };
        }
        Some(col)
    }

    /// Fill described by the children of `parent` (spPr, bgPr, tcPr, a fill style …).
    /// `Some(None)` = explicitly no fill; `None` = not specified.
    pub fn fill(&self, parent: Node, ph: Option<Rgba>, img: &mut dyn FnMut(&str) -> Option<Arc<Pixmap>>) -> Option<Option<Paint>> {
        for c in parent.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "noFill" | "solidFill" | "gradFill" | "blipFill" | "pattFill" => return Some(self.fill_elem(c, ph, img)),
                "grpFill" => return None,
                _ => {}
            }
        }
        None
    }

    /// Evaluate one fill element (solidFill, gradFill …).
    pub fn fill_elem(&self, c: Node, ph: Option<Rgba>, img: &mut dyn FnMut(&str) -> Option<Arc<Pixmap>>) -> Option<Paint> {
        match c.tag_name().name() {
            "solidFill" => self.color(c, ph).map(Paint::Solid),
            "gradFill" => {
                let mut stops: Vec<(f32, Rgba)> = c.child("gsLst").map(|l| l.children_named("gs").filter_map(|g| Some((g.attr_f32("pos").unwrap_or(0.0) / 100_000.0, self.color(g, ph)?))).collect()).unwrap_or_default();
                stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                if stops.is_empty() {
                    return None;
                }
                if c.child("path").is_some() {
                    // OOXML radial stops run from the outside in.
                    let s = stops.into_iter().rev().map(|(p, c)| (1.0 - p, c)).collect();
                    return Some(Paint::Radial { stops: s });
                }
                let angle = c.child("lin").and_then(|l| l.attr_f32("ang")).unwrap_or(5_400_000.0) / 60_000.0;
                Some(Paint::Linear { angle, stops })
            }
            "blipFill" => c.child("blip").and_then(|b| b.attr("embed")).and_then(|r| img(r)).map(|img| Paint::Image { img }),
            "pattFill" => c.child("fgClr").and_then(|f| self.color(f, ph)).map(Paint::Solid),
            _ => None,
        }
    }

    pub fn line(&self, ln: Node, ph: Option<Rgba>) -> Option<Option<Line>> {
        let width = ln.attr_i64("w").map(emu);
        let mut color = None;
        let mut explicit_none = false;
        for c in ln.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "noFill" => explicit_none = true,
                "solidFill" => color = self.color(c, ph),
                "gradFill" => color = c.child("gsLst").and_then(|l| l.children_named("gs").next()).and_then(|g| self.color(g, ph)),
                _ => {}
            }
        }
        if explicit_none {
            return Some(None);
        }
        let dash = match ln.child("prstDash").and_then(|d| d.attr("val")) {
            Some("dash") | Some("sysDash") => Dash::Dash,
            Some("dot") | Some("sysDot") => Dash::Dot,
            Some("dashDot") | Some("sysDashDot") | Some("lgDashDot") | Some("lgDashDotDot") | Some("sysDashDotDot") => Dash::DashDot,
            Some("lgDash") => Dash::LongDash,
            _ => Dash::Solid,
        };
        let arrow = |n: &str| match ln.child(n).and_then(|e| e.attr("type")) {
            Some("triangle") | Some("stealth") => Arrow::Triangle,
            Some("arrow") => Arrow::Open,
            Some("oval") | Some("diamond") => Arrow::Oval,
            _ => Arrow::None,
        };
        let color = color?;
        Some(Some(Line { width: width.unwrap_or(0.75), color, dash, head: arrow("headEnd"), tail: arrow("tailEnd") }))
    }

    /// Fill from a style reference (`a:fillRef idx` with a colour child).
    pub fn fill_ref(&self, r: Node, img: &mut dyn FnMut(&str) -> Option<Arc<Pixmap>>) -> Option<Paint> {
        let idx = r.attr_i64("idx").unwrap_or(0);
        let ph = self.color(r, None);
        let style = if idx >= 1001 { self.theme.bg_fills.get((idx - 1001) as usize) } else if idx >= 1 { self.theme.fills.get((idx - 1) as usize) } else { None };
        match style {
            Some(s) => self.fill_elem(*s, ph, img),
            None if idx > 0 => ph.map(Paint::Solid),
            None => None,
        }
    }

    pub fn line_ref(&self, r: Node) -> Option<Line> {
        let idx = r.attr_i64("idx").unwrap_or(0);
        if idx <= 0 {
            return None;
        }
        let ph = self.color(r, None);
        match self.theme.lines.get((idx - 1) as usize) {
            Some(ln) => self.line(*ln, ph).flatten(),
            None => ph.map(|color| Line { width: 0.75 * idx as f32, color, dash: Dash::Solid, head: Arrow::None, tail: Arrow::None }),
        }
    }
}

fn preset_color(name: &str) -> Rgba {
    match name {
        "white" => Rgba::WHITE,
        "red" => Rgba::rgb(255, 0, 0),
        "green" => Rgba::rgb(0, 128, 0),
        "blue" => Rgba::rgb(0, 0, 255),
        "yellow" => Rgba::rgb(255, 255, 0),
        "gray" | "grey" => Rgba::rgb(128, 128, 128),
        "orange" => Rgba::rgb(255, 165, 0),
        _ => Rgba::BLACK,
    }
}

fn to_hsl(c: Rgba) -> (f32, f32, f32) {
    let (r, g, b) = (c.r as f32 / 255.0, c.g as f32 / 255.0, c.b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r { (g - b) / d + if g < b { 6.0 } else { 0.0 } } else if max == g { (b - r) / d + 2.0 } else { (r - g) / d + 4.0 };
    (h * 60.0, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32, a: u8) -> Rgba {
    let (s, l) = (s.clamp(0.0, 1.0), l.clamp(0.0, 1.0));
    if s <= 0.0 {
        let v = (l * 255.0).round() as u8;
        return Rgba { r: v, g: v, b: v, a };
    }
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let h = h.rem_euclid(360.0) / 360.0;
    let f = |t: f32| {
        let t = t.rem_euclid(1.0);
        let v = if t < 1.0 / 6.0 { p + (q - p) * 6.0 * t } else if t < 0.5 { q } else if t < 2.0 / 3.0 { p + (q - p) * (2.0 / 3.0 - t) * 6.0 } else { p };
        (v * 255.0).round().clamp(0.0, 255.0) as u8
    };
    Rgba { r: f(h + 1.0 / 3.0), g: f(h), b: f(h - 1.0 / 3.0), a }
}

pub(crate) fn map_linear(c: Rgba, f: impl Fn(f32) -> f32) -> Rgba {
    let to_lin = |v: u8| {
        let x = v as f32 / 255.0;
        if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
    };
    let to_srgb = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let v = if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    };
    Rgba { r: to_srgb(f(to_lin(c.r))), g: to_srgb(f(to_lin(c.g))), b: to_srgb(f(to_lin(c.b))), a: c.a }
}

fn map_hsl(c: Rgba, f: impl Fn(f32, f32, f32) -> (f32, f32, f32)) -> Rgba {
    let (h, s, l) = to_hsl(c);
    let (h, s, l) = f(h, s, l);
    from_hsl(h, s, l, c.a)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn colour_modifiers() {
        let xml = r#"<a:solidFill xmlns:a="x"><a:schemeClr val="accent1"><a:lumMod val="75000"/></a:schemeClr></a:solidFill>"#;
        let d = roxmltree::Document::parse(xml).unwrap();
        let theme = Theme::empty();
        let ctx = ColorCtx::new(&theme);
        let c = ctx.color(d.root_element(), None).unwrap();
        // accent1 4472C4 darkened 25% ≈ 2F5597.
        assert!((c.r as i32 - 0x2F).abs() <= 3 && (c.g as i32 - 0x55).abs() <= 3 && (c.b as i32 - 0x97).abs() <= 3, "{c:?}");
        let xml = r#"<a:solidFill xmlns:a="x"><a:schemeClr val="tx1"/></a:solidFill>"#;
        let d = roxmltree::Document::parse(xml).unwrap();
        assert_eq!(ctx.color(d.root_element(), None), Some(Rgba::BLACK));
    }
}
