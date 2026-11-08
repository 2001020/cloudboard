//! Word (.docx) import: styles, numbering, paragraphs, tables, images,
//! text boxes, headers/footers, and pagination into fixed pages.

use crate::dml::{parse_hex, ColorCtx, Theme};
use crate::geom;
use crate::model::*;
use crate::opc::{emu, NodeExt, Package, Rel};
use crate::text::{self, Align, FontMap, Para, RunStyle, Spacing};
use cosmic_text::FontSystem;
use roxmltree::Node;
use std::collections::HashMap;
use std::sync::Arc;
use tiny_skia::{Pixmap, Transform};

type N<'a> = Node<'a, 'a>;

fn tw(v: f32) -> f32 {
    v / 20.0
}

fn on(n: Option<N>) -> Option<bool> {
    n.map(|e| !matches!(e.attr("val"), Some("0") | Some("false") | Some("off") | Some("none")))
}

struct Style<'a> {
    based_on: Option<String>,
    ppr: Option<N<'a>>,
    rpr: Option<N<'a>>,
    tblpr: Option<N<'a>>,
}

struct NumLevel {
    start: i64,
    fmt: String,
    text: String,
    left: f32,
    hanging: f32,
    rpr_font: Option<String>,
}

struct Ctx<'s, 'a> {
    pkg: &'a Package,
    fs: &'s mut FontSystem,
    map: &'s mut FontMap,
    styles: HashMap<String, Style<'a>>,
    default_para: Option<String>,
    doc_rpr: Option<N<'a>>,
    doc_ppr: Option<N<'a>>,
    theme: &'s Theme<'a>,
    cc: &'s ColorCtx<'s, 'a>,
    rels: HashMap<String, Rel>,
    images: HashMap<String, Option<Arc<Pixmap>>>,
    nums: HashMap<i64, Vec<NumLevel>>,
    counters: HashMap<(i64, usize), i64>,
    grid: Option<f32>,
    /// Paragraph properties of the enclosing table's style.
    table_ppr: Vec<N<'a>>,
    // Page geometry.
    pw: f32,
    ph: f32,
    margin: (f32, f32, f32, f32), // top, right, bottom, left
    pages: Vec<Page>,
    y: f32,
    next_id: u32,
}

pub fn load(bytes: &[u8], fs: &mut FontSystem, map: &mut FontMap) -> Result<Document, String> {
    let pkg = Package::open(bytes)?;
    let main = pkg.rel_of_kind("", "officeDocument").unwrap_or_else(|| "word/document.xml".into());
    let xml = pkg.text(&main).ok_or("不是有效的 Word 文件（缺少 document.xml）")?;
    let doc = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let body = doc.root_element().child("body").ok_or("文档没有正文")?;
    let rels = pkg.rels(&main);
    let part = |kind: &str| rels.values().find(|r| r.kind == kind && !r.external).map(|r| r.target.clone());
    let styles_xml = part("styles").and_then(|p| pkg.text(&p));
    let styles_doc = styles_xml.and_then(|x| roxmltree::Document::parse(x).ok());
    let num_xml = part("numbering").and_then(|p| pkg.text(&p));
    let num_doc = num_xml.and_then(|x| roxmltree::Document::parse(x).ok());
    let theme_xml = part("theme").and_then(|p| pkg.text(&p));
    let theme_doc = theme_xml.and_then(|x| roxmltree::Document::parse(x).ok());
    let theme = theme_doc.as_ref().map(Theme::parse).unwrap_or_else(Theme::empty);
    let cc = ColorCtx::new(&theme);
    // Headers/footers referenced by the final section.
    let hf_parts: Vec<(String, String)> = body.child("sectPr").map(|s| s.children().filter(|c| matches!(c.tag_name().name(), "headerReference" | "footerReference") && c.attr("type").unwrap_or("default") == "default").filter_map(|c| Some((c.tag_name().name().to_string(), rels.get(c.attr("id")?)?.target.clone()))).collect()).unwrap_or_default();
    let hf_docs: Vec<(String, String, roxmltree::Document)> = hf_parts.iter().filter_map(|(k, p)| Some((k.clone(), p.clone(), roxmltree::Document::parse(pkg.text(p)?).ok()?))).collect();

    let mut styles = HashMap::new();
    let mut default_para = None;
    let (mut doc_rpr, mut doc_ppr) = (None, None);
    if let Some(sd) = &styles_doc {
        let root = sd.root_element();
        if let Some(dd) = root.child("docDefaults") {
            doc_rpr = dd.path(&["rPrDefault", "rPr"]);
            doc_ppr = dd.path(&["pPrDefault", "pPr"]);
        }
        for s in root.children_named("style") {
            let Some(id) = s.attr("styleId") else { continue };
            if s.attr("type") == Some("paragraph") && s.attr_bool("default").unwrap_or(false) {
                default_para = Some(id.to_string());
            }
            styles.insert(id.to_string(), Style { based_on: s.child("basedOn").and_then(|b| b.attr("val")).map(|v| v.to_string()), ppr: s.child("pPr"), rpr: s.child("rPr"), tblpr: s.child("tblPr") });
        }
    }
    let mut nums = HashMap::new();
    if let Some(nd) = &num_doc {
        let root = nd.root_element();
        let abs: HashMap<i64, N> = root.children_named("abstractNum").filter_map(|a| Some((a.attr_i64("abstractNumId")?, a))).collect();
        for n in root.children_named("num") {
            let (Some(id), Some(aid)) = (n.attr_i64("numId"), n.child("abstractNumId").and_then(|a| a.attr_i64("val"))) else { continue };
            let Some(a) = abs.get(&aid) else { continue };
            let mut levels: Vec<NumLevel> = (0..9).map(|_| NumLevel { start: 1, fmt: "decimal".into(), text: "%1.".into(), left: 0.0, hanging: 0.0, rpr_font: None }).collect();
            for l in a.children_named("lvl") {
                let i = l.attr_i64("ilvl").unwrap_or(0).clamp(0, 8) as usize;
                let ind = l.path(&["pPr", "ind"]);
                levels[i] = NumLevel {
                    start: l.child("start").and_then(|s| s.attr_i64("val")).unwrap_or(1),
                    fmt: l.child("numFmt").and_then(|s| s.attr("val")).unwrap_or("decimal").to_string(),
                    text: l.child("lvlText").and_then(|s| s.attr("val")).unwrap_or("").to_string(),
                    left: ind.and_then(|i| i.attr_f32("left").or(i.attr_f32("start"))).map(tw).unwrap_or(0.0),
                    hanging: ind.and_then(|i| i.attr_f32("hanging")).map(tw).unwrap_or(0.0),
                    rpr_font: l.path(&["rPr", "rFonts"]).and_then(|f| f.attr("ascii")).map(|s| s.to_string()),
                };
            }
            for o in n.children_named("lvlOverride") {
                let i = o.attr_i64("ilvl").unwrap_or(0).clamp(0, 8) as usize;
                if let Some(s) = o.child("startOverride").and_then(|s| s.attr_i64("val")) {
                    levels[i].start = s;
                }
            }
            nums.insert(id, levels);
        }
    }
    let sect = body.child("sectPr");
    let pg = sect.and_then(|s| s.child("pgSz"));
    let pw = pg.and_then(|p| p.attr_f32("w")).map(tw).unwrap_or(595.3);
    let ph = pg.and_then(|p| p.attr_f32("h")).map(tw).unwrap_or(841.9);
    let mar = sect.and_then(|s| s.child("pgMar"));
    let m = |k: &str, d: f32| mar.and_then(|p| p.attr_f32(k)).map(tw).unwrap_or(d);
    let margin = (m("top", 72.0), m("right", 90.0), m("bottom", 72.0), m("left", 90.0));
    let header_y = m("header", 42.5);
    let footer_y = m("footer", 49.6);
    let grid = sect.and_then(|s| s.child("docGrid")).filter(|g| matches!(g.attr("type"), Some("lines") | Some("linesAndChars") | Some("snapToChars"))).and_then(|g| g.attr_f32("linePitch")).map(tw);
    let mut c = Ctx {
        pkg: &pkg,
        fs,
        map,
        styles,
        default_para,
        doc_rpr,
        doc_ppr,
        theme: &theme,
        cc: &cc,
        rels,
        images: HashMap::new(),
        nums,
        counters: HashMap::new(),
        grid,
        table_ppr: Vec::new(),
        pw,
        ph,
        margin,
        pages: Vec::new(),
        y: margin.0,
        next_id: 1,
    };
    c.new_page();
    let content_w = pw - margin.1 - margin.3;
    c.blocks(body, margin.3, content_w, None);
    // Headers and footers on every page (with page numbers).
    let total = c.pages.len();
    for (kind, part, d) in &hf_docs {
        let saved = std::mem::replace(&mut c.rels, pkg.rels(part));
        for pi in 0..total {
            let root = d.root_element();
            let items = c.flow_static(root, margin.3, content_w, pi + 1);
            let h: f32 = items.1;
            let top = if kind == "headerReference" { header_y } else { ph - footer_y - h };
            for mut it in items.0 {
                it.xf = Transform::from_translate(0.0, top).pre_concat(it.xf);
                c.pages[pi].items.push(it);
            }
        }
        c.rels = saved;
    }
    Ok(Document { pages: c.pages })
}

impl<'s, 'a> Ctx<'s, 'a> {
    fn new_page(&mut self) {
        let mut p = Page::new(self.pw, self.ph);
        p.background = Some(Paint::Solid(Rgba::WHITE));
        self.pages.push(p);
        self.y = self.margin.0;
    }

    fn bottom(&self) -> f32 {
        self.ph - self.margin.2
    }

    fn id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    fn put(&mut self, x: f32, y: f32, w: f32, h: f32, kind: Kind) {
        let id = self.id();
        self.pages.last_mut().unwrap().items.push(Item { ids: vec![id], xf: Transform::from_translate(x, y), w, h, kind });
    }

    fn image(&mut self, rid: &str) -> Option<Arc<Pixmap>> {
        let target = self.rels.get(rid).filter(|r| !r.external)?.target.clone();
        if let Some(v) = self.images.get(&target) {
            return v.clone();
        }
        let img = self.pkg.bytes(&target).and_then(cb_render::images::decode).map(Arc::new);
        self.images.insert(target, img.clone());
        img
    }

    fn style_chain(&self, id: Option<&str>) -> Vec<&Style<'a>> {
        let mut v = Vec::new();
        let mut cur = id.map(|s| s.to_string());
        let mut guard = 0;
        while let Some(i) = cur {
            guard += 1;
            if guard > 20 {
                break;
            }
            match self.styles.get(&i) {
                Some(s) => {
                    v.push(s);
                    cur = s.based_on.clone();
                }
                None => break,
            }
        }
        v
    }

    fn para_style_id<'n>(&self, p: N<'n>) -> Option<String> {
        p.child("pPr").and_then(|x| x.child("pStyle")).and_then(|s| s.attr("val")).map(|s| s.to_string()).or_else(|| self.default_para.clone())
    }

    /// pPr nodes in priority order.
    fn ppr_nodes(&self, p: N<'a>) -> Vec<N<'a>> {
        let mut v: Vec<N> = p.child("pPr").into_iter().collect();
        let sid = self.para_style_id(p);
        v.extend(self.style_chain(sid.as_deref()).iter().filter_map(|s| s.ppr));
        v.extend(self.table_ppr.iter().copied());
        v.extend(self.doc_ppr);
        v
    }

    /// rPr nodes in priority order for a run inside paragraph `p`.
    fn rpr_nodes(&self, rpr: Option<N<'a>>, p: N<'a>, table_rpr: &[N<'a>]) -> Vec<N<'a>> {
        let mut v: Vec<N> = rpr.into_iter().collect();
        let rstyle = rpr.and_then(|r| r.child("rStyle")).and_then(|s| s.attr("val")).map(|s| s.to_string());
        v.extend(self.style_chain(rstyle.as_deref()).iter().filter_map(|s| s.rpr));
        let sid = self.para_style_id(p);
        v.extend(self.style_chain(sid.as_deref()).iter().filter_map(|s| s.rpr));
        v.extend(table_rpr.iter().copied());
        v.extend(self.doc_rpr);
        v
    }

    fn run_style(&mut self, nodes: &[N<'a>]) -> RunStyle {
        let child = |name: &str| nodes.iter().find_map(|n| n.child(name));
        let size = child("sz").and_then(|s| s.attr_f32("val")).map(|v| v / 2.0).unwrap_or(10.5);
        let color = child("color").and_then(|c| {
            if let Some(t) = c.attr("themeColor") {
                let key = match t {
                    "text1" => "dk1",
                    "text2" => "dk2",
                    "background1" => "lt1",
                    "background2" => "lt2",
                    other => other,
                };
                let mut col = self.cc.scheme(key)?;
                if let Some(sh) = c.attr("themeShade").and_then(|v| u8::from_str_radix(v, 16).ok()) {
                    let k = sh as f32 / 255.0;
                    col = Rgba { r: (col.r as f32 * k) as u8, g: (col.g as f32 * k) as u8, b: (col.b as f32 * k) as u8, a: 255 };
                }
                if let Some(ti) = c.attr("themeTint").and_then(|v| u8::from_str_radix(v, 16).ok()) {
                    let k = ti as f32 / 255.0;
                    let f = |v: u8| (v as f32 * k + 255.0 * (1.0 - k)) as u8;
                    col = Rgba { r: f(col.r), g: f(col.g), b: f(col.b), a: 255 };
                }
                return Some(col);
            }
            c.attr("val").filter(|v| *v != "auto").and_then(parse_hex)
        });
        let fonts = child("rFonts");
        let theme_font = |v: Option<&str>| -> Option<String> {
            match v? {
                "majorHAnsi" | "majorAscii" | "majorBidi" => Some(self.theme.major.0.clone()),
                "minorHAnsi" | "minorAscii" | "minorBidi" => Some(self.theme.minor.0.clone()),
                "majorEastAsia" => Some(self.theme.major.1.clone()),
                "minorEastAsia" => Some(self.theme.minor.1.clone()),
                _ => None,
            }
        };
        // Fonts may be split across nodes (rFonts in style vs direct); look up each attribute.
        let fattr = |name: &str| nodes.iter().find_map(|n| n.child("rFonts").and_then(|f| f.attr(name)));
        let latin = fattr("ascii").map(|s| s.to_string()).or_else(|| theme_font(fattr("asciiTheme"))).or_else(|| fattr("hAnsi").map(|s| s.to_string())).filter(|s| !s.is_empty());
        let ea = fattr("eastAsia").map(|s| s.to_string()).or_else(|| theme_font(fattr("eastAsiaTheme"))).filter(|s| !s.is_empty());
        let _ = fonts;
        let highlight = child("highlight").and_then(|h| highlight_color(h.attr("val").unwrap_or(""))).or_else(|| child("shd").and_then(|s| s.attr("fill")).filter(|f| *f != "auto").and_then(parse_hex));
        let va = child("vertAlign").and_then(|v| v.attr("val"));
        RunStyle {
            size,
            bold: on(child("b")).unwrap_or(false),
            italic: on(child("i")).unwrap_or(false),
            color: color.unwrap_or(Rgba::BLACK),
            latin,
            ea,
            underline: child("u").is_some_and(|u| u.attr("val") != Some("none")),
            strike: on(child("strike")).unwrap_or(false) || on(child("dstrike")).unwrap_or(false),
            highlight,
            baseline: match va {
                Some("superscript") => 0.33,
                Some("subscript") => -0.15,
                _ => 0.0,
            },
        }
    }

    /// Build a paragraph spec (text runs only; drawings are returned separately).
    fn para(&mut self, p: N<'a>, table_rpr: &[N<'a>], page_no: Option<usize>) -> (Para, Vec<N<'a>>, bool) {
        let pnodes = self.ppr_nodes(p);
        let pchild = |name: &str| pnodes.iter().find_map(|n| n.child(name));
        let align = match pchild("jc").and_then(|j| j.attr("val")) {
            Some("center") => Align::Center,
            Some("right") | Some("end") => Align::Right,
            Some("both") | Some("distribute") => Align::Justify,
            _ => Align::Left,
        };
        let end_nodes = self.rpr_nodes(p.child("pPr").and_then(|x| x.child("rPr")), p, table_rpr);
        let end_style = self.run_style(&end_nodes);
        let mut runs = Vec::new();
        let mut drawings = Vec::new();
        let mut page_break = false;
        // Complex fields: replace PAGE results with the page number.
        let mut in_field = 0;
        let mut field_is_page = false;
        let mut field_result = false;
        let mut collect = |r: N<'a>, this: &mut Self, runs: &mut Vec<(String, RunStyle)>, drawings: &mut Vec<N<'a>>| {
            let nodes = this.rpr_nodes(r.child("rPr"), p, table_rpr);
            let st = this.run_style(&nodes);
            let mut t = String::new();
            for c in r.children().filter(|c| c.is_element()) {
                match c.tag_name().name() {
                    "t" => {
                        if !(in_field > 0 && field_is_page && field_result) {
                            t.push_str(c.text().unwrap_or(""));
                        }
                    }
                    "tab" => t.push_str("    "),
                    "br" => {
                        if c.attr("type") == Some("page") {
                            page_break = true;
                        } else {
                            t.push('\n');
                        }
                    }
                    "cr" => t.push('\n'),
                    "sym" => {
                        if let Some(ch) = c.attr("char").and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
                            t.push(ch);
                        }
                    }
                    "fldChar" => match c.attr("fldCharType") {
                        Some("begin") => {
                            in_field += 1;
                            field_is_page = false;
                            field_result = false;
                        }
                        Some("separate") => {
                            field_result = true;
                            if field_is_page {
                                if let Some(n) = page_no {
                                    t.push_str(&n.to_string());
                                }
                            }
                        }
                        Some("end") => {
                            in_field = (in_field - 1).max(0);
                            field_is_page = false;
                            field_result = false;
                        }
                        _ => {}
                    },
                    "instrText" => {
                        let ins = c.text().unwrap_or("").trim().to_uppercase();
                        if ins.starts_with("PAGE") && !ins.starts_with("PAGEREF") {
                            field_is_page = true;
                        }
                    }
                    "drawing" | "pict" => drawings.push(c),
                    "AlternateContent" => {
                        if let Some(ch) = c.child("Choice").or_else(|| c.child("Fallback")) {
                            drawings.extend(ch.descendants().filter(|d| d.has_tag_name_local("drawing")));
                        }
                    }
                    _ => {}
                }
            }
            if !t.is_empty() {
                runs.push((t, st));
            }
        };
        for c in p.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "r" => collect(c, self, &mut runs, &mut drawings),
                "hyperlink" | "ins" | "smartTag" | "customXml" | "sdt" => {
                    for r in c.descendants().filter(|d| d.has_tag_name_local("r")) {
                        collect(r, self, &mut runs, &mut drawings);
                    }
                }
                "fldSimple" => {
                    let is_page = c.attr("instr").map(|i| i.trim().to_uppercase().starts_with("PAGE")).unwrap_or(false);
                    if is_page {
                        if let Some(n) = page_no {
                            let nodes = self.rpr_nodes(c.descendants().find(|d| d.has_tag_name_local("rPr")), p, table_rpr);
                            let st = self.run_style(&nodes);
                            runs.push((n.to_string(), st));
                        }
                    } else {
                        for r in c.children_named("r") {
                            collect(r, self, &mut runs, &mut drawings);
                        }
                    }
                }
                "AlternateContent" => {
                    if let Some(ch) = c.child("Choice").or_else(|| c.child("Fallback")) {
                        for r in ch.descendants().filter(|d| d.has_tag_name_local("r")) {
                            collect(r, self, &mut runs, &mut drawings);
                        }
                    }
                }
                _ => {}
            }
        }
        let first_size = runs.iter().find(|(t, _)| !t.trim().is_empty()).map(|(_, s)| s.size).unwrap_or(end_style.size);
        // Spacing.
        let sp = pchild("spacing");
        let spa = |name: &str| pnodes.iter().find_map(|n| n.child("spacing").and_then(|s| s.attr(name)));
        let line = spa("line").and_then(|v| v.parse::<f32>().ok());
        let rule = spa("lineRule").unwrap_or("auto");
        let spacing = match (line, rule) {
            (Some(l), "exact") => Spacing::Exact(tw(l)),
            (Some(l), "atLeast") => Spacing::AtLeast(tw(l)),
            (Some(l), _) => Spacing::Multiple(l / 240.0),
            (None, _) => Spacing::Multiple(1.0),
        };
        let _ = sp;
        let before = spa("beforeLines").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0 * first_size * 1.3).or_else(|| spa("before").and_then(|v| v.parse::<f32>().ok()).map(tw)).unwrap_or(0.0);
        let after = spa("afterLines").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0 * first_size * 1.3).or_else(|| spa("after").and_then(|v| v.parse::<f32>().ok()).map(tw)).unwrap_or(0.0);
        // Indents.
        let ind = |name: &str| pnodes.iter().find_map(|n| n.child("ind").and_then(|s| s.attr(name))).and_then(|v| v.parse::<f32>().ok());
        let mut left = ind("left").or(ind("start")).map(tw).unwrap_or(0.0);
        let right = ind("right").or(ind("end")).map(tw).unwrap_or(0.0);
        let mut first = ind("firstLineChars").map(|v| v / 100.0 * first_size).or_else(|| ind("firstLine").map(tw)).unwrap_or(0.0);
        if let Some(h) = ind("hangingChars").map(|v| v / 100.0 * first_size).or_else(|| ind("hanging").map(tw)) {
            first = -h;
        }
        // Numbering.
        let mut bullet = None;
        let num = pchild("numPr");
        let num_id = num.and_then(|n| n.child("numId")).and_then(|n| n.attr_i64("val"));
        let ilvl = num.and_then(|n| n.child("ilvl")).and_then(|n| n.attr_i64("val")).unwrap_or(0).clamp(0, 8) as usize;
        if let Some(nid) = num_id.filter(|n| *n > 0) {
            if let Some(levels) = self.nums.get(&nid) {
                let lv = &levels[ilvl];
                let starts: Vec<i64> = levels.iter().map(|l| l.start).collect();
                let cur = {
                    let e = self.counters.entry((nid, ilvl)).or_insert(lv.start - 1);
                    *e += 1;
                    *e
                };
                // Deeper levels restart after a higher level advances.
                for d in ilvl + 1..9 {
                    self.counters.remove(&(nid, d));
                }
                let mut label = lv.text.clone();
                for (k, l) in levels.iter().enumerate().take(ilvl + 1) {
                    let v = if k == ilvl { cur } else { self.counters.get(&(nid, k)).copied().unwrap_or(starts[k]) };
                    label = label.replace(&format!("%{}", k + 1), &format_num(&l.fmt, v));
                }
                if lv.fmt == "bullet" {
                    label = match label.as_str() {
                        "\u{f0b7}" | "·" => "•".into(),
                        "\u{f0a7}" | "§" => "▪".into(),
                        "\u{f06c}" => "●".into(),
                        "\u{f0d8}" | "Ø" => "➢".into(),
                        "\u{f0fc}" => "✓".into(),
                        "o" if lv.rpr_font.as_deref() == Some("Courier New") => "○".into(),
                        s if s.chars().all(|c| (c as u32) >= 0xF000) => "•".into(),
                        _ => label,
                    };
                }
                if ind("left").is_none() && ind("start").is_none() {
                    left = lv.left;
                }
                if ind("hanging").is_none() && ind("firstLine").is_none() {
                    first = -lv.hanging;
                }
                let st = RunStyle { underline: false, highlight: None, ..runs.first().map(|r| r.1.clone()).unwrap_or_else(|| end_style.clone()) };
                if first > -1.0 {
                    // No hanging space: prefix the label to the text.
                    runs.insert(0, (format!("{label} "), st));
                } else {
                    bullet = Some((label, st));
                }
            }
        }
        let page_break_before = on(pchild("pageBreakBefore")).unwrap_or(false);
        let contextual = on(pchild("contextualSpacing")).unwrap_or(false);
        let para = Para {
            runs,
            end_style,
            align,
            spacing,
            before: if contextual { 0.0 } else { before },
            after: if contextual { 0.0 } else { after },
            left,
            right,
            first,
            bullet,
            grid: self.grid,
            natural: 1.17,
        };
        let _ = page_break_before;
        (para, drawings, page_break || page_break_before)
    }

    /// Flow block-level content (paragraphs, tables) into pages.
    fn blocks(&mut self, parent: N<'a>, x0: f32, width: f32, table_rpr: Option<&[N<'a>]>) {
        for c in parent.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "p" => self.paragraph(c, x0, width, table_rpr.unwrap_or(&[])),
                "tbl" => self.table(c, x0, width),
                "sdt" => {
                    if let Some(sc) = c.child("sdtContent") {
                        self.blocks(sc, x0, width, table_rpr);
                    }
                }
                _ => {}
            }
        }
    }

    fn paragraph(&mut self, p: N<'a>, x0: f32, width: f32, table_rpr: &[N<'a>]) {
        let pb_before = self.ppr_nodes(p).iter().find_map(|n| n.child("pageBreakBefore")).is_some_and(|n| on(Some(n)).unwrap_or(false));
        if pb_before && self.y > self.margin.0 + 1.0 {
            self.new_page();
        }
        let (para, drawings, page_break) = self.para(p, table_rpr, None);
        let has_text = para.runs.iter().any(|(t, _)| !t.trim().is_empty());
        if self.y > self.margin.0 + 0.5 {
            self.y += para.before;
        }
        if has_text || drawings.is_empty() {
            let laid = text::layout(self.fs, self.map, &para, Some(width));
            let mut l0 = 0;
            let mut top_y = self.y;
            for (i, (lt, lh)) in laid.lines.iter().enumerate() {
                let line_bottom = self.y + lh;
                if line_bottom > self.bottom() + 0.5 && self.y > self.margin.0 + 0.5 {
                    // Flush lines so far onto this page, continue on the next.
                    if i > l0 {
                        let blocks = laid.blocks(l0, i, para.left, 0.0);
                        self.put(x0, top_y, width, 0.0, Kind::Text(blocks));
                    }
                    self.new_page();
                    l0 = i;
                    top_y = self.y;
                }
                let _ = lt;
                self.y += lh;
            }
            let blocks = laid.blocks(l0, laid.lines.len(), para.left, 0.0);
            self.put(x0, top_y, width, 0.0, Kind::Text(blocks));
        }
        // Drawings: inline ones flow in a row below the text; anchored ones are absolute.
        let mut row: Vec<(Arc<Pixmap>, f32, f32)> = Vec::new();
        for d in drawings {
            self.drawing(d, x0, width, &mut row);
        }
        if !row.is_empty() {
            let gap = 6.0;
            let total: f32 = row.iter().map(|r| r.1).sum::<f32>() + gap * (row.len() - 1) as f32;
            let h = row.iter().map(|r| r.2).fold(0.0, f32::max);
            if self.y + h > self.bottom() && self.y > self.margin.0 + 0.5 {
                self.new_page();
            }
            let mut x = x0 + match para.align {
                Align::Center => ((width - total) * 0.5).max(0.0),
                Align::Right => (width - total).max(0.0),
                _ => para.left,
            };
            for (img, w, ih) in row {
                self.put(x, self.y + (h - ih), w, ih, Kind::Image { img, crop: [0.0; 4], alpha: 1.0 });
                x += w + gap;
            }
            self.y += h;
        }
        self.y += para.after;
        if page_break {
            self.new_page();
        }
    }

    fn drawing(&mut self, d: N<'a>, x0: f32, width: f32, row: &mut Vec<(Arc<Pixmap>, f32, f32)>) {
        for host in d.children().filter(|c| c.is_element()) {
            let inline = host.tag_name().name() == "inline";
            if !inline && host.tag_name().name() != "anchor" {
                continue;
            }
            let ext = host.child("extent");
            let w = ext.and_then(|e| e.attr_i64("cx")).map(emu).unwrap_or(0.0);
            let h = ext.and_then(|e| e.attr_i64("cy")).map(emu).unwrap_or(0.0);
            let Some(gd) = host.path(&["graphic", "graphicData"]) else { continue };
            let pic = gd.descendants().find(|n| n.has_tag_name_local("pic"));
            let wsp = gd.descendants().find(|n| n.has_tag_name_local("wsp"));
            if inline {
                if let Some(rid) = pic.and_then(|p| p.descendants().find(|n| n.has_tag_name_local("blip"))).and_then(|b| b.attr("embed")) {
                    if let Some(img) = self.image(rid) {
                        let k = (width / w.max(1.0)).min(1.0);
                        row.push((img, w * k, h * k));
                    }
                }
                continue;
            }
            // Anchored: position relative to page/margin/paragraph.
            let pos = |axis: &str| -> f32 {
                let p = host.child(axis);
                let rel = p.and_then(|p| p.attr("relativeFrom")).unwrap_or("column");
                let off = p.and_then(|p| p.child("posOffset")).and_then(|o| o.text()).and_then(|t| t.trim().parse::<i64>().ok()).map(emu);
                let align = p.and_then(|p| p.child("align")).and_then(|a| a.text()).unwrap_or("");
                let (base, span, size) = if axis == "positionH" {
                    match rel {
                        "page" => (0.0, self.pw, w),
                        _ => (x0, width, w),
                    }
                } else {
                    match rel {
                        "page" => (0.0, self.ph, h),
                        "margin" | "topMargin" => (self.margin.0, self.ph - self.margin.0 - self.margin.2, h),
                        _ => (self.y, 0.0, h),
                    }
                };
                match off {
                    Some(o) => base + o,
                    None => match align {
                        "center" => base + (span - size) * 0.5,
                        "right" | "bottom" => base + span - size,
                        _ => base,
                    },
                }
            };
            let (x, y) = (pos("positionH"), pos("positionV"));
            let wraps_block = host.child("wrapTopAndBottom").is_some();
            if let Some(rid) = pic.and_then(|p| p.descendants().find(|n| n.has_tag_name_local("blip"))).and_then(|b| b.attr("embed")) {
                if let Some(img) = self.image(rid) {
                    self.put(x, y, w, h, Kind::Image { img, crop: [0.0; 4], alpha: 1.0 });
                }
            } else if let Some(s) = wsp {
                self.text_box(s, x, y, w, h);
            }
            if wraps_block && host.child("positionV").and_then(|p| p.attr("relativeFrom")) == Some("paragraph") {
                self.y = self.y.max(y + h);
            }
        }
    }

    /// A Word text box / shape (wps:wsp): DrawingML geometry plus WordprocessingML text.
    fn text_box(&mut self, s: N<'a>, x: f32, y: f32, w: f32, h: f32) {
        let cc = self.cc;
        let sppr = s.child("spPr");
        let style = s.child("style");
        let geom = sppr.and_then(|p| p.child("prstGeom")).map(|g| geom::preset(g.attr("prst").unwrap_or("rect"), w, h, &HashMap::new())).unwrap_or_else(|| geom::preset("rect", w, h, &HashMap::new()));
        let mut img = |r: &str| self.image(r);
        let fill = sppr.and_then(|p| cc.fill(p, None, &mut img)).unwrap_or_else(|| style.and_then(|s| s.child("fillRef")).and_then(|r| cc.fill_ref(r, &mut img)));
        let line = sppr.and_then(|p| p.child("ln")).and_then(|l| cc.line(l, None)).unwrap_or_else(|| style.and_then(|s| s.child("lnRef")).and_then(|r| cc.line_ref(r)));
        if fill.is_some() || line.is_some() {
            self.put(x, y, w, h, Kind::Shape { geom, fill, line });
        }
        if let Some(content) = s.descendants().find(|n| n.has_tag_name_local("txbxContent")) {
            let bpr = s.child("bodyPr");
            let ins = |k: &str, d: i64| bpr.and_then(|b| b.attr_i64(k)).map(emu).unwrap_or(emu(d));
            let (l, t, r) = (ins("lIns", 91_440), ins("tIns", 45_720), ins("rIns", 91_440));
            let (items, _) = self.flow_static(content, l, (w - l - r).max(4.0), 0);
            for mut it in items {
                it.xf = Transform::from_translate(x, y + t).pre_concat(it.xf);
                self.pages.last_mut().unwrap().items.push(it);
            }
        }
    }

    /// Lay out paragraphs into a free-standing list of items (headers,
    /// footers, text boxes), returning the items and their total height.
    fn flow_static(&mut self, parent: N<'a>, x0: f32, width: f32, page_no: usize) -> (Vec<Item>, f32) {
        let mut y = 0.0;
        let mut items = Vec::new();
        for p in parent.children().filter(|c| c.has_tag_name_local("p")) {
            let (para, drawings, _) = self.para(p, &[], Some(page_no));
            y += para.before;
            let laid = text::layout(self.fs, self.map, &para, Some(width));
            let id = self.id();
            items.push(Item { ids: vec![id], xf: Transform::from_translate(x0, y), w: width, h: laid.height, kind: Kind::Text(laid.blocks(0, laid.lines.len(), para.left, 0.0)) });
            y += laid.height;
            for d in drawings {
                for host in d.children().filter(|c| c.has_tag_name_local("inline") || c.has_tag_name_local("anchor")) {
                    let ext = host.child("extent");
                    let (w, h) = (ext.and_then(|e| e.attr_i64("cx")).map(emu).unwrap_or(0.0), ext.and_then(|e| e.attr_i64("cy")).map(emu).unwrap_or(0.0));
                    if let Some(rid) = host.descendants().find(|n| n.has_tag_name_local("blip")).and_then(|b| b.attr("embed")) {
                        if let Some(img) = self.image(rid) {
                            let id = self.id();
                            items.push(Item { ids: vec![id], xf: Transform::from_translate(x0, y), w, h, kind: Kind::Image { img, crop: [0.0; 4], alpha: 1.0 } });
                            if host.tag_name().name() == "inline" {
                                y += h;
                            }
                        }
                    }
                }
            }
            y += para.after;
        }
        (items, y)
    }

    fn table(&mut self, tbl: N<'a>, x0: f32, width: f32) {
        let tblpr = tbl.child("tblPr");
        let style_id = tblpr.and_then(|p| p.child("tblStyle")).and_then(|s| s.attr("val")).map(|s| s.to_string());
        let style_pr: Vec<N> = self.style_chain(style_id.as_deref()).iter().filter_map(|s| s.tblpr).collect();
        let style_rpr: Vec<N> = self.style_chain(style_id.as_deref()).iter().filter_map(|s| s.rpr).collect();
        let style_ppr: Vec<N> = self.style_chain(style_id.as_deref()).iter().filter_map(|s| s.ppr).collect();
        let saved_ppr = std::mem::replace(&mut self.table_ppr, style_ppr);
        let tnodes: Vec<N> = tblpr.into_iter().chain(style_pr.iter().copied()).collect();
        let border = |name: &str| -> Option<Line> {
            let b = tnodes.iter().find_map(|n| n.child("tblBorders").and_then(|bs| bs.child(name)))?;
            border_line(b)
        };
        let borders = [border("top"), border("left").or(border("start")), border("bottom"), border("right").or(border("end")), border("insideH"), border("insideV")];
        let mar = |name: &str, d: f32| tnodes.iter().find_map(|n| n.child("tblCellMar").and_then(|m| m.child(name)).and_then(|e| e.attr_f32("w"))).map(tw).unwrap_or(d);
        let (cml, cmr, cmt, cmb) = (mar("left", 5.4).max(mar("start", 0.0)), mar("right", 5.4).max(mar("end", 0.0)), mar("top", 0.0), mar("bottom", 0.0));
        let indent = tnodes.iter().find_map(|n| n.child("tblInd")).and_then(|i| i.attr_f32("w")).map(tw).unwrap_or(0.0);
        let mut cols: Vec<f32> = tbl.child("tblGrid").map(|g| g.children_named("gridCol").map(|c| tw(c.attr_f32("w").unwrap_or(0.0))).collect()).unwrap_or_default();
        let total: f32 = cols.iter().sum();
        if cols.is_empty() || total <= 0.0 {
            return;
        }
        let avail = width - indent;
        if total > avail {
            let k = avail / total;
            cols.iter_mut().for_each(|c| *c *= k);
        }
        let tx0 = x0 + indent;
        let rows: Vec<N> = tbl.children_named("tr").collect();
        let nrows = rows.len();
        // vMerge bookkeeping: column index → item index of the cell that started the merge.
        for (ri, tr) in rows.iter().enumerate() {
            let min_h = tr.path(&["trPr", "trHeight"]).and_then(|h| h.attr_f32("val")).map(tw).unwrap_or(0.0);
            let cells: Vec<N> = tr.children_named("tc").collect();
            // Lay out cell contents.
            let mut col = 0usize;
            struct Cell<'b> {
                x: f32,
                w: f32,
                content: Vec<Item>,
                need: f32,
                tcpr: Option<N<'b>>,
                cont: bool,
            }
            let mut laid: Vec<Cell> = Vec::new();
            for tc in &cells {
                let tcpr = tc.child("tcPr");
                let span = tcpr.and_then(|p| p.child("gridSpan")).and_then(|g| g.attr_i64("val")).unwrap_or(1).max(1) as usize;
                let x: f32 = cols.iter().take(col).sum();
                let w: f32 = cols.iter().skip(col).take(span).sum();
                col += span;
                let vmerge = tcpr.and_then(|p| p.child("vMerge"));
                let cont = vmerge.is_some_and(|v| v.attr("val").unwrap_or("continue") == "continue");
                let (content, need) = if cont { (Vec::new(), 0.0) } else { self.cell_content(*tc, cml, (w - cml - cmr).max(4.0), &style_rpr) };
                laid.push(Cell { x, w, content, need: need + cmt + cmb, tcpr, cont });
            }
            let h = laid.iter().map(|c| c.need).fold(min_h, f32::max).max(8.0);
            if self.y + h > self.bottom() && self.y > self.margin.0 + 0.5 {
                self.new_page();
            }
            let y = self.y;
            for c in laid {
                let cx = tx0 + c.x;
                if let Some(fill) = c.tcpr.and_then(|p| p.child("shd")).and_then(|s| s.attr("fill")).filter(|f| *f != "auto").and_then(parse_hex) {
                    let g = geom::preset("rect", c.w, h, &HashMap::new());
                    self.put(cx, y, c.w, h, Kind::Shape { geom: g, fill: Some(Paint::Solid(fill)), line: None });
                }
                let valign = c.tcpr.and_then(|p| p.child("vAlign")).and_then(|v| v.attr("val")).unwrap_or("top");
                let dy = cmt + match valign {
                    "center" => ((h - c.need) * 0.5).max(0.0),
                    "bottom" => (h - c.need).max(0.0),
                    _ => 0.0,
                };
                for mut it in c.content {
                    it.xf = Transform::from_translate(cx, y + dy).pre_concat(it.xf);
                    self.pages.last_mut().unwrap().items.push(it);
                }
                // Borders: cell overrides, else table borders (outer vs inside).
                let tcb = |name: &str| c.tcpr.and_then(|p| p.child("tcBorders")).and_then(|b| b.child(name));
                let side = |name: &str, alt: &str, outer: &Option<Line>, inner: &Option<Line>, is_outer: bool| -> Option<Line> {
                    match tcb(name).or_else(|| tcb(alt)) {
                        Some(b) => border_line(b),
                        None => if is_outer { outer.clone() } else { inner.clone() },
                    }
                };
                let last_col = (c.x + c.w - cols.iter().sum::<f32>()).abs() < 0.5;
                let top = if c.cont { None } else { side("top", "top", &borders[0], &borders[4], ri == 0) };
                let left = side("left", "start", &borders[1], &borders[5], c.x < 0.5);
                let bottom = side("bottom", "bottom", &borders[2], &borders[4], ri + 1 == nrows);
                let right = side("right", "end", &borders[3], &borders[5], last_col);
                for (l, (a, b)) in [(top, ((0.0, 0.0), (c.w, 0.0))), (left, ((0.0, 0.0), (0.0, h))), (bottom, ((0.0, h), (c.w, h))), (right, ((c.w, 0.0), (c.w, h)))] {
                    if let Some(l) = l {
                        let mut pb = tiny_skia::PathBuilder::new();
                        pb.move_to(a.0, a.1);
                        pb.line_to(b.0, b.1);
                        if let Some(path) = pb.finish() {
                            self.put(cx, y, c.w, h, Kind::Shape { geom: vec![SubPath { path, fill: false, stroke: true, shade: 1.0 }], fill: None, line: Some(l) });
                        }
                    }
                }
            }
            self.y += h;
        }
        self.table_ppr = saved_ppr;
        self.y += 4.0;
    }

    /// Paragraphs (and nested tables, flattened) of a table cell as items relative to the cell.
    fn cell_content(&mut self, tc: N<'a>, x0: f32, width: f32, style_rpr: &[N<'a>]) -> (Vec<Item>, f32) {
        let mut items = Vec::new();
        let mut y = 0.0;
        for c in tc.children().filter(|c| c.is_element()) {
            let ps: Vec<N> = match c.tag_name().name() {
                "p" => vec![c],
                "tbl" => c.descendants().filter(|d| d.has_tag_name_local("p")).collect(),
                _ => continue,
            };
            for p in ps {
                let (para, drawings, _) = self.para(p, style_rpr, None);
                if y > 0.0 {
                    y += para.before;
                }
                let has_text = para.runs.iter().any(|(t, _)| !t.trim().is_empty());
                if has_text || drawings.is_empty() {
                    let laid = text::layout(self.fs, self.map, &para, Some(width));
                    let id = self.id();
                    items.push(Item { ids: vec![id], xf: Transform::from_translate(x0, y), w: width, h: laid.height, kind: Kind::Text(laid.blocks(0, laid.lines.len(), para.left, 0.0)) });
                    y += laid.height;
                }
                for d in drawings {
                    for host in d.children().filter(|h| h.has_tag_name_local("inline")) {
                        let ext = host.child("extent");
                        let (w, h) = (ext.and_then(|e| e.attr_i64("cx")).map(emu).unwrap_or(0.0), ext.and_then(|e| e.attr_i64("cy")).map(emu).unwrap_or(0.0));
                        let k = (width / w.max(1.0)).min(1.0);
                        if let Some(rid) = host.descendants().find(|n| n.has_tag_name_local("blip")).and_then(|b| b.attr("embed")) {
                            if let Some(img) = self.image(rid) {
                                let id = self.id();
                                items.push(Item { ids: vec![id], xf: Transform::from_translate(x0, y), w: w * k, h: h * k, kind: Kind::Image { img, crop: [0.0; 4], alpha: 1.0 } });
                                y += h * k;
                            }
                        }
                    }
                }
                y += para.after;
            }
        }
        (items, y)
    }
}

fn border_line(b: N) -> Option<Line> {
    let val = b.attr("val").unwrap_or("single");
    if matches!(val, "nil" | "none") {
        return None;
    }
    let width = b.attr_f32("sz").map(|v| v / 8.0).unwrap_or(0.5).max(0.25);
    let color = b.attr("color").filter(|c| *c != "auto").and_then(parse_hex).unwrap_or(Rgba::BLACK);
    let dash = match val {
        "dashed" | "dashSmallGap" => Dash::Dash,
        "dotted" => Dash::Dot,
        "dotDash" | "dotDotDash" => Dash::DashDot,
        _ => Dash::Solid,
    };
    Some(Line { width: if val == "double" { width * 2.5 } else { width }, color, dash, head: Arrow::None, tail: Arrow::None })
}

fn highlight_color(name: &str) -> Option<Rgba> {
    Some(match name {
        "yellow" => Rgba::rgb(255, 255, 0),
        "green" => Rgba::rgb(0, 255, 0),
        "cyan" => Rgba::rgb(0, 255, 255),
        "magenta" => Rgba::rgb(255, 0, 255),
        "blue" => Rgba::rgb(0, 0, 255),
        "red" => Rgba::rgb(255, 0, 0),
        "darkBlue" => Rgba::rgb(0, 0, 128),
        "darkCyan" => Rgba::rgb(0, 128, 128),
        "darkGreen" => Rgba::rgb(0, 128, 0),
        "darkMagenta" => Rgba::rgb(128, 0, 128),
        "darkRed" => Rgba::rgb(128, 0, 0),
        "darkYellow" => Rgba::rgb(128, 128, 0),
        "darkGray" => Rgba::rgb(128, 128, 128),
        "lightGray" => Rgba::rgb(192, 192, 192),
        "black" => Rgba::BLACK,
        _ => return None,
    })
}

/// Number formatting for list labels.
fn format_num(fmt: &str, n: i64) -> String {
    match fmt {
        "upperRoman" => roman(n).to_uppercase(),
        "lowerRoman" => roman(n),
        "upperLetter" => letter(n).to_uppercase(),
        "lowerLetter" => letter(n),
        "chineseCounting" | "chineseCountingThousand" | "ideographTraditional" | "chineseLegalSimplified" | "japaneseCounting" => crate::pptx::chinese_number(n),
        "decimalEnclosedCircle" | "decimalEnclosedCircleChinese" => char::from_u32(0x2460 + (n as u32).saturating_sub(1).min(19)).map(|c| c.to_string()).unwrap_or_default(),
        "decimalZero" => format!("{n:02}"),
        "bullet" | "none" => String::new(),
        _ => n.to_string(),
    }
}

fn roman(mut n: i64) -> String {
    let t = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

fn letter(n: i64) -> String {
    let mut n = n.max(1);
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}
