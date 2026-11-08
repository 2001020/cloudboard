//! PowerPoint (.pptx) import: slides with their layouts and masters,
//! placeholders, theme colours/fonts, shapes, pictures, groups, tables,
//! SmartArt drawings and build animations.

use crate::dml::{ColorCtx, Theme};
use crate::geom;
use crate::model::*;
use crate::opc::{emu, NodeExt, Package, Rel};
use crate::text::{self, Align, FontMap, Para, RunStyle, Spacing};
use cosmic_text::FontSystem;
use roxmltree::Node;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tiny_skia::{Pixmap, Transform};

type N<'a> = Node<'a, 'a>;

pub fn load(bytes: &[u8], fs: &mut FontSystem, map: &mut FontMap) -> Result<Document, String> {
    let pkg = Package::open(bytes)?;
    let pres_part = pkg.rel_of_kind("", "officeDocument").unwrap_or_else(|| "ppt/presentation.xml".into());
    let pres_xml = pkg.text(&pres_part).ok_or("不是有效的 PowerPoint 文件（缺少 presentation.xml）")?;
    if !pres_xml.contains("presentation") {
        return Err("不是有效的 PowerPoint 文件".into());
    }
    let pres = roxmltree::Document::parse(pres_xml).map_err(|e| e.to_string())?;
    let root = pres.root_element();
    let (sw, sh) = root.child("sldSz").map(|s| (emu(s.attr_i64("cx").unwrap_or(12_192_000)), emu(s.attr_i64("cy").unwrap_or(6_858_000)))).unwrap_or((960.0, 540.0));
    let rels = pkg.rels(&pres_part);
    // sldId has a plain numeric `id` and the relationship `r:id`.
    let slide_parts: Vec<String> = root.child("sldIdLst").map(|l| l.children_named("sldId").filter_map(|s| s.attributes().find(|a| a.name() == "id" && a.namespace().is_some()).map(|a| a.value())).filter_map(|rid| rels.get(rid).map(|r| r.target.clone())).collect()).unwrap_or_default();
    // Every part we need, parsed once.
    let mut names: Vec<String> = Vec::new();
    let push = |n: String, names: &mut Vec<String>| {
        if !names.contains(&n) && pkg.has(&n) {
            names.push(n);
        }
    };
    for s in &slide_parts {
        push(s.clone(), &mut names);
        let r = pkg.rels(s);
        for rel in r.values() {
            if matches!(rel.kind.as_str(), "diagramDrawing" | "diagramData") {
                push(rel.target.clone(), &mut names);
            }
        }
        if let Some(l) = r.values().find(|x| x.kind == "slideLayout") {
            push(l.target.clone(), &mut names);
            if let Some(m) = pkg.rel_of_kind(&l.target, "slideMaster") {
                push(m.clone(), &mut names);
                if let Some(t) = pkg.rel_of_kind(&m, "theme") {
                    push(t, &mut names);
                }
            }
        }
    }
    let mut docs: HashMap<String, roxmltree::Document> = HashMap::new();
    for n in &names {
        if let Some(t) = pkg.text(n) {
            if let Ok(d) = roxmltree::Document::parse(t) {
                docs.insert(n.clone(), d);
            }
        }
    }
    let default_text = root.child("defaultTextStyle");
    let mut images: HashMap<String, Option<Arc<Pixmap>>> = HashMap::new();
    let mut pages = Vec::new();
    for (i, sp) in slide_parts.iter().enumerate() {
        let Some(sd) = docs.get(sp) else { continue };
        let sroot = sd.root_element();
        if sroot.attr("show") == Some("0") {
            continue; // hidden slide
        }
        let srels = pkg.rels(sp);
        let layout_part = srels.values().find(|r| r.kind == "slideLayout").map(|r| r.target.clone());
        let ld = layout_part.as_ref().and_then(|l| docs.get(l));
        let master_part = layout_part.as_ref().and_then(|l| pkg.rel_of_kind(l, "slideMaster"));
        let md = master_part.as_ref().and_then(|m| docs.get(m));
        let theme_doc = master_part.as_ref().and_then(|m| pkg.rel_of_kind(m, "theme")).and_then(|t| docs.get(&t));
        let theme = theme_doc.map(Theme::parse).unwrap_or_else(Theme::empty);
        // Colour map: master clrMap, overridden by layout/slide clrMapOvr.
        let mroot = md.map(|d| d.root_element());
        let lroot = ld.map(|d| d.root_element());
        let mut clr = mroot.and_then(|m| m.child("clrMap"));
        for r in [lroot, Some(sroot)].into_iter().flatten() {
            if let Some(o) = r.child("clrMapOvr").and_then(|o| o.child("overrideClrMapping")) {
                clr = Some(o);
            }
        }
        let cc = ColorCtx::with_map(&theme, clr);
        let mut page = Page::new(sw, sh);
        let mut b = Builder {
            pkg: &pkg,
            fs: &mut *fs,
            map: &mut *map,
            cc: &cc,
            theme: &theme,
            rels: srels.clone(),
            images: &mut images,
            default_text,
            master_tx: mroot.and_then(|m| m.child("txStyles")),
            layout_tree: lroot.and_then(|l| l.path(&["cSld", "spTree"])),
            master_tree: mroot.and_then(|m| m.path(&["cSld", "spTree"])),
            slide_no: i + 1,
            items: Vec::new(),
            docs: &docs,
            slide_part: sp.clone(),
        };
        // Background: slide → layout → master.
        page.background = [Some(sroot), lroot, mroot].into_iter().flatten().find_map(|r| {
            let bg = r.path(&["cSld", "bg"])?;
            Some(b.background(bg))
        });
        if page.background.is_none() {
            page.background = Some(Paint::Solid(cc.scheme("bg1").unwrap_or(Rgba::WHITE)));
        }
        // Master and layout decorations (non-placeholder shapes).
        let show_master = |r: Option<N>| r.and_then(|r| r.attr("showMasterSp")).map(|v| v != "0" && v != "false").unwrap_or(true);
        if show_master(Some(sroot)) {
            if show_master(lroot) {
                if let (Some(tree), Some(mp)) = (b.master_tree, master_part.as_ref()) {
                    b.rels = pkg.rels(mp);
                    b.tree(tree, &Group::root(), false);
                }
            }
            if let (Some(tree), Some(lp)) = (b.layout_tree, layout_part.as_ref()) {
                b.rels = pkg.rels(lp);
                b.tree(tree, &Group::root(), false);
            }
        }
        b.rels = srels;
        if let Some(tree) = sroot.path(&["cSld", "spTree"]) {
            b.tree(tree, &Group::root(), true);
        }
        page.items = std::mem::take(&mut b.items);
        if let Some(t) = sroot.child("timing") {
            let (auto, steps) = crate::anim::parse(t);
            page.auto = auto;
            page.steps = steps;
        }
        pages.push(page);
    }
    if pages.is_empty() {
        return Err("演示文稿中没有幻灯片".into());
    }
    Ok(Document { pages })
}

/// Coordinate mapping for (nested) groups: linear part plus rotations.
#[derive(Clone)]
struct Group {
    sx: f32,
    sy: f32,
    tx: f32,
    ty: f32,
    rot: Transform,
    ids: Vec<u32>,
}

impl Group {
    fn root() -> Group {
        Group { sx: 1.0, sy: 1.0, tx: 0.0, ty: 0.0, rot: Transform::identity(), ids: Vec::new() }
    }
    fn map(&self, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
        (self.tx + x * self.sx, self.ty + y * self.sy, w * self.sx, h * self.sy)
    }
}

#[derive(Clone, Copy, Default)]
struct Xfrm {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rot: f32,
    flip_h: bool,
    flip_v: bool,
}

fn read_xfrm(x: N) -> Option<Xfrm> {
    let off = x.child("off")?;
    let ext = x.child("ext")?;
    Some(Xfrm {
        x: emu(off.attr_i64("x").unwrap_or(0)),
        y: emu(off.attr_i64("y").unwrap_or(0)),
        w: emu(ext.attr_i64("cx").unwrap_or(0)),
        h: emu(ext.attr_i64("cy").unwrap_or(0)),
        rot: x.attr_f32("rot").unwrap_or(0.0) / 60_000.0,
        flip_h: x.attr_bool("flipH").unwrap_or(false),
        flip_v: x.attr_bool("flipV").unwrap_or(false),
    })
}

/// Local box → page transform (with or without the mirroring flips).
fn item_xf(g: &Group, xf: &Xfrm, with_flip: bool) -> (Transform, f32, f32) {
    let (x, y, w, h) = g.map(xf.x, xf.y, xf.w, xf.h);
    let mut t = g.rot.pre_translate(x, y);
    if xf.rot.abs() > 0.01 {
        t = t.pre_translate(w * 0.5, h * 0.5).pre_rotate(xf.rot).pre_translate(-w * 0.5, -h * 0.5);
    }
    if with_flip && (xf.flip_h || xf.flip_v) {
        t = t.pre_translate(if xf.flip_h { w } else { 0.0 }, if xf.flip_v { h } else { 0.0 }).pre_scale(if xf.flip_h { -1.0 } else { 1.0 }, if xf.flip_v { -1.0 } else { 1.0 });
    }
    (t, w, h)
}

struct Builder<'s, 'a> {
    pkg: &'a Package,
    fs: &'s mut FontSystem,
    map: &'s mut FontMap,
    cc: &'s ColorCtx<'s, 'a>,
    theme: &'s Theme<'a>,
    rels: HashMap<String, Rel>,
    images: &'s mut HashMap<String, Option<Arc<Pixmap>>>,
    default_text: Option<N<'a>>,
    master_tx: Option<N<'a>>,
    layout_tree: Option<N<'a>>,
    master_tree: Option<N<'a>>,
    slide_no: usize,
    items: Vec<Item>,
    docs: &'a HashMap<String, roxmltree::Document<'a>>,
    slide_part: String,
}

fn ph_info(sp: N) -> Option<(String, Option<i64>)> {
    let nv = sp.children().find(|c| c.is_element() && c.tag_name().name().starts_with("nv"))?;
    let ph = nv.child("nvPr")?.child("ph")?;
    Some((ph.attr("type").unwrap_or("body").to_string(), ph.attr_i64("idx")))
}

fn shape_id(sp: N) -> u32 {
    sp.children().find(|c| c.is_element() && c.tag_name().name().starts_with("nv")).and_then(|nv| nv.children().find(|c| c.is_element() && c.tag_name().name().starts_with("cNv"))).and_then(|c| c.attr_i64("id")).unwrap_or(0) as u32
}

fn is_hidden(sp: N) -> bool {
    sp.children().find(|c| c.is_element() && c.tag_name().name().starts_with("nv")).and_then(|nv| nv.children().find(|c| c.is_element() && c.tag_name().name().starts_with("cNv"))).and_then(|c| c.attr_bool("hidden")).unwrap_or(false)
}

fn norm_ph(t: &str) -> &str {
    match t {
        "title" | "ctrTitle" => "title",
        "dt" | "ftr" | "sldNum" | "hdr" => t,
        _ => "body",
    }
}

/// Find the placeholder matching (type, idx) in a layout/master shape tree.
fn find_ph<'a>(tree: Option<N<'a>>, ty: &str, idx: Option<i64>, master: bool) -> Option<N<'a>> {
    let tree = tree?;
    let phs: Vec<(N, String, Option<i64>)> = tree.descendants().filter(|n| n.has_tag_name_local("sp")).filter_map(|n| ph_info(n).map(|(t, i)| (n, t, i))).collect();
    if !master {
        if let Some(i) = idx {
            if let Some(p) = phs.iter().find(|p| p.2 == Some(i)) {
                return Some(p.0);
            }
        }
        if let Some(p) = phs.iter().find(|p| p.1 == ty) {
            return Some(p.0);
        }
    }
    phs.iter().find(|p| norm_ph(&p.1) == norm_ph(ty)).map(|p| p.0)
}

impl<'s, 'a> Builder<'s, 'a> {
    fn image(&mut self, rid: &str) -> Option<Arc<Pixmap>> {
        let target = self.rels.get(rid).filter(|r| !r.external)?.target.clone();
        if let Some(v) = self.images.get(&target) {
            return v.clone();
        }
        let img = self.pkg.bytes(&target).and_then(cb_render::images::decode).map(Arc::new);
        self.images.insert(target, img.clone());
        img
    }

    fn background(&mut self, bg: N<'a>) -> Paint {
        let cc = self.cc;
        let mut img = |r: &str| self.image(r);
        if let Some(pr) = bg.child("bgPr") {
            if let Some(Some(p)) = cc.fill(pr, None, &mut img) {
                return p;
            }
        }
        if let Some(r) = bg.child("bgRef") {
            if let Some(p) = cc.fill_ref(r, &mut img) {
                return p;
            }
        }
        Paint::Solid(cc.scheme("bg1").unwrap_or(Rgba::WHITE))
    }

    fn tree(&mut self, tree: N<'a>, g: &Group, slide: bool) {
        for c in tree.children().filter(|c| c.is_element()) {
            self.node(c, g, slide);
        }
    }

    fn node(&mut self, c: N<'a>, g: &Group, slide: bool) {
        if is_hidden(c) {
            return;
        }
        match c.tag_name().name() {
            "sp" | "cxnSp" => {
                // Placeholders on layouts/masters are prompts, not content.
                if !slide && ph_info(c).is_some() {
                    return;
                }
                self.shape(c, g);
            }
            "pic" => self.picture(c, g),
            "grpSp" => self.group(c, g, slide),
            "graphicFrame" => self.frame(c, g),
            "AlternateContent" => {
                if let Some(f) = c.child("Fallback").or_else(|| c.child("Choice")) {
                    for k in f.children().filter(|k| k.is_element()) {
                        self.node(k, g, slide);
                    }
                }
            }
            _ => {}
        }
    }

    fn group(&mut self, c: N<'a>, g: &Group, slide: bool) {
        let Some(gx) = c.child("grpSpPr").and_then(|p| p.child("xfrm")) else { return };
        let Some(xf) = read_xfrm(gx) else { return };
        let (cx, cy, cw, ch) = match (gx.child("chOff"), gx.child("chExt")) {
            (Some(o), Some(e)) => (emu(o.attr_i64("x").unwrap_or(0)), emu(o.attr_i64("y").unwrap_or(0)), emu(e.attr_i64("cx").unwrap_or(0)), emu(e.attr_i64("cy").unwrap_or(0))),
            _ => (xf.x, xf.y, xf.w, xf.h),
        };
        let (px, py, pw, ph) = g.map(xf.x, xf.y, xf.w, xf.h);
        let sx = if cw > 0.0 { pw / cw } else { 1.0 };
        let sy = if ch > 0.0 { ph / ch } else { 1.0 };
        let mut rot = g.rot;
        if xf.rot.abs() > 0.01 || xf.flip_h || xf.flip_v {
            let (ccx, ccy) = (px + pw * 0.5, py + ph * 0.5);
            rot = rot.pre_translate(ccx, ccy).pre_rotate(xf.rot).pre_scale(if xf.flip_h { -1.0 } else { 1.0 }, if xf.flip_v { -1.0 } else { 1.0 }).pre_translate(-ccx, -ccy);
        }
        let mut ids = g.ids.clone();
        ids.push(shape_id(c));
        let ng = Group { sx, sy, tx: px - cx * sx, ty: py - cy * sy, rot, ids };
        for k in c.children().filter(|k| k.is_element() && !matches!(k.tag_name().name(), "nvGrpSpPr" | "grpSpPr")) {
            self.node(k, &ng, slide);
        }
    }

    fn push(&mut self, g: &Group, id: u32, xf: Transform, w: f32, h: f32, kind: Kind) {
        let mut ids = g.ids.clone();
        ids.push(id);
        self.items.push(Item { ids, xf, w, h, kind });
    }

    /// Placeholder chain for a slide shape: [the shape, layout ph, master ph].
    fn chain(&self, sp: N<'a>) -> Vec<N<'a>> {
        let mut v = vec![sp];
        if let Some((ty, idx)) = ph_info(sp) {
            if let Some(l) = find_ph(self.layout_tree, &ty, idx, false) {
                v.push(l);
            }
            if let Some(m) = find_ph(self.master_tree, &ty, idx, true) {
                v.push(m);
            }
        }
        v
    }

    fn shape(&mut self, sp: N<'a>, g: &Group) {
        let chain = self.chain(sp);
        let sppr = |n: N<'a>| n.child("spPr");
        let Some(xf) = chain.iter().find_map(|n| sppr(*n).and_then(|p| p.child("xfrm")).and_then(read_xfrm)) else { return };
        let id = shape_id(sp);
        let (t, w, h) = item_xf(g, &xf, true);
        let (tt, _, _) = item_xf(g, &xf, false);
        let cc = self.cc;
        // Geometry: first in chain.
        let spr = chain.iter().find_map(|n| sppr(*n).filter(|p| p.child("prstGeom").is_some() || p.child("custGeom").is_some()));
        let geom = match spr {
            Some(p) => {
                if let Some(cg) = p.child("custGeom") {
                    geom::custom(cg, w, h)
                } else {
                    let pg = p.child("prstGeom").unwrap();
                    let adj: HashMap<String, f32> = pg.child("avLst").map(|l| l.children_named("gd").filter_map(|gd| Some((gd.attr("name")?.to_string(), gd.attr("fmla")?.trim_start_matches("val ").trim().parse().ok()?))).collect()).unwrap_or_default();
                    geom::preset(pg.attr("prst").unwrap_or("rect"), w, h, &adj)
                }
            }
            None => geom::preset("rect", w, h, &HashMap::new()),
        };
        let style = sp.child("style");
        let mut img = |r: &str| self.image(r);
        let fill = chain.iter().find_map(|n| sppr(*n).and_then(|p| cc.fill(p, None, &mut img))).unwrap_or_else(|| style.and_then(|s| s.child("fillRef")).and_then(|r| cc.fill_ref(r, &mut img)));
        let line = chain.iter().find_map(|n| sppr(*n).and_then(|p| p.child("ln")).and_then(|l| {
            let ph = style.and_then(|s| s.child("lnRef")).and_then(|r| cc.color(r, None));
            match cc.line(l, ph) {
                Some(v) => Some(v),
                // <a:ln w=".."/> without a fill: width override on the style line.
                None => style.and_then(|s| s.child("lnRef")).and_then(|r| cc.line_ref(r)).map(|mut base| {
                    if let Some(w) = l.attr_i64("w") {
                        base.width = emu(w);
                    }
                    Some(base)
                }),
            }
        })).unwrap_or_else(|| style.and_then(|s| s.child("lnRef")).and_then(|r| cc.line_ref(r)));
        if fill.is_some() || line.is_some() {
            self.push(g, id, t, w, h, Kind::Shape { geom, fill, line });
        }
        // Text.
        let font_ref_color = style.and_then(|s| s.child("fontRef")).and_then(|r| cc.color(r, None));
        if let Some(body) = sp.child("txBody") {
            let blocks = self.text_body(body, &chain, w, h, font_ref_color);
            if !blocks.is_empty() {
                self.push(g, id, tt, w, h, Kind::Text(blocks));
            }
        }
    }

    fn picture(&mut self, pic: N<'a>, g: &Group) {
        let Some(xf) = pic.child("spPr").and_then(|p| p.child("xfrm")).and_then(read_xfrm) else { return };
        let id = shape_id(pic);
        let (t, w, h) = item_xf(g, &xf, true);
        let Some(bf) = pic.child("blipFill") else { return };
        let blip = bf.child("blip");
        let rid = blip.and_then(|b| b.attr("embed"));
        let crop = bf.child("srcRect").map(|r| [r.attr_f32("l").unwrap_or(0.0) / 100_000.0, r.attr_f32("t").unwrap_or(0.0) / 100_000.0, r.attr_f32("r").unwrap_or(0.0) / 100_000.0, r.attr_f32("b").unwrap_or(0.0) / 100_000.0]).unwrap_or([0.0; 4]);
        let alpha = blip.and_then(|b| b.child("alphaModFix")).and_then(|a| a.attr_f32("amt")).map(|v| v / 100_000.0).unwrap_or(1.0);
        match rid.and_then(|r| self.image(r)) {
            Some(img) => self.push(g, id, t, w, h, Kind::Image { img, crop, alpha }),
            None => self.push(g, id, t, w, h, Kind::Placeholder("图片".into())),
        }
        let cc = self.cc;
        if let Some(Some(line)) = pic.child("spPr").and_then(|p| p.child("ln")).and_then(|l| cc.line(l, None)) {
            let geom = geom::preset("rect", w, h, &HashMap::new());
            self.push(g, id, t, w, h, Kind::Shape { geom, fill: None, line: Some(line) });
        }
    }

    fn frame(&mut self, fr: N<'a>, g: &Group) {
        let Some(xf) = fr.child("xfrm").and_then(read_xfrm) else { return };
        let id = shape_id(fr);
        let Some(gd) = fr.path(&["graphic", "graphicData"]) else { return };
        if let Some(tbl) = gd.child("tbl") {
            self.table(tbl, g, &xf, id);
            return;
        }
        let uri = gd.attr("uri").unwrap_or("");
        if uri.ends_with("/diagram") {
            if self.diagram(gd, g, &xf, id) {
                return;
            }
        }
        // OLE objects (equations etc.) carry a picture fallback.
        if let Some(p) = gd.descendants().find(|n| n.has_tag_name_local("pic")) {
            let mut ng = g.clone();
            ng.ids.push(id);
            // The fallback picture's xfrm usually matches the frame; force it.
            let (t, w, h) = item_xf(g, &xf, false);
            if let Some(rid) = p.child("blipFill").and_then(|b| b.child("blip")).and_then(|b| b.attr("embed")) {
                if let Some(img) = self.image(rid) {
                    self.push(g, id, t, w, h, Kind::Image { img, crop: [0.0; 4], alpha: 1.0 });
                    return;
                }
            }
        }
        let (t, w, h) = item_xf(g, &xf, false);
        let label = if uri.ends_with("/chart") { "图表" } else { "对象" };
        self.push(g, id, t, w, h, Kind::Placeholder(label.into()));
    }

    /// SmartArt: draw the pre-rendered drawing part that PowerPoint saves.
    fn diagram(&mut self, gd: N<'a>, g: &Group, xf: &Xfrm, id: u32) -> bool {
        let Some(rel_ids) = gd.child("relIds") else { return false };
        let Some(dm) = rel_ids.attr("dm") else { return false };
        let slide_rels = self.pkg.rels(&self.slide_part);
        let Some(data_part) = slide_rels.get(dm).map(|r| r.target.clone()) else { return false };
        // The data part names the drawing relationship (on the slide) in an extension.
        let drawing_rid = self.docs.get(&data_part).and_then(|d| d.descendants().find(|n| n.has_tag_name_local("dataModelExt")).and_then(|n| n.attr("relId")).map(|s| s.to_string()));
        let drawing_part = drawing_rid.and_then(|r| slide_rels.get(&r).map(|x| x.target.clone())).or_else(|| slide_rels.values().find(|r| r.kind == "diagramDrawing").map(|r| r.target.clone()));
        let Some(dp) = drawing_part else { return false };
        let Some(doc) = self.docs.get(&dp) else { return false };
        let Some(tree) = doc.root_element().child("spTree") else { return false };
        let (px, py, _, _) = g.map(xf.x, xf.y, xf.w, xf.h);
        let mut ids = g.ids.clone();
        ids.push(id);
        let ng = Group { sx: g.sx, sy: g.sy, tx: px, ty: py, rot: g.rot, ids };
        let saved = std::mem::replace(&mut self.rels, self.pkg.rels(&dp));
        for sp in tree.children().filter(|c| c.is_element()) {
            match sp.tag_name().name() {
                "sp" => self.diagram_shape(sp, &ng),
                "grpSp" => self.group(sp, &ng, true),
                _ => {}
            }
        }
        self.rels = saved;
        true
    }

    fn diagram_shape(&mut self, sp: N<'a>, g: &Group) {
        self.shape(sp, g);
        // Diagram text may live in its own text box (dsp:txXfrm); `shape` used
        // the shape box. Good enough for the common layouts.
    }

    fn table(&mut self, tbl: N<'a>, g: &Group, xf: &Xfrm, id: u32) {
        let cols: Vec<f32> = tbl.child("tblGrid").map(|gr| gr.children_named("gridCol").map(|c| emu(c.attr_i64("w").unwrap_or(0))).collect()).unwrap_or_default();
        if cols.is_empty() {
            return;
        }
        let pr = tbl.child("tblPr");
        let styled = pr.and_then(|p| p.child("tableStyleId")).is_some() || pr.is_some_and(|p| p.attr_bool("firstRow").unwrap_or(false));
        let first_row = pr.and_then(|p| p.attr_bool("firstRow")).unwrap_or(false);
        let band_row = pr.and_then(|p| p.attr_bool("bandRow")).unwrap_or(false);
        let accent = self.cc.scheme("accent1").unwrap_or(Rgba::rgb(68, 114, 196));
        let tint = |c: Rgba, k: f32| crate::dml::map_linear(c, |v| v * k + (1.0 - k));
        let mut y = 0.0;
        let rows: Vec<N> = tbl.children_named("tr").collect();
        let cc = self.cc;
        for (ri, tr) in rows.iter().enumerate() {
            let mut h = emu(tr.attr_i64("h").unwrap_or(0));
            let header = first_row && ri == 0;
            // First pass: measure cell text to grow the row if needed.
            let cells: Vec<N> = tr.children_named("tc").collect();
            let mut col = 0usize;
            let mut laid: Vec<(usize, usize, Option<(Vec<TextBlock>, f32)>)> = Vec::new();
            for tc in &cells {
                let span = tc.attr_i64("gridSpan").unwrap_or(1).max(1) as usize;
                let merged = tc.attr_bool("hMerge").unwrap_or(false) || tc.attr_bool("vMerge").unwrap_or(false);
                if !merged {
                    let w: f32 = cols.iter().skip(col).take(span).sum();
                    let color = if header { Some(Rgba::WHITE) } else { None };
                    let body = tc.child("txBody");
                    let r = body.map(|b| {
                        let (blocks, need) = self.table_text(b, w, header, color);
                        (blocks, need)
                    });
                    if let Some((_, need)) = &r {
                        h = h.max(*need);
                    }
                    laid.push((col, span, r));
                }
                col += span;
            }
            // Second pass: fills, borders, text.
            let mut li = 0;
            col = 0;
            for tc in &cells {
                let span = tc.attr_i64("gridSpan").unwrap_or(1).max(1) as usize;
                let merged = tc.attr_bool("hMerge").unwrap_or(false) || tc.attr_bool("vMerge").unwrap_or(false);
                let x: f32 = cols.iter().take(col).sum();
                let w: f32 = cols.iter().skip(col).take(span).sum();
                col += span;
                if merged {
                    continue;
                }
                let row_span = tc.attr_i64("rowSpan").unwrap_or(1).max(1) as usize;
                let mut ch = h;
                if row_span > 1 {
                    for r in rows.iter().skip(ri + 1).take(row_span - 1) {
                        ch += emu(r.attr_i64("h").unwrap_or(0));
                    }
                }
                let tcpr = tc.child("tcPr");
                let mut img = |r: &str| self.image(r);
                let own = tcpr.and_then(|p| cc.fill(p, None, &mut img));
                let fill = match own {
                    Some(f) => f,
                    None if styled => Some(Paint::Solid(if header { accent } else if band_row && ri % 2 == if first_row { 1 } else { 0 } { tint(accent, 0.4) } else { tint(accent, 0.2) })),
                    None => None,
                };
                let cell = Xfrm { x: xf.x + x, y: xf.y + y, w, h: ch, ..Default::default() };
                let (t, cw, chh) = item_xf(g, &cell, false);
                let rect = geom::preset("rect", cw, chh, &HashMap::new());
                if let Some(f) = fill {
                    self.push(g, id, t, cw, chh, Kind::Shape { geom: rect.clone(), fill: Some(f), line: None });
                }
                // Borders: explicit per side, else white grid lines for styled tables.
                for (side, (a, b)) in [("lnL", ((0.0, 0.0), (0.0, chh))), ("lnR", ((cw, 0.0), (cw, chh))), ("lnT", ((0.0, 0.0), (cw, 0.0))), ("lnB", ((0.0, chh), (cw, chh)))] {
                    let line = match tcpr.and_then(|p| p.child(side)) {
                        Some(l) => cc.line(l, None).flatten(),
                        None if styled => Some(Line { width: 1.0, color: Rgba::WHITE, dash: Dash::Solid, head: Arrow::None, tail: Arrow::None }),
                        None => None,
                    };
                    if let Some(l) = line {
                        let mut pb = tiny_skia::PathBuilder::new();
                        pb.move_to(a.0, a.1);
                        pb.line_to(b.0, b.1);
                        if let Some(path) = pb.finish() {
                            self.push(g, id, t, cw, chh, Kind::Shape { geom: vec![SubPath { path, fill: false, stroke: true, shade: 1.0 }], fill: None, line: Some(l) });
                        }
                    }
                }
                if let Some((_, _, Some((blocks, need)))) = laid.get(li) {
                    let anchor = tcpr.and_then(|p| p.attr("anchor")).unwrap_or("t");
                    let dy = match anchor {
                        "ctr" => ((chh - need) * 0.5).max(0.0),
                        "b" => (chh - need).max(0.0),
                        _ => 0.0,
                    };
                    let blocks: Vec<TextBlock> = blocks.iter().cloned().map(|mut b| {
                        b.dy += dy;
                        b
                    }).collect();
                    self.push(g, id, t, cw, chh, Kind::Text(blocks));
                }
                li += 1;
            }
            y += h;
        }
    }

    fn table_text(&mut self, body: N<'a>, w: f32, header: bool, color: Option<Rgba>) -> (Vec<TextBlock>, f32) {
        let (l, r, t, b) = (7.2, 7.2, 3.6, 3.6);
        let chain: Vec<N<'a>> = Vec::new();
        let mut paras = self.paragraphs(body, &chain, w - l - r, color, 1.0, 0.0);
        if header {
            for p in &mut paras {
                for (_, st) in &mut p.runs {
                    st.bold = true;
                }
            }
        }
        let mut blocks = Vec::new();
        let mut y = t;
        for (i, p) in paras.iter().enumerate() {
            if i > 0 {
                y += p.before;
            }
            let laid = text::layout(self.fs, self.map, p, Some(w - l - r));
            blocks.extend(laid.blocks(0, laid.lines.len(), l + p.left, y));
            y += laid.height + p.after;
        }
        (blocks, y + b)
    }

    /// Lay out a shape's text body inside its w×h box.
    fn text_body(&mut self, body: N<'a>, chain: &[N<'a>], w: f32, h: f32, font_ref: Option<Rgba>) -> Vec<TextBlock> {
        let bodies: Vec<N> = chain.iter().filter_map(|n| n.child("txBody")).collect();
        let bpr = |name: &str| bodies.iter().find_map(|b| b.child("bodyPr").and_then(|p| p.attr(name)));
        let ins = |name: &str, d: i64| bpr(name).and_then(|v| v.parse::<i64>().ok()).map(emu).unwrap_or(emu(d));
        let (l, t, r, b) = (ins("lIns", 91_440), ins("tIns", 45_720), ins("rIns", 91_440), ins("bIns", 45_720));
        let wrap = bpr("wrap") != Some("none");
        let anchor = bpr("anchor").unwrap_or("t");
        let auto = body.child("bodyPr").and_then(|p| p.child("normAutofit"));
        let font_scale = auto.and_then(|a| a.attr_f32("fontScale")).map(|v| v / 100_000.0).unwrap_or(1.0);
        let spc_red = auto.and_then(|a| a.attr_f32("lnSpcReduction")).map(|v| v / 100_000.0).unwrap_or(0.0);
        let inner_w = (w - l - r).max(1.0);
        let paras = self.paragraphs(body, chain, inner_w, font_ref, font_scale, spc_red);
        let mut laid = Vec::new();
        let mut total = 0.0;
        for (i, p) in paras.iter().enumerate() {
            let lp = text::layout(self.fs, self.map, p, if wrap { Some(inner_w) } else { None });
            let before = if i == 0 { 0.0 } else { p.before };
            total += before + lp.height + p.after;
            laid.push((before, lp, p.left, p.after));
        }
        let inner_h = h - t - b;
        let mut y = t + match anchor {
            "ctr" => (inner_h - total) * 0.5,
            "b" => inner_h - total,
            _ => 0.0,
        };
        let mut blocks = Vec::new();
        for (before, lp, left, after) in laid {
            y += before;
            let x = if wrap { l } else {
                // Unwrapped text: honour alignment against the box.
                l
            };
            blocks.extend(lp.blocks(0, lp.lines.len(), x + left, y));
            y += lp.height + after;
        }
        blocks
    }

    /// Level style nodes for the shape: [own lstStyle, placeholder chain lstStyles, master txStyles, defaultTextStyle].
    fn level_styles(&self, chain: &[N<'a>], lvl: usize) -> Vec<N<'a>> {
        let key = format!("lvl{}pPr", lvl + 1);
        let mut v = Vec::new();
        for n in chain {
            if let Some(ls) = n.child("txBody").and_then(|b| b.child("lstStyle")) {
                if let Some(p) = ls.child(&key) {
                    v.push(p);
                }
            }
        }
        let ph = chain.first().and_then(|s| ph_info(*s));
        let master_style = match ph.as_ref().map(|p| norm_ph(&p.0)) {
            Some("title") => "titleStyle",
            Some("body") => "bodyStyle",
            _ => "otherStyle",
        };
        if let Some(p) = self.master_tx.and_then(|t| t.child(master_style)).and_then(|s| s.child(&key)) {
            v.push(p);
        }
        if let Some(p) = self.default_text.and_then(|d| d.child(&key)) {
            v.push(p);
        }
        v
    }

    fn paragraphs(&mut self, body: N<'a>, chain: &[N<'a>], width: f32, font_ref: Option<Rgba>, font_scale: f32, spc_red: f32) -> Vec<Para> {
        let cc = self.cc;
        let theme = self.theme;
        let mut out = Vec::new();
        let mut auto_num: HashMap<(usize, String), i64> = HashMap::new();
        for p in body.children_named("p") {
            let ppr = p.child("pPr");
            let lvl = ppr.and_then(|x| x.attr_i64("lvl")).unwrap_or(0).clamp(0, 8) as usize;
            // The shape's own lstStyle first, then the font reference colour, then the rest.
            let styles = self.level_styles(chain, lvl);
            let pnodes: Vec<N> = ppr.into_iter().chain(styles.iter().copied()).collect();
            let pattr = |name: &str| pnodes.iter().find_map(|n| n.attr(name));
            let pchild = |name: &str| pnodes.iter().find_map(|n| n.child(name));
            let align = match pattr("algn") {
                Some("ctr") => Align::Center,
                Some("r") => Align::Right,
                Some("just") | Some("dist") => Align::Justify,
                _ => Align::Left,
            };
            let spc = |n: Option<N>, size: f32| -> Option<f32> {
                let n = n?;
                if let Some(p) = n.child("spcPts") {
                    return p.attr_f32("val").map(|v| v / 100.0);
                }
                n.child("spcPct").and_then(|p| p.attr_f32("val")).map(|v| v / 100_000.0 * size * 1.2)
            };
            let def_rpr: Vec<N> = styles.iter().filter_map(|s| s.child("defRPr")).collect();
            let run_style = |rpr: Option<N>, builder: &Builder| -> RunStyle {
                let nodes: Vec<N> = rpr.into_iter().chain(def_rpr.iter().copied()).collect();
                let a = |name: &str| nodes.iter().find_map(|n| n.attr(name));
                let size = a("sz").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100.0).unwrap_or(18.0) * font_scale;
                // Colour: run, then the shape's own lstStyle (first style node if it is local), then the font ref, then inherited.
                let own_color = rpr.and_then(|r| r.child("solidFill")).and_then(|f| cc.color(f, None));
                let inherited = def_rpr.iter().find_map(|n| n.child("solidFill").and_then(|f| cc.color(f, None)));
                let color = own_color.or(font_ref).or(inherited).unwrap_or_else(|| cc.scheme("tx1").unwrap_or(Rgba::BLACK));
                let face = |kind: &str| -> Option<String> {
                    let tf = nodes.iter().find_map(|n| n.child(kind).and_then(|f| f.attr("typeface")))?;
                    let resolved = match tf {
                        "+mj-lt" => theme.major.0.clone(),
                        "+mn-lt" => theme.minor.0.clone(),
                        "+mj-ea" => theme.major.1.clone(),
                        "+mn-ea" => theme.minor.1.clone(),
                        t => t.to_string(),
                    };
                    (!resolved.is_empty()).then_some(resolved)
                };
                let latin = face("latin").or_else(|| Some(theme.minor.0.clone()).filter(|s| !s.is_empty()));
                let ea = face("ea").or_else(|| Some(theme.minor.1.clone()).filter(|s| !s.is_empty()));
                let _ = builder;
                RunStyle {
                    size: size.max(1.0),
                    bold: a("b").map(|v| v == "1" || v == "true").unwrap_or(false),
                    italic: a("i").map(|v| v == "1" || v == "true").unwrap_or(false),
                    color,
                    latin,
                    ea,
                    underline: a("u").map(|v| v != "none").unwrap_or(false),
                    strike: a("strike").map(|v| v != "noStrike").unwrap_or(false),
                    highlight: rpr.and_then(|r| r.child("highlight")).and_then(|h| cc.color(h, None)),
                    baseline: a("baseline").and_then(|v| v.parse::<f32>().ok()).map(|v| v / 100_000.0).unwrap_or(0.0),
                }
            };
            let mut runs = Vec::new();
            for r in p.children().filter(|c| c.is_element()) {
                match r.tag_name().name() {
                    "r" => {
                        let t: String = r.child("t").map(|t| t.text().unwrap_or("").to_string()).unwrap_or_default();
                        runs.push((t, run_style(r.child("rPr"), self)));
                    }
                    "br" => runs.push(("\n".to_string(), run_style(r.child("rPr"), self))),
                    "fld" => {
                        let mut t: String = r.child("t").map(|t| t.text().unwrap_or("").to_string()).unwrap_or_default();
                        if r.attr("type") == Some("slidenum") {
                            t = self.slide_no.to_string();
                        }
                        runs.push((t, run_style(r.child("rPr"), self)));
                    }
                    _ => {}
                }
            }
            let end_style = run_style(p.child("endParaRPr"), self);
            let first_size = runs.iter().find(|(t, _)| !t.trim().is_empty()).map(|(_, s)| s.size).unwrap_or(end_style.size);
            let spacing = match pchild("lnSpc") {
                Some(n) => {
                    if let Some(p) = n.child("spcPts") {
                        Spacing::Exact(p.attr_f32("val").unwrap_or(1200.0) / 100.0)
                    } else {
                        Spacing::Multiple((n.child("spcPct").and_then(|p| p.attr_f32("val")).unwrap_or(100_000.0) / 100_000.0 - spc_red).max(0.5))
                    }
                }
                None => Spacing::Multiple((1.0 - spc_red).max(0.5)),
            };
            let mar_l = pattr("marL").and_then(|v| v.parse::<i64>().ok()).map(emu).unwrap_or(0.0);
            let indent = pattr("indent").and_then(|v| v.parse::<i64>().ok()).map(emu).unwrap_or(0.0);
            // Bullets: the first node that says anything about bullets decides.
            let has_text = runs.iter().any(|(t, _)| !t.trim().is_empty());
            let bullet_node = pnodes.iter().find(|n| n.child("buNone").is_some() || n.child("buChar").is_some() || n.child("buAutoNum").is_some() || n.child("buBlip").is_some());
            let bullet = bullet_node.filter(|_| has_text).and_then(|n| {
                let size_pct = pnodes.iter().find_map(|n| n.child("buSzPct")).and_then(|s| s.attr_f32("val")).map(|v| v / 100_000.0).unwrap_or(1.0);
                let color = pnodes.iter().find_map(|n| n.child("buClr")).and_then(|c| cc.color(c, None)).or_else(|| runs.first().map(|r| r.1.color)).unwrap_or(Rgba::BLACK);
                let font = pnodes.iter().find_map(|n| n.child("buFont")).and_then(|f| f.attr("typeface")).map(|s| s.to_string());
                let base = runs.iter().find(|(t, _)| !t.trim().is_empty()).map(|r| r.1.clone()).unwrap_or_default();
                let st = RunStyle { size: base.size * size_pct, color, latin: font.clone().or(base.latin.clone()), underline: false, strike: false, highlight: None, ..base };
                if let Some(c) = n.child("buChar") {
                    let ch = c.attr("char").unwrap_or("•");
                    // Symbol/Wingdings bullets: map the common private-use glyphs to Unicode.
                    let ch = match ch {
                        "§" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "■",
                        "Ø" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "➢",
                        "ü" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "✓",
                        "q" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "❑",
                        "v" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "❖",
                        "n" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "■",
                        "l" if font.as_deref().is_some_and(|f| f.contains("Wingdings")) => "●",
                        c => c,
                    };
                    Some((ch.to_string(), RunStyle { latin: None, ..st }))
                } else if let Some(a) = n.child("buAutoNum") {
                    let ty = a.attr("type").unwrap_or("arabicPeriod").to_string();
                    let start = a.attr_i64("startAt").unwrap_or(1);
                    let e = auto_num.entry((lvl, ty.clone())).or_insert(start - 1);
                    *e += 1;
                    Some((auto_number(&ty, *e), st))
                } else {
                    None
                }
            });
            // Reset numbering when a paragraph without that numbering appears at this level.
            if bullet.is_none() {
                auto_num.retain(|(l, _), _| *l < lvl);
            }
            out.push(Para {
                runs,
                end_style,
                align,
                spacing,
                before: spc(pchild("spcBef"), first_size).unwrap_or(0.0),
                after: spc(pchild("spcAft"), first_size).unwrap_or(0.0),
                left: mar_l,
                right: 0.0,
                first: if bullet.is_some() { indent } else { indent.max(0.0) },
                bullet,
                grid: None,
                natural: 1.2,
            });
            let _ = width;
        }
        out
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

fn alpha(n: i64) -> String {
    let mut n = n.max(1);
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
    }
    s.iter().rev().collect()
}

pub fn chinese_number(n: i64) -> String {
    let d = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match n {
        0..=9 => d[n as usize].to_string(),
        10 => "十".into(),
        11..=19 => format!("十{}", d[(n % 10) as usize]),
        20..=99 => format!("{}十{}", d[(n / 10) as usize], if n % 10 == 0 { "" } else { d[(n % 10) as usize] }),
        _ => n.to_string(),
    }
}

/// Text for an automatic bullet number.
fn auto_number(ty: &str, n: i64) -> String {
    let base = if ty.starts_with("romanUc") {
        roman(n).to_uppercase()
    } else if ty.starts_with("romanLc") {
        roman(n)
    } else if ty.starts_with("alphaUc") {
        alpha(n).to_uppercase()
    } else if ty.starts_with("alphaLc") {
        alpha(n)
    } else if ty.starts_with("circleNum") {
        char::from_u32(0x2460 + (n as u32).saturating_sub(1).min(19)).map(|c| c.to_string()).unwrap_or_default()
    } else if ty.starts_with("ea1Chs") || ty.contains("Chs") {
        chinese_number(n)
    } else {
        n.to_string()
    };
    if ty.ends_with("ParenBoth") {
        format!("({base})")
    } else if ty.ends_with("ParenR") {
        format!("{base})")
    } else if ty.ends_with("Plain") {
        base
    } else if ty.contains("Chs") {
        format!("{base}、")
    } else {
        format!("{base}.")
    }
}

#[allow(dead_code)]
fn _unused(_: &HashSet<u32>) {}
