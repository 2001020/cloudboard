//! Small 2D geometry helpers shared by every crate.

use serde::{Deserialize, Serialize};
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

#[inline]
pub const fn v2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

impl Vec2 {
    pub const ZERO: Vec2 = v2(0.0, 0.0);
    #[inline]
    pub fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    #[inline]
    pub fn len_sq(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    #[inline]
    pub fn dot(self, o: Vec2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    #[inline]
    pub fn cross(self, o: Vec2) -> f32 {
        self.x * o.y - self.y * o.x
    }
    #[inline]
    pub fn dist(self, o: Vec2) -> f32 {
        (self - o).len()
    }
    #[inline]
    pub fn normalized(self) -> Vec2 {
        let l = self.len();
        if l < 1e-9 {
            Vec2::ZERO
        } else {
            v2(self.x / l, self.y / l)
        }
    }
    #[inline]
    pub fn perp(self) -> Vec2 {
        v2(-self.y, self.x)
    }
    #[inline]
    pub fn rotate(self, ang: f32) -> Vec2 {
        let (s, c) = ang.sin_cos();
        v2(self.x * c - self.y * s, self.x * s + self.y * c)
    }
    #[inline]
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }
    #[inline]
    pub fn from_angle(a: f32) -> Vec2 {
        let (s, c) = a.sin_cos();
        v2(c, s)
    }
    #[inline]
    pub fn lerp(self, o: Vec2, t: f32) -> Vec2 {
        self + (o - self) * t
    }
}

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        self.x += o.x;
        self.y += o.y;
    }
}
impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        self.x -= o.x;
        self.y -= o.y;
    }
}
impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, s: f32) -> Vec2 {
        v2(self.x * s, self.y * s)
    }
}
impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        v2(-self.x, -self.y)
    }
}

/// Axis aligned rectangle (min inclusive, max exclusive for pixel use).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Rect {
    pub const EMPTY: Rect = Rect { x0: f32::INFINITY, y0: f32::INFINITY, x1: f32::NEG_INFINITY, y1: f32::NEG_INFINITY };
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect { x0, y0, x1, y1 }
    }
    pub fn xywh(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x0: x, y0: y, x1: x + w, y1: y + h }
    }
    pub fn from_points(a: Vec2, b: Vec2) -> Rect {
        Rect { x0: a.x.min(b.x), y0: a.y.min(b.y), x1: a.x.max(b.x), y1: a.y.max(b.y) }
    }
    pub fn w(&self) -> f32 {
        self.x1 - self.x0
    }
    pub fn h(&self) -> f32 {
        self.y1 - self.y0
    }
    pub fn center(&self) -> Vec2 {
        v2((self.x0 + self.x1) * 0.5, (self.y0 + self.y1) * 0.5)
    }
    pub fn is_empty(&self) -> bool {
        !(self.x1 > self.x0 && self.y1 > self.y0)
    }
    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x0 && p.x < self.x1 && p.y >= self.y0 && p.y < self.y1
    }
    pub fn include(&mut self, p: Vec2) {
        self.x0 = self.x0.min(p.x);
        self.y0 = self.y0.min(p.y);
        self.x1 = self.x1.max(p.x);
        self.y1 = self.y1.max(p.y);
    }
    pub fn union(&self, o: &Rect) -> Rect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Rect { x0: self.x0.min(o.x0), y0: self.y0.min(o.y0), x1: self.x1.max(o.x1), y1: self.y1.max(o.y1) }
    }
    pub fn intersect(&self, o: &Rect) -> Rect {
        Rect { x0: self.x0.max(o.x0), y0: self.y0.max(o.y0), x1: self.x1.min(o.x1), y1: self.y1.min(o.y1) }
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        !self.intersect(o).is_empty()
    }
    pub fn inflate(&self, d: f32) -> Rect {
        Rect { x0: self.x0 - d, y0: self.y0 - d, x1: self.x1 + d, y1: self.y1 + d }
    }
    pub fn translate(&self, d: Vec2) -> Rect {
        Rect { x0: self.x0 + d.x, y0: self.y0 + d.y, x1: self.x1 + d.x, y1: self.y1 + d.y }
    }
    pub fn round_out(&self) -> Rect {
        Rect { x0: self.x0.floor(), y0: self.y0.floor(), x1: self.x1.ceil(), y1: self.y1.ceil() }
    }
}

/// Distance from p to segment ab, plus the projection parameter t (clamped 0..1).
pub fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, f32) {
    let ab = b - a;
    let l2 = ab.len_sq();
    if l2 < 1e-12 {
        return (p.dist(a), 0.0);
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p.dist(a + ab * t), t)
}

/// Point-in-polygon (even-odd).
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Wrap an angle delta into (-PI, PI].
pub fn normalize_angle_delta(mut d: f32) -> f32 {
    use std::f32::consts::PI;
    while d > PI {
        d -= 2.0 * PI;
    }
    while d <= -PI {
        d += 2.0 * PI;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn segment_distance() {
        let (d, t) = dist_to_segment(v2(5.0, 3.0), v2(0.0, 0.0), v2(10.0, 0.0));
        assert!((d - 3.0).abs() < 1e-5 && (t - 0.5).abs() < 1e-5);
    }
    #[test]
    fn polygon() {
        let sq = [v2(0.0, 0.0), v2(10.0, 0.0), v2(10.0, 10.0), v2(0.0, 10.0)];
        assert!(point_in_polygon(v2(5.0, 5.0), &sq));
        assert!(!point_in_polygon(v2(15.0, 5.0), &sq));
    }
    #[test]
    fn angle_wrap() {
        assert!((normalize_angle_delta(3.0 * std::f32::consts::PI) - std::f32::consts::PI).abs() < 1e-4);
    }
}
