//! Hand-drawn shape recognition (port of the legacy ShapeRecognizer).

use crate::doc::{InkPoint, ShapeKind};
use crate::geom::{v2, Vec2};

fn path_len(p: &[Vec2]) -> f32 {
    p.windows(2).map(|w| w[0].dist(w[1])).sum()
}

fn bbox(p: &[Vec2]) -> (Vec2, Vec2) {
    let mut mn = v2(f32::MAX, f32::MAX);
    let mut mx = v2(f32::MIN, f32::MIN);
    for q in p {
        mn.x = mn.x.min(q.x);
        mn.y = mn.y.min(q.y);
        mx.x = mx.x.max(q.x);
        mx.y = mx.y.max(q.y);
    }
    (mn, mx)
}

fn resample(input: &[Vec2], step: f32) -> Vec<Vec2> {
    if input.len() < 2 {
        return input.to_vec();
    }
    let mut res = vec![input[0]];
    let mut accum = 0.0f32;
    for i in 1..input.len() {
        let d = input[i] - input[i - 1];
        let l = d.len();
        if l < 1e-6 {
            continue;
        }
        let u = d * (1.0 / l);
        let mut remaining = l;
        let start = step - accum;
        if start > remaining {
            accum += remaining;
            continue;
        }
        let mut p = input[i - 1] + u * start;
        res.push(p);
        remaining -= start;
        while remaining >= step {
            p += u * step;
            res.push(p);
            remaining -= step;
        }
        accum = remaining;
    }
    res
}

fn gaussian(p: &[Vec2]) -> Vec<Vec2> {
    if p.len() < 5 {
        return p.to_vec();
    }
    const K: [f32; 5] = [1.0 / 17.0, 4.0 / 17.0, 7.0 / 17.0, 4.0 / 17.0, 1.0 / 17.0];
    let n = p.len() as i32;
    (0..n)
        .map(|i| {
            let mut s = Vec2::ZERO;
            for j in -2..=2 {
                let idx = (i + j).clamp(0, n - 1) as usize;
                s += p[idx] * K[(j + 2) as usize];
            }
            s
        })
        .collect()
}

fn perp_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let d = b - a;
    let l = d.len();
    if l < 1e-6 {
        return p.dist(a);
    }
    (d.y * p.x - d.x * p.y + b.x * a.y - b.y * a.x).abs() / l
}

fn dp(p: &[Vec2], i: usize, j: usize, eps: f32, keep: &mut Vec<usize>) {
    let mut maxd = 0.0;
    let mut idx = None;
    for k in i + 1..j {
        let d = perp_dist(p[k], p[i], p[j]);
        if d > maxd {
            maxd = d;
            idx = Some(k);
        }
    }
    match idx {
        Some(k) if maxd > eps => {
            dp(p, i, k, eps, keep);
            dp(p, k, j, eps, keep);
        }
        _ => keep.push(j),
    }
}

struct Features {
    closed: bool,
    linearity: f32,
    line_r2: f32,
    end_dist_ratio: f32,
    path_len: f32,
    diag: f32,
    corner_count: i32,
    area_ratio: f32,
    ellipse_err: f32,
    rect_err: f32,
    aspect: f32,
    right_angle: f32,
}

fn features(p: &[Vec2], corners: &[usize]) -> Features {
    let (mn, mx) = bbox(p);
    let (w, h) = (mx.x - mn.x, mx.y - mn.y);
    let diag = (w * w + h * h).sqrt();
    let pl = path_len(p);
    let end = p[0].dist(*p.last().unwrap());
    let closedness = if diag > 1e-3 { end / diag } else { 1.0 };
    let mut linearity = 0.0;
    if end > 1e-3 {
        let maxd = p.iter().map(|q| perp_dist(*q, p[0], *p.last().unwrap())).fold(0.0, f32::max);
        linearity = maxd / end;
    }
    let n = p.len() as f64;
    let (mxm, mym) = p.iter().fold((0.0f64, 0.0f64), |a, q| (a.0 + q.x as f64, a.1 + q.y as f64));
    let (mxm, mym) = (mxm / n, mym / n);
    let (mut sxx, mut syy, mut sxy) = (0.0f64, 0.0f64, 0.0f64);
    for q in p {
        let (dx, dy) = (q.x as f64 - mxm, q.y as f64 - mym);
        sxx += dx * dx;
        syy += dy * dy;
        sxy += dx * dy;
    }
    let line_r2 = if sxx * syy > 1e-6 { (sxy * sxy / (sxx * syy)) as f32 } else { 0.0 };
    let mut area = 0.0f64;
    for i in 0..p.len() {
        let j = (i + 1) % p.len();
        area += p[i].x as f64 * p[j].y as f64 - p[j].x as f64 * p[i].y as f64;
    }
    let area = area.abs() * 0.5;
    let ba = w * h;
    let c = v2((mn.x + mx.x) * 0.5, (mn.y + mx.y) * 0.5);
    let (rx, ry) = ((w * 0.5).max(1.0), (h * 0.5).max(1.0));
    let ellipse_err = p
        .iter()
        .map(|q| {
            let (nx, ny) = ((q.x - c.x) / rx, (q.y - c.y) / ry);
            ((nx * nx + ny * ny).sqrt() - 1.0).abs()
        })
        .sum::<f32>()
        / p.len() as f32;
    let rect_err = p
        .iter()
        .map(|q| (q.x - mn.x).abs().min((q.x - mx.x).abs()).min((q.y - mn.y).abs()).min((q.y - mx.y).abs()))
        .sum::<f32>()
        / p.len() as f32
        / diag.max(1.0);
    let corner_count = corners.len() as i32 - 1;
    let mut right_angle = 0.0;
    if corner_count >= 4 && corners.len() >= 5 {
        let (mut tot, mut pairs) = (0.0f32, 0);
        for i in 0..corners.len() - 2 {
            let (a, b, cc) = (p[corners[i]], p[corners[i + 1]], p[corners[i + 2]]);
            let (v1, v2_) = (b - a, cc - b);
            if v1.len() > 1e-3 && v2_.len() > 1e-3 {
                tot += (v1.dot(v2_) / (v1.len() * v2_.len())).abs();
                pairs += 1;
            }
        }
        if pairs > 0 {
            right_angle = 1.0 - tot / pairs as f32;
        }
    }
    Features {
        closed: closedness < 0.20,
        linearity,
        line_r2,
        end_dist_ratio: if pl > 1e-3 { end / pl } else { 0.0 },
        path_len: pl,
        diag,
        corner_count,
        area_ratio: if ba > 1e-3 { (area / ba as f64) as f32 } else { 0.0 },
        ellipse_err,
        rect_err,
        aspect: if h > 1e-3 { w / h } else { 1.0 },
        right_angle,
    }
}

fn rule_decide(f: &Features) -> (Option<ShapeKind>, f32) {
    if !f.closed {
        if f.linearity < 0.08 && f.line_r2 > 0.95 && f.path_len > 30.0 {
            return (Some(ShapeKind::Line), 0.95);
        }
        if f.linearity < 0.12 && f.line_r2 > 0.90 && f.end_dist_ratio > 0.85 {
            return (Some(ShapeKind::Line), 0.80);
        }
        return (None, 0.0);
    }
    if f.diag < 12.0 {
        return (None, 0.0);
    }
    if f.ellipse_err < 0.12 {
        let _ar = f.aspect.max(1.0 / f.aspect.max(0.01));
        return (Some(ShapeKind::Ellipse), 0.92);
    }
    if f.corner_count == 3 {
        return (Some(ShapeKind::Triangle), 0.88);
    }
    if f.corner_count == 4 {
        if f.right_angle > 0.65 && f.rect_err < 0.06 {
            return (Some(ShapeKind::Rect), 0.92);
        }
        if f.rect_err < 0.08 {
            return (Some(ShapeKind::Rect), 0.80);
        }
    }
    if f.corner_count >= 5 && f.ellipse_err < 0.18 && f.area_ratio > 0.65 {
        return (Some(ShapeKind::Ellipse), 0.78);
    }
    if f.corner_count == 4 && f.area_ratio > 0.5 {
        return (Some(ShapeKind::Rect), 0.70);
    }
    (None, 0.0)
}

/// Recognize a stroke as a regular shape. Returns kind and its (a, b) extent:
/// bounding box corners, or start/end for lines.
pub fn recognize(raw: &[InkPoint]) -> Option<(ShapeKind, Vec2, Vec2)> {
    if raw.len() < 6 {
        return None;
    }
    let pts: Vec<Vec2> = raw.iter().map(|p| p.pos()).collect();
    let p = gaussian(&resample(&pts, 3.0));
    if p.len() < 3 {
        return None;
    }
    let (mn, mx) = bbox(&p);
    let diag = mn.dist(mx);
    if diag < 12.0 {
        return None;
    }
    let mut corners = vec![0usize];
    dp(&p, 0, p.len() - 1, diag * 0.035, &mut corners);
    let f = features(&p, &corners);
    let (k, conf) = rule_decide(&f);
    let kind = if conf >= 0.70 { k? } else { return None };
    if kind == ShapeKind::Line {
        Some((kind, pts[0], *pts.last().unwrap()))
    } else {
        Some((kind, mn, mx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pts(f: impl Fn(f32) -> (f32, f32), n: usize) -> Vec<InkPoint> {
        (0..=n).map(|i| {
            let (x, y) = f(i as f32 / n as f32);
            InkPoint::new(x, y)
        }).collect()
    }
    #[test]
    fn line() {
        let p = pts(|t| (t * 200.0, t * 50.0 + (t * 40.0).sin()), 60);
        assert_eq!(recognize(&p).map(|r| r.0), Some(ShapeKind::Line));
    }
    #[test]
    fn circle() {
        let p = pts(|t| {
            let a = t * std::f32::consts::TAU;
            (100.0 + 80.0 * a.cos(), 100.0 + 80.0 * a.sin())
        }, 80);
        assert_eq!(recognize(&p).map(|r| r.0), Some(ShapeKind::Ellipse));
    }
    #[test]
    fn rect() {
        let corners = [(0.0, 0.0), (200.0, 0.0), (200.0, 120.0), (0.0, 120.0), (0.0, 0.0)];
        let mut p = Vec::new();
        for w in corners.windows(2) {
            for i in 0..30 {
                let t = i as f32 / 30.0;
                p.push(InkPoint::new(w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t));
            }
        }
        assert_eq!(recognize(&p).map(|r| r.0), Some(ShapeKind::Rect));
    }
    #[test]
    fn scribble_is_free() {
        let p = pts(|t| (t * 300.0, (t * 60.0).sin() * 40.0), 120);
        assert_eq!(recognize(&p), None);
    }
}
