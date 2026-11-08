//! Ink input processing: point filtering while drawing and curve smoothing
//! for rendering (ported and refined from the legacy Board.cpp).

use crate::doc::InkPoint;
use crate::geom::{v2, Vec2};

/// Incrementally builds a stroke from raw pointer samples.
#[derive(Debug, Clone)]
pub struct InkBuilder {
    pub points: Vec<InkPoint>,
    /// Minimum distance between kept points in board units.
    pub min_dist: f32,
    smoothed_pressure: f32,
}

impl InkBuilder {
    pub fn new(zoom: f32) -> InkBuilder {
        InkBuilder { points: Vec::with_capacity(256), min_dist: 0.55 / zoom.max(0.05), smoothed_pressure: -1.0 }
    }

    /// Add a sample. Returns true if the point list changed.
    pub fn push(&mut self, x: f32, y: f32, pressure: f32, dt_ms: u16) -> bool {
        let pr = pressure.clamp(0.05, 1.0);
        self.smoothed_pressure = if self.smoothed_pressure < 0.0 { pr } else { self.smoothed_pressure * 0.6 + pr * 0.4 };
        let p = InkPoint { x, y, pressure: self.smoothed_pressure, dt_ms };
        let n = self.points.len();
        if n == 0 {
            self.points.push(p);
            return true;
        }
        let last = self.points[n - 1];
        if last.pos().dist(p.pos()) < self.min_dist {
            return false;
        }
        // Merge nearly collinear runs to keep strokes light.
        if n >= 2 {
            let prev = self.points[n - 2];
            let d1 = (last.pos() - prev.pos()).normalized();
            let d2 = (p.pos() - last.pos()).normalized();
            if d1.dot(d2) > 0.9995 && (last.pressure - p.pressure).abs() < 0.02 && last.pos().dist(prev.pos()) < 40.0 {
                let dt = last.dt_ms.saturating_add(p.dt_ms);
                self.points[n - 1] = InkPoint { dt_ms: dt, ..p };
                return true;
            }
        }
        self.points.push(p);
        true
    }
}

/// Smooth a polyline with centripetal-ish Catmull-Rom, refusing to smooth
/// across sharp corners or long jumps (which would overshoot).
pub fn smooth(points: &[InkPoint], width: f32) -> Vec<InkPoint> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }
    let long_seg = (7.0 * width).max(18.0);
    let mut out = Vec::with_capacity(n * 4);
    out.push(points[0]);
    for i in 0..n - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < n { points[i + 2] } else { points[n - 1] };
        let seg = p1.pos().dist(p2.pos());
        let sharp = |a: Vec2, b: Vec2, c: Vec2| {
            let (d1, d2) = ((b - a).normalized(), (c - b).normalized());
            d1.dot(d2) < 0.62
        };
        let corner = sharp(p0.pos(), p1.pos(), p2.pos()) || sharp(p1.pos(), p2.pos(), p3.pos());
        if seg > long_seg || corner || seg < 1.0 {
            out.push(p2);
            continue;
        }
        let steps = (seg / 2.5).ceil().clamp(1.0, 12.0) as usize;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * ((2.0 * b) + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            let x = f(p0.x, p1.x, p2.x, p3.x);
            let y = f(p0.y, p1.y, p2.y, p3.y);
            let pr = p1.pressure + (p2.pressure - p1.pressure) * t;
            out.push(InkPoint { x, y, pressure: pr, dt_ms: 0 });
        }
    }
    out
}

/// Width at a point given pressure; pressure maps to 45%..115% of nominal width.
pub fn pressure_width(width: f32, pressure: f32, use_pressure: bool) -> f32 {
    if !use_pressure {
        return width;
    }
    width * (0.45 + 0.7 * pressure.clamp(0.0, 1.0))
}

/// Build a filled outline polygon for a variable-width stroke.
/// Returns (left side, right side) offsets; the renderer joins them with
/// round caps.
pub fn variable_outline(pts: &[InkPoint], width: f32, use_pressure: bool) -> Vec<Vec2> {
    if pts.len() < 2 {
        return vec![];
    }
    let mut left = Vec::with_capacity(pts.len());
    let mut right = Vec::with_capacity(pts.len());
    for i in 0..pts.len() {
        let a = if i == 0 { pts[0].pos() } else { pts[i - 1].pos() };
        let b = if i + 1 < pts.len() { pts[i + 1].pos() } else { pts[i].pos() };
        let mut d = (b - a).normalized();
        if d == Vec2::ZERO {
            d = v2(1.0, 0.0);
        }
        let n = d.perp();
        let hw = pressure_width(width, pts[i].pressure, use_pressure) * 0.5;
        left.push(pts[i].pos() + n * hw);
        right.push(pts[i].pos() - n * hw);
    }
    right.reverse();
    left.extend(right);
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_close_points() {
        let mut b = InkBuilder::new(1.0);
        assert!(b.push(0.0, 0.0, 1.0, 0));
        assert!(!b.push(0.1, 0.0, 1.0, 1));
        assert!(b.push(5.0, 0.0, 1.0, 1));
        assert!(b.push(10.0, 0.0, 1.0, 1)); // merged (collinear)
        assert_eq!(b.points.len(), 2);
        assert_eq!(b.points[1].x, 10.0);
        b.push(10.0, 10.0, 1.0, 1);
        assert_eq!(b.points.len(), 3);
    }
    #[test]
    fn smoothing_keeps_endpoints() {
        let pts: Vec<InkPoint> = (0..10).map(|i| InkPoint::new(i as f32 * 5.0, ((i as f32) * 0.7).sin() * 5.0)).collect();
        let s = smooth(&pts, 4.0);
        assert!(s.len() > pts.len());
        assert_eq!(s.first().unwrap().pos(), pts[0].pos());
        assert!(s.last().unwrap().pos().dist(pts[9].pos()) < 1e-3);
    }
}
