//! DrawingML shape geometry: the common preset shapes and custom paths.

use crate::model::SubPath;
use crate::opc::NodeExt;
use std::collections::HashMap;
use std::f32::consts::PI;
use tiny_skia::{Path, PathBuilder};

fn sub(p: Option<Path>) -> Vec<SubPath> {
    p.map(|path| vec![SubPath { path, fill: true, stroke: true, shade: 1.0 }]).unwrap_or_default()
}

fn poly(pts: &[(f32, f32)]) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let (x, y) = *pts.first()?;
    pb.move_to(x, y);
    for &(x, y) in &pts[1..] {
        pb.line_to(x, y);
    }
    pb.close();
    pb.finish()
}

fn ellipse_path(pb: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32) {
    if let Some(r) = tiny_skia::Rect::from_xywh(x, y, w.max(0.01), h.max(0.01)) {
        pb.push_oval(r);
    }
}

/// Elliptical arc as a polyline from angle a0 sweeping `sw` (radians).
fn arc_pts(pb: &mut PathBuilder, cx: f32, cy: f32, rx: f32, ry: f32, a0: f32, sw: f32, first: bool) {
    let n = ((sw.abs() / (2.0f32).to_radians()).ceil() as usize).clamp(2, 400);
    for i in 0..=n {
        let a = a0 + sw * i as f32 / n as f32;
        let (x, y) = (cx + rx * a.cos(), cy + ry * a.sin());
        if i == 0 && first {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
}

fn round_rect(w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let mut pb = PathBuilder::new();
    pb.move_to(r, 0.0);
    pb.line_to(w - r, 0.0);
    arc_pts(&mut pb, w - r, r, r, r, -PI / 2.0, PI / 2.0, false);
    pb.line_to(w, h - r);
    arc_pts(&mut pb, w - r, h - r, r, r, 0.0, PI / 2.0, false);
    pb.line_to(r, h);
    arc_pts(&mut pb, r, h - r, r, r, PI / 2.0, PI / 2.0, false);
    pb.line_to(0.0, r);
    arc_pts(&mut pb, r, r, r, r, PI, PI / 2.0, false);
    pb.close();
    pb.finish()
}

fn star(w: f32, h: f32, n: usize, inner: f32) -> Option<Path> {
    let (cx, cy) = (w * 0.5, h * 0.5);
    let pts: Vec<(f32, f32)> = (0..n * 2)
        .map(|i| {
            let a = -PI / 2.0 + i as f32 * PI / n as f32;
            let k = if i % 2 == 0 { 1.0 } else { inner };
            (cx + cx * k * a.cos(), cy + cy * k * a.sin())
        })
        .collect();
    poly(&pts)
}

/// Preset geometry `name` for a w×h box. `adj` holds the shape's adjust
/// values (raw OOXML units, usually 1/100000 of the shorter side).
pub fn preset(name: &str, w: f32, h: f32, adj: &HashMap<String, f32>) -> Vec<SubPath> {
    let ss = w.min(h);
    let a = |k: &str, d: f32| adj.get(k).copied().unwrap_or(d) / 100_000.0;
    let r = |pts: &[(f32, f32)]| sub(poly(pts));
    match name {
        "rect" | "flowChartProcess" | "textBox" => r(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
        "roundRect" | "flowChartAlternateProcess" => sub(round_rect(w, h, ss * a("adj", 16_667.0))),
        "flowChartTerminator" => sub(round_rect(w, h, ss * 0.5)),
        "ellipse" | "flowChartConnector" => {
            let mut pb = PathBuilder::new();
            ellipse_path(&mut pb, 0.0, 0.0, w, h);
            sub(pb.finish())
        }
        "triangle" | "flowChartExtract" => r(&[(w * a("adj", 50_000.0), 0.0), (w, h), (0.0, h)]),
        "rtTriangle" => r(&[(0.0, 0.0), (w, h), (0.0, h)]),
        "diamond" | "flowChartDecision" => r(&[(w * 0.5, 0.0), (w, h * 0.5), (w * 0.5, h), (0.0, h * 0.5)]),
        "parallelogram" | "flowChartInputOutput" => {
            let o = ss * a("adj", 25_000.0);
            r(&[(o, 0.0), (w, 0.0), (w - o, h), (0.0, h)])
        }
        "trapezoid" => {
            let o = ss * a("adj", 25_000.0);
            r(&[(o, 0.0), (w - o, 0.0), (w, h), (0.0, h)])
        }
        "pentagon" | "homePlate" => {
            let o = ss * a("adj", 50_000.0);
            r(&[(0.0, 0.0), (w - o, 0.0), (w, h * 0.5), (w - o, h), (0.0, h)])
        }
        "chevron" => {
            let o = ss * a("adj", 50_000.0);
            r(&[(0.0, 0.0), (w - o, 0.0), (w, h * 0.5), (w - o, h), (0.0, h), (o, h * 0.5)])
        }
        "regularPentagon" => {
            let pts: Vec<(f32, f32)> = (0..5).map(|i| {
                let t = -PI / 2.0 + i as f32 * 2.0 * PI / 5.0;
                (w * 0.5 + w * 0.5 * t.cos(), h * 0.5 + h * 0.5 * t.sin() + h * 0.05)
            }).collect();
            r(&pts)
        }
        "hexagon" => {
            let o = ss * a("adj", 25_000.0);
            r(&[(o, 0.0), (w - o, 0.0), (w, h * 0.5), (w - o, h), (o, h), (0.0, h * 0.5)])
        }
        "octagon" => {
            let o = ss * a("adj", 29_289.0);
            r(&[(o, 0.0), (w - o, 0.0), (w, o), (w, h - o), (w - o, h), (o, h), (0.0, h - o), (0.0, o)])
        }
        "plus" | "flowChartOr" => {
            let o = ss * a("adj", 25_000.0);
            r(&[(o, 0.0), (w - o, 0.0), (w - o, o), (w, o), (w, h - o), (w - o, h - o), (w - o, h), (o, h), (o, h - o), (0.0, h - o), (0.0, o), (o, o)])
        }
        "star4" => sub(star(w, h, 4, a("adj", 12_500.0) * 2.0)),
        "star5" => sub(star(w, h, 5, 0.38)),
        "star6" => sub(star(w, h, 6, 0.58)),
        "star8" => sub(star(w, h, 8, 0.7)),
        "line" | "straightConnector1" => {
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, 0.0);
            pb.line_to(w, h);
            pb.finish().map(|path| vec![SubPath { path, fill: false, stroke: true, shade: 1.0 }]).unwrap_or_default()
        }
        "bentConnector2" | "bentConnector3" | "bentConnector4" => {
            let x = w * a("adj1", 50_000.0);
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, 0.0);
            if name == "bentConnector2" {
                pb.line_to(w, 0.0);
            } else {
                pb.line_to(x, 0.0);
                pb.line_to(x, h);
            }
            pb.line_to(w, h);
            pb.finish().map(|path| vec![SubPath { path, fill: false, stroke: true, shade: 1.0 }]).unwrap_or_default()
        }
        "curvedConnector3" | "curvedConnector2" => {
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, 0.0);
            pb.cubic_to(w * 0.5, 0.0, w * 0.5, h, w, h);
            pb.finish().map(|path| vec![SubPath { path, fill: false, stroke: true, shade: 1.0 }]).unwrap_or_default()
        }
        "rightArrow" | "leftArrow" | "upArrow" | "downArrow" => {
            // Build pointing right in a (L, T) box, then rotate the points.
            let horiz = matches!(name, "rightArrow" | "leftArrow");
            let (lw, lh) = if horiz { (w, h) } else { (h, w) };
            let s2 = lw.min(lh);
            let th = lh * a("adj1", 50_000.0);
            let head = (s2 * a("adj2", 50_000.0)).min(lw);
            let y0 = (lh - th) * 0.5;
            let pts = [(0.0, y0), (lw - head, y0), (lw - head, 0.0), (lw, lh * 0.5), (lw - head, lh), (lw - head, y0 + th), (0.0, y0 + th)];
            let m: Vec<(f32, f32)> = pts
                .iter()
                .map(|&(x, y)| match name {
                    "rightArrow" => (x, y),
                    "leftArrow" => (w - x, y),
                    "downArrow" => (y, x),
                    _ => (y, h - x),
                })
                .collect();
            r(&m)
        }
        "leftRightArrow" => {
            let th = h * a("adj1", 50_000.0);
            let head = (ss * a("adj2", 50_000.0)).min(w * 0.5);
            let y0 = (h - th) * 0.5;
            r(&[(0.0, h * 0.5), (head, 0.0), (head, y0), (w - head, y0), (w - head, 0.0), (w, h * 0.5), (w - head, h), (w - head, y0 + th), (head, y0 + th), (head, h)])
        }
        "upDownArrow" => {
            let tw = w * a("adj1", 50_000.0);
            let head = (ss * a("adj2", 50_000.0)).min(h * 0.5);
            let x0 = (w - tw) * 0.5;
            r(&[(w * 0.5, 0.0), (w, head), (x0 + tw, head), (x0 + tw, h - head), (w, h - head), (w * 0.5, h), (0.0, h - head), (x0, h - head), (x0, head), (0.0, head)])
        }
        "donut" | "noSmoking" => {
            let t = ss * a("adj", 25_000.0);
            let mut pb = PathBuilder::new();
            ellipse_path(&mut pb, 0.0, 0.0, w, h);
            ellipse_path(&mut pb, t, t, w - 2.0 * t, h - 2.0 * t);
            pb.finish().map(|path| vec![SubPath { path, fill: true, stroke: true, shade: 1.0 }]).unwrap_or_default()
        }
        "frame" => {
            let t = ss * a("adj1", 12_500.0);
            let mut pb = PathBuilder::new();
            pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w.max(0.01), h.max(0.01)).unwrap());
            if let Some(rr) = tiny_skia::Rect::from_xywh(t, t, (w - 2.0 * t).max(0.01), (h - 2.0 * t).max(0.01)) {
                pb.push_rect(rr);
            }
            sub(pb.finish())
        }
        "can" | "flowChartMagneticDisk" => {
            let e = (h * a("adj", 25_000.0) * ss / h).min(h * 0.5);
            let mut body = PathBuilder::new();
            body.move_to(0.0, e * 0.5);
            arc_pts(&mut body, w * 0.5, e * 0.5, w * 0.5, e * 0.5, PI, -PI, false);
            body.line_to(w, h - e * 0.5);
            arc_pts(&mut body, w * 0.5, h - e * 0.5, w * 0.5, e * 0.5, 0.0, PI, false);
            body.close();
            let mut top = PathBuilder::new();
            ellipse_path(&mut top, 0.0, 0.0, w, e);
            let mut v = sub(body.finish());
            if let Some(path) = top.finish() {
                v.push(SubPath { path, fill: true, stroke: true, shade: 1.2 });
            }
            v
        }
        "cube" => {
            let d = ss * a("adj", 25_000.0);
            let mut v = r(&[(0.0, d), (w - d, d), (w - d, h), (0.0, h)]);
            if let Some(p) = poly(&[(0.0, d), (d, 0.0), (w, 0.0), (w - d, d)]) {
                v.push(SubPath { path: p, fill: true, stroke: true, shade: 1.2 });
            }
            if let Some(p) = poly(&[(w - d, d), (w, 0.0), (w, h - d), (w - d, h)]) {
                v.push(SubPath { path: p, fill: true, stroke: true, shade: 0.8 });
            }
            v
        }
        "snip1Rect" => {
            let o = ss * a("adj", 16_667.0);
            r(&[(0.0, 0.0), (w - o, 0.0), (w, o), (w, h), (0.0, h)])
        }
        "snip2SameRect" => {
            let o = ss * a("adj1", 16_667.0);
            r(&[(o, 0.0), (w - o, 0.0), (w, o), (w, h), (0.0, h), (0.0, o)])
        }
        "round1Rect" | "round2SameRect" | "round2DiagRect" => sub(round_rect(w, h, ss * a("adj1", 16_667.0))),
        "wedgeRectCallout" | "wedgeRoundRectCallout" | "wedgeEllipseCallout" | "cloudCallout" => {
            let (tx, ty) = (w * 0.5 + w * a("adj1", -20_833.0), h * 0.5 + h * a("adj2", 62_500.0));
            let mut pb = PathBuilder::new();
            match name {
                "wedgeEllipseCallout" | "cloudCallout" => ellipse_path(&mut pb, 0.0, 0.0, w, h),
                "wedgeRoundRectCallout" => {
                    if let Some(p) = round_rect(w, h, ss * 0.16667) {
                        pb.push_path(&p);
                    }
                }
                _ => pb.push_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, w.max(0.01), h.max(0.01)).unwrap()),
            }
            // Tail triangle from the box edge towards (tx, ty).
            let (cx, cy) = (w * 0.5, h * 0.5);
            let (dx, dy) = (tx - cx, ty - cy);
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            let (nx, ny) = (-dy / len * ss * 0.12, dx / len * ss * 0.12);
            let (bx, by) = (cx + dx * 0.25, cy + dy * 0.25);
            pb.move_to(bx + nx, by + ny);
            pb.line_to(tx, ty);
            pb.line_to(bx - nx, by - ny);
            pb.close();
            sub(pb.finish())
        }
        "heart" => {
            let mut pb = PathBuilder::new();
            pb.move_to(w * 0.5, h * 0.25);
            pb.cubic_to(w * 0.5, -h * 0.1, -w * 0.1, h * 0.05, w * 0.05, h * 0.4);
            pb.line_to(w * 0.5, h);
            pb.line_to(w * 0.95, h * 0.4);
            pb.cubic_to(w * 1.1, h * 0.05, w * 0.5, -h * 0.1, w * 0.5, h * 0.25);
            pb.close();
            sub(pb.finish())
        }
        "arc" => {
            let a0 = adj.get("adj1").copied().unwrap_or(16_200_000.0) / 60_000.0;
            let a1 = adj.get("adj2").copied().unwrap_or(0.0) / 60_000.0;
            let mut sw = a1 - a0;
            if sw <= 0.0 {
                sw += 360.0;
            }
            let mut pb = PathBuilder::new();
            arc_pts(&mut pb, w * 0.5, h * 0.5, w * 0.5, h * 0.5, a0.to_radians(), sw.to_radians(), true);
            pb.finish().map(|path| vec![SubPath { path, fill: false, stroke: true, shade: 1.0 }]).unwrap_or_default()
        }
        "flowChartDocument" => {
            let mut pb = PathBuilder::new();
            pb.move_to(0.0, 0.0);
            pb.line_to(w, 0.0);
            pb.line_to(w, h * 0.83);
            pb.cubic_to(w * 0.75, h * 0.7, w * 0.25, h * 1.05, 0.0, h * 0.9);
            pb.close();
            sub(pb.finish())
        }
        "cloud" => {
            let mut pb = PathBuilder::new();
            for (x, y, rx, ry) in [(0.3, 0.35, 0.22, 0.22), (0.55, 0.28, 0.25, 0.25), (0.75, 0.45, 0.22, 0.22), (0.6, 0.68, 0.24, 0.22), (0.32, 0.65, 0.24, 0.22), (0.18, 0.5, 0.18, 0.18)] {
                ellipse_path(&mut pb, w * (x - rx), h * (y - ry), w * rx * 2.0, h * ry * 2.0);
            }
            sub(pb.finish())
        }
        _ => r(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]),
    }
}

/// Custom geometry (`a:custGeom`) scaled to the w×h box.
pub fn custom(cg: roxmltree::Node, w: f32, h: f32) -> Vec<SubPath> {
    let mut out = Vec::new();
    let Some(list) = cg.child("pathLst") else { return out };
    for p in list.children_named("path") {
        let pw = p.attr_f32("w").filter(|v| *v > 0.0).unwrap_or(w.max(1.0) * 12_700.0);
        let ph = p.attr_f32("h").filter(|v| *v > 0.0).unwrap_or(h.max(1.0) * 12_700.0);
        let (sx, sy) = (w / pw, h / ph);
        let fill = p.attr("fill") != Some("none");
        let stroke = p.attr("stroke") != Some("0") && p.attr("stroke") != Some("false");
        let pt = |n: roxmltree::Node| (n.attr_f32("x").unwrap_or(0.0) * sx, n.attr_f32("y").unwrap_or(0.0) * sy);
        let mut pb = PathBuilder::new();
        let mut cur = (0.0f32, 0.0f32);
        let mut started = false;
        for c in p.children().filter(|c| c.is_element()) {
            let pts: Vec<(f32, f32)> = c.children_named("pt").map(pt).collect();
            match c.tag_name().name() {
                "moveTo" => {
                    if let Some(&q) = pts.first() {
                        pb.move_to(q.0, q.1);
                        cur = q;
                        started = true;
                    }
                }
                "lnTo" => {
                    if let Some(&q) = pts.first() {
                        if !started {
                            pb.move_to(cur.0, cur.1);
                            started = true;
                        }
                        pb.line_to(q.0, q.1);
                        cur = q;
                    }
                }
                "cubicBezTo" if pts.len() == 3 => {
                    if !started {
                        pb.move_to(cur.0, cur.1);
                        started = true;
                    }
                    pb.cubic_to(pts[0].0, pts[0].1, pts[1].0, pts[1].1, pts[2].0, pts[2].1);
                    cur = pts[2];
                }
                "quadBezTo" if pts.len() == 2 => {
                    if !started {
                        pb.move_to(cur.0, cur.1);
                        started = true;
                    }
                    pb.quad_to(pts[0].0, pts[0].1, pts[1].0, pts[1].1);
                    cur = pts[1];
                }
                "arcTo" => {
                    let wr = c.attr_f32("wR").unwrap_or(0.0) * sx;
                    let hr = c.attr_f32("hR").unwrap_or(0.0) * sy;
                    let st = (c.attr_f32("stAng").unwrap_or(0.0) / 60_000.0).to_radians();
                    let sw = (c.attr_f32("swAng").unwrap_or(0.0) / 60_000.0).to_radians();
                    let (cx, cy) = (cur.0 - wr * st.cos(), cur.1 - hr * st.sin());
                    if !started {
                        pb.move_to(cur.0, cur.1);
                        started = true;
                    }
                    arc_pts(&mut pb, cx, cy, wr, hr, st, sw, false);
                    cur = (cx + wr * (st + sw).cos(), cy + hr * (st + sw).sin());
                }
                "close" => pb.close(),
                _ => {}
            }
        }
        if let Some(path) = pb.finish() {
            out.push(SubPath { path, fill, stroke, shade: 1.0 });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_fit_their_box() {
        let adj = HashMap::new();
        for name in ["rect", "roundRect", "ellipse", "triangle", "diamond", "rightArrow", "downArrow", "chevron", "star5", "hexagon", "can", "cube", "wedgeRectCallout", "heart", "cloud", "plus", "donut", "unknownShape"] {
            let g = preset(name, 200.0, 100.0, &adj);
            assert!(!g.is_empty(), "{name}");
            for sp in &g {
                let b = sp.path.bounds();
                assert!(b.left() >= -0.5 && b.right() <= 200.5 && b.bottom() <= 125.0, "{name}: {b:?}");
            }
        }
    }
}
