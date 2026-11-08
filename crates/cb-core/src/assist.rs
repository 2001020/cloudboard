//! Geometry for the teaching aids: ruler, set squares (45° / 30°-60°),
//! protractor and compass. Everything is in board units; rendering lives in
//! cb-render, interaction in cb-ui.

use crate::geom::{dist_to_segment, normalize_angle_delta, point_in_polygon, v2, Vec2};
use serde::{Deserialize, Serialize};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Board units per centimetre: 1 board unit = 1 CSS pixel = 1/96 inch.
pub const PX_PER_CM: f32 = 96.0 / 2.54;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssistKind {
    Ruler,
    Triangle45,
    Triangle30,
    Protractor,
    Compass,
}

impl AssistKind {
    pub const ALL: [AssistKind; 5] = [AssistKind::Ruler, AssistKind::Triangle45, AssistKind::Triangle30, AssistKind::Protractor, AssistKind::Compass];
    pub fn label(self) -> &'static str {
        match self {
            AssistKind::Ruler => "直尺",
            AssistKind::Triangle45 => "45°三角板",
            AssistKind::Triangle30 => "30°三角板",
            AssistKind::Protractor => "量角器",
            AssistKind::Compass => "圆规",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AssistTool {
    pub kind: AssistKind,
    /// Anchor in board space: ruler/triangle = scale origin corner,
    /// protractor = centre, compass = needle point.
    pub pos: Vec2,
    pub angle: f32,
    pub scale: f32,
    /// Compass radius (board units).
    pub radius: f32,
    /// Protractor pointer reading, radians in [0, PI].
    pub reading: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssistHit {
    Body,
    Rotate,
    Close,
    /// A straight edge usable for ruling lines.
    Edge(usize),
    CompassNeedle,
    CompassPencil,
    CompassHandle,
    ProtractorPointer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    pub a: Vec2,
    pub b: Vec2,
    /// Unit normal pointing away from the tool body.
    pub outward: Vec2,
}

impl Edge {
    pub fn len(&self) -> f32 {
        self.a.dist(self.b)
    }
    pub fn dir(&self) -> Vec2 {
        (self.b - self.a).normalized()
    }
    /// Project p onto the edge, clamped to its extent; returns (point, t in units from a).
    pub fn project(&self, p: Vec2) -> (Vec2, f32) {
        let d = self.dir();
        let t = (p - self.a).dot(d).clamp(0.0, self.len());
        (self.a + d * t, t)
    }
}

/// A tick mark for rendering: from `a` to `b`, with optional label at `label_pos`.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub a: Vec2,
    pub b: Vec2,
    pub major: bool,
    pub label: Option<(String, Vec2)>,
}

impl AssistTool {
    pub fn new(kind: AssistKind, at: Vec2) -> AssistTool {
        let mut t = AssistTool { kind, pos: at, angle: 0.0, scale: 1.0, radius: 4.0 * PX_PER_CM, reading: PI / 3.0 };
        // Place so the tool body is roughly centred on `at`.
        let c = t.local_centroid();
        t.pos = at - c;
        if kind == AssistKind::Compass {
            t.pos = at - v2(t.radius * 0.5, 0.0);
            t.angle = 0.0;
        }
        t
    }

    fn cm(&self, v: f32) -> f32 {
        v * PX_PER_CM * self.scale
    }

    pub fn to_world(&self, local: Vec2) -> Vec2 {
        self.pos + local.rotate(self.angle)
    }
    pub fn to_local(&self, world: Vec2) -> Vec2 {
        (world - self.pos).rotate(-self.angle)
    }

    /// Outer polygon in local coordinates (empty for the compass).
    pub fn local_outline(&self) -> Vec<Vec2> {
        match self.kind {
            AssistKind::Ruler => {
                let (l, w) = (self.cm(Self::RULER_LEN_CM + 1.0), self.cm(3.0));
                let p = self.cm(0.5);
                vec![v2(-p, 0.0), v2(l - p, 0.0), v2(l - p, w), v2(-p, w)]
            }
            AssistKind::Triangle45 => {
                let l = self.cm(Self::TRI45_LEG_CM);
                vec![v2(0.0, 0.0), v2(l, 0.0), v2(0.0, -l)]
            }
            AssistKind::Triangle30 => {
                let l = self.cm(Self::TRI30_LEG_CM);
                vec![v2(0.0, 0.0), v2(l, 0.0), v2(0.0, -l * (PI / 6.0).tan())]
            }
            AssistKind::Protractor => {
                let r = self.cm(Self::PROTRACTOR_R_CM);
                let base = self.cm(0.6);
                let mut v = vec![v2(-r, base), v2(r, base)];
                let n = 72;
                for i in 0..=n {
                    let a = i as f32 / n as f32 * PI;
                    v.push(v2(r * a.cos(), -r * a.sin()));
                }
                v
            }
            AssistKind::Compass => vec![],
        }
    }

    /// Inner cut-out (holes are transparent to input).
    pub fn local_hole(&self) -> Vec<Vec2> {
        match self.kind {
            AssistKind::Triangle45 | AssistKind::Triangle30 => {
                let o = self.local_outline();
                inset_triangle(&o, self.cm(2.3))
            }
            AssistKind::Protractor => {
                let r = self.cm(Self::PROTRACTOR_R_CM - 2.6);
                if r <= 0.0 {
                    return vec![];
                }
                let base = -self.cm(0.9);
                let mut v = vec![v2(-r, base), v2(r, base)];
                let ang0 = (base.abs() / r).asin();
                let n = 48;
                for i in 0..=n {
                    let a = ang0 + i as f32 / n as f32 * (PI - 2.0 * ang0);
                    v.push(v2(r * a.cos(), -r * a.sin()));
                }
                v
            }
            _ => vec![],
        }
    }

    pub const RULER_LEN_CM: f32 = 20.0;
    pub const TRI45_LEG_CM: f32 = 12.0;
    pub const TRI30_LEG_CM: f32 = 15.0;
    pub const PROTRACTOR_R_CM: f32 = 8.0;

    pub fn outline(&self) -> Vec<Vec2> {
        self.local_outline().into_iter().map(|p| self.to_world(p)).collect()
    }
    pub fn hole(&self) -> Vec<Vec2> {
        self.local_hole().into_iter().map(|p| self.to_world(p)).collect()
    }

    fn local_centroid(&self) -> Vec2 {
        let o = self.local_outline();
        if o.is_empty() {
            return Vec2::ZERO;
        }
        let s = o.iter().fold(Vec2::ZERO, |a, p| a + *p);
        s * (1.0 / o.len() as f32)
    }

    /// Rotation handle position (world).
    pub fn rotate_handle(&self) -> Vec2 {
        match self.kind {
            AssistKind::Ruler => self.to_world(v2(self.cm(Self::RULER_LEN_CM - 0.5), self.cm(1.5))),
            AssistKind::Triangle45 => self.to_world(v2(self.cm(Self::TRI45_LEG_CM * 0.62), -self.cm(1.1))),
            AssistKind::Triangle30 => self.to_world(v2(self.cm(Self::TRI30_LEG_CM * 0.70), -self.cm(0.9))),
            AssistKind::Protractor => self.to_world(v2(0.0, -self.cm(Self::PROTRACTOR_R_CM * 0.30))),
            AssistKind::Compass => self.compass_handle(),
        }
    }

    pub fn close_button(&self) -> Vec2 {
        match self.kind {
            AssistKind::Ruler => self.to_world(v2(self.cm(Self::RULER_LEN_CM - 1.6), self.cm(1.5))),
            AssistKind::Triangle45 => self.to_world(v2(self.cm(1.1), -self.cm(Self::TRI45_LEG_CM * 0.62))),
            AssistKind::Triangle30 => self.to_world(v2(self.cm(1.1), -self.cm(Self::TRI30_LEG_CM * 0.27))),
            AssistKind::Protractor => self.to_world(v2(self.cm(Self::PROTRACTOR_R_CM - 1.0), -self.cm(0.2))),
            AssistKind::Compass => self.compass_hinge() + v2(self.cm(1.4), 0.0).rotate(self.angle),
        }
    }

    pub fn handle_radius(&self) -> f32 {
        (0.55 * PX_PER_CM).max(14.0)
    }

    // ---------- compass ----------
    pub fn compass_needle(&self) -> Vec2 {
        self.pos
    }
    pub fn compass_pencil(&self) -> Vec2 {
        self.pos + Vec2::from_angle(self.angle) * self.radius
    }
    pub fn compass_leg_len(&self) -> f32 {
        (9.0 * PX_PER_CM).max(self.radius * 0.5 * 1.08)
    }
    pub fn compass_hinge(&self) -> Vec2 {
        let half = self.radius * 0.5;
        let l = self.compass_leg_len();
        let h = (l * l - half * half).max(0.0).sqrt();
        let mid = self.pos.lerp(self.compass_pencil(), 0.5);
        // Hinge sits "above" the legs (towards -perp so it is up when angle = 0).
        mid - Vec2::from_angle(self.angle).perp() * h
    }
    pub fn compass_handle(&self) -> Vec2 {
        let hinge = self.compass_hinge();
        let up = (hinge - self.pos.lerp(self.compass_pencil(), 0.5)).normalized();
        hinge + up * (1.2 * PX_PER_CM)
    }
    pub const COMPASS_MIN_R: f32 = 0.5 * PX_PER_CM;
    pub const COMPASS_MAX_R: f32 = 30.0 * PX_PER_CM;

    /// Straight edges usable for ruling.
    pub fn edges(&self) -> Vec<Edge> {
        let mk = |a: Vec2, b: Vec2, inside: Vec2| {
            let n = (b - a).perp().normalized();
            let outward = if (inside - a).dot(n) > 0.0 { -n } else { n };
            Edge { a, b, outward }
        };
        let c = self.to_world(self.local_centroid());
        match self.kind {
            AssistKind::Ruler | AssistKind::Triangle45 | AssistKind::Triangle30 => {
                let o = self.outline();
                (0..o.len()).map(|i| mk(o[i], o[(i + 1) % o.len()], c)).collect()
            }
            AssistKind::Protractor => {
                let o = self.outline();
                let mut v = vec![mk(o[0], o[1], self.to_world(v2(0.0, -self.cm(2.0))))];
                // Pointer arm: from centre outwards at `reading`.
                let tip = self.to_world(Vec2::from_angle(-self.reading) * self.cm(Self::PROTRACTOR_R_CM + 1.5));
                let n = (tip - self.pos).perp().normalized();
                v.push(Edge { a: self.pos, b: tip, outward: n });
                v
            }
            AssistKind::Compass => vec![],
        }
    }

    pub fn protractor_pointer_tip(&self) -> Vec2 {
        self.to_world(Vec2::from_angle(-self.reading) * self.cm(Self::PROTRACTOR_R_CM + 1.5))
    }

    /// Whether `p` lies on the solid body (outline minus hole).
    pub fn body_contains(&self, p: Vec2) -> bool {
        if self.kind == AssistKind::Compass {
            let (needle, pencil, hinge) = (self.compass_needle(), self.compass_pencil(), self.compass_hinge());
            let w = 0.45 * PX_PER_CM;
            return dist_to_segment(p, needle, hinge).0 < w || dist_to_segment(p, pencil, hinge).0 < w;
        }
        let o = self.outline();
        if !point_in_polygon(p, &o) {
            return false;
        }
        let h = self.hole();
        !(h.len() >= 3 && point_in_polygon(p, &h))
    }

    /// Hit test. `edge_tol` = distance within which a pen snaps to an edge.
    /// `for_pen` selects ruling behaviour (edges win over the body).
    pub fn hit(&self, p: Vec2, edge_tol: f32, for_pen: bool) -> Option<AssistHit> {
        let hr = self.handle_radius();
        if p.dist(self.close_button()) < hr * 0.9 {
            return Some(AssistHit::Close);
        }
        if self.kind == AssistKind::Compass {
            if p.dist(self.compass_handle()) < hr * 1.2 {
                return Some(AssistHit::CompassHandle);
            }
            if p.dist(self.compass_pencil()) < hr * 1.3 {
                return Some(AssistHit::CompassPencil);
            }
            if p.dist(self.compass_needle()) < hr * 1.3 {
                return Some(AssistHit::CompassNeedle);
            }
            if self.body_contains(p) {
                return Some(AssistHit::Body);
            }
            return None;
        }
        if p.dist(self.rotate_handle()) < hr {
            return Some(AssistHit::Rotate);
        }
        if self.kind == AssistKind::Protractor && dist_to_segment(p, self.pos, self.protractor_pointer_tip()).0 < hr * 0.6
            && p.dist(self.pos) > self.cm(1.5)
            && !for_pen
        {
            return Some(AssistHit::ProtractorPointer);
        }
        if for_pen {
            if let Some((i, _)) = self.nearest_edge(p, edge_tol) {
                return Some(AssistHit::Edge(i));
            }
        }
        if self.body_contains(p) {
            return Some(AssistHit::Body);
        }
        None
    }

    /// Nearest ruling edge within `tol` of p (outside or on the edge).
    pub fn nearest_edge(&self, p: Vec2, tol: f32) -> Option<(usize, f32)> {
        let mut best: Option<(usize, f32)> = None;
        for (i, e) in self.edges().iter().enumerate() {
            let (d, t) = dist_to_segment(p, e.a, e.b);
            // Skip near the very ends so corners stay grabbable.
            if d < tol && t > 0.0 && t < 1.0 && best.map_or(true, |b| d < b.1) {
                best = Some((i, d));
            }
        }
        best
    }

    /// Ruled line along edge `i` between the projections of `from` and `to`,
    /// offset outward by `half_width` so ink sits next to the edge.
    pub fn rule(&self, i: usize, from: Vec2, to: Vec2, half_width: f32) -> Option<(Vec2, Vec2)> {
        let e = *self.edges().get(i)?;
        let (a, _) = e.project(from);
        let (b, _) = e.project(to);
        let off = e.outward * half_width;
        Some((a + off, b + off))
    }

    /// Ruling readout: length in cm between two points.
    pub fn length_cm(a: Vec2, b: Vec2) -> f32 {
        a.dist(b) / PX_PER_CM
    }

    /// Tick marks for rendering.
    pub fn ticks(&self) -> Vec<Tick> {
        let mut out = Vec::new();
        let mm = self.cm(0.1);
        let along = |out: &mut Vec<Tick>, origin: Vec2, dir: Vec2, inward: Vec2, len_cm: f32, from_zero: bool| {
            let n = (len_cm * 10.0).round() as i32;
            for i in 0..=n {
                let major = i % 10 == 0;
                let half = i % 5 == 0;
                let l = if major { 0.55 } else if half { 0.38 } else { 0.22 } * PX_PER_CM * self.scale;
                let a = origin + dir * (i as f32 * mm);
                let b = a + inward * l;
                let label = if major && (from_zero || i > 0) {
                    Some(((i / 10).to_string(), a + inward * (l + 0.32 * PX_PER_CM * self.scale)))
                } else {
                    None
                };
                out.push(Tick { a: self.to_world(a), b: self.to_world(b), major, label: label.map(|(s, p)| (s, self.to_world(p))) });
            }
        };
        match self.kind {
            AssistKind::Ruler => {
                along(&mut out, v2(0.0, 0.0), v2(1.0, 0.0), v2(0.0, 1.0), Self::RULER_LEN_CM, true);
            }
            AssistKind::Triangle45 => {
                let l = Self::TRI45_LEG_CM - 0.5;
                along(&mut out, v2(0.0, 0.0), v2(1.0, 0.0), v2(0.0, -1.0), l.floor(), true);
                along(&mut out, v2(0.0, 0.0), v2(0.0, -1.0), v2(1.0, 0.0), l.floor(), false);
            }
            AssistKind::Triangle30 => {
                along(&mut out, v2(0.0, 0.0), v2(1.0, 0.0), v2(0.0, -1.0), (Self::TRI30_LEG_CM - 0.5).floor(), true);
                let short = Self::TRI30_LEG_CM * (PI / 6.0).tan();
                along(&mut out, v2(0.0, 0.0), v2(0.0, -1.0), v2(1.0, 0.0), (short - 0.6).floor(), false);
            }
            AssistKind::Protractor => {
                let r = self.cm(Self::PROTRACTOR_R_CM);
                for deg in 0..=180 {
                    let a = (deg as f32).to_radians();
                    let dir = v2(a.cos(), -a.sin());
                    let major = deg % 10 == 0;
                    let l = if major { 0.7 } else if deg % 5 == 0 { 0.45 } else { 0.25 } * PX_PER_CM * self.scale;
                    let p0 = dir * r;
                    let p1 = dir * (r - l);
                    let label = if major { Some((deg.to_string(), dir * (r - l - 0.45 * PX_PER_CM * self.scale))) } else { None };
                    out.push(Tick { a: self.to_world(p0), b: self.to_world(p1), major, label: label.map(|(s, p)| (s, self.to_world(p))) });
                }
            }
            AssistKind::Compass => {}
        }
        out
    }

    /// Angle labels at triangle vertices (world position, text).
    pub fn angle_labels(&self) -> Vec<(Vec2, String)> {
        let o = self.local_outline();
        let inset = |p: Vec2, k: f32| {
            let c = self.local_centroid();
            self.to_world(p + (c - p).normalized() * self.cm(k))
        };
        match self.kind {
            AssistKind::Triangle45 => vec![(inset(o[1], 2.2), "45°".into()), (inset(o[2], 2.2), "45°".into())],
            AssistKind::Triangle30 => vec![(inset(o[1], 3.2), "30°".into()), (inset(o[2], 1.6), "60°".into())],
            _ => vec![],
        }
    }

    /// Protractor reading in degrees (0..180).
    pub fn reading_deg(&self) -> f32 {
        self.reading.to_degrees()
    }

    // ---------- manipulation ----------
    pub fn translate(&mut self, d: Vec2) {
        self.pos += d;
    }

    /// Rotate around `pivot` by `da` radians.
    pub fn rotate_about(&mut self, pivot: Vec2, da: f32) {
        self.pos = pivot + (self.pos - pivot).rotate(da);
        self.angle = (self.angle + da).rem_euclid(TAU);
    }

    pub fn pivot(&self) -> Vec2 {
        match self.kind {
            AssistKind::Compass => self.pos,
            AssistKind::Protractor => self.pos,
            _ => self.to_world(self.local_centroid()),
        }
    }

    /// Scale around pivot (pinch); clamped to 0.5..2.5.
    pub fn scale_about(&mut self, pivot: Vec2, factor: f32) {
        if self.kind == AssistKind::Compass {
            self.set_radius(self.radius * factor);
            return;
        }
        let ns = (self.scale * factor).clamp(0.5, 2.5);
        let f = ns / self.scale;
        self.pos = pivot + (self.pos - pivot) * f;
        self.scale = ns;
    }

    pub fn set_radius(&mut self, r: f32) {
        self.radius = r.clamp(Self::COMPASS_MIN_R, Self::COMPASS_MAX_R);
    }

    /// Move the compass pencil leg to `p` (sets radius and angle).
    pub fn drag_pencil_to(&mut self, p: Vec2) {
        let d = p - self.pos;
        self.set_radius(d.len());
        if d.len() > 1.0 {
            self.angle = d.angle();
        }
    }

    pub fn set_reading_towards(&mut self, p: Vec2) {
        let l = self.to_local(p);
        let a = (-l.y).atan2(l.x);
        self.reading = if a < 0.0 { if a < -FRAC_PI_2 { PI } else { 0.0 } } else { a };
    }
}

/// Inset a triangle by distance d (homothety about the incentre).
fn inset_triangle(t: &[Vec2], d: f32) -> Vec<Vec2> {
    if t.len() != 3 {
        return vec![];
    }
    let (a, b, c) = (t[0], t[1], t[2]);
    let (la, lb, lc) = (b.dist(c), a.dist(c), a.dist(b));
    let p = la + lb + lc;
    let inc = (a * la + b * lb + c * lc) * (1.0 / p);
    let s = p * 0.5;
    let area = ((s * (s - la) * (s - lb) * (s - lc)).max(0.0)).sqrt();
    let r = area / s;
    if r <= d * 1.2 {
        return vec![];
    }
    let k = (r - d) / r;
    t.iter().map(|q| inc + (*q - inc) * k).collect()
}

/// Points along a circular arc for compass strokes.
pub fn arc_points(center: Vec2, radius: f32, start: f32, sweep: f32) -> Vec<Vec2> {
    let steps = ((sweep.abs() * radius) / 3.0).ceil().clamp(2.0, 2000.0) as usize;
    (0..=steps).map(|i| center + Vec2::from_angle(start + sweep * i as f32 / steps as f32) * radius).collect()
}

/// Tracks a compass rotation gesture: accumulates sweep across angle wraps.
#[derive(Debug, Clone, Copy)]
pub struct ArcTracker {
    pub start: f32,
    pub last: f32,
    pub sweep: f32,
}

impl ArcTracker {
    pub fn new(angle: f32) -> ArcTracker {
        ArcTracker { start: angle, last: angle, sweep: 0.0 }
    }
    pub fn update(&mut self, angle: f32) {
        self.sweep += normalize_angle_delta(angle - self.last);
        self.sweep = self.sweep.clamp(-TAU, TAU);
        self.last = angle;
    }
    pub fn is_full_circle(&self) -> bool {
        self.sweep.abs() >= TAU - 1e-3
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AssistState {
    pub tools: Vec<AssistTool>,
}

impl AssistState {
    pub fn toggle(&mut self, kind: AssistKind, at: Vec2) {
        if let Some(i) = self.tools.iter().position(|t| t.kind == kind) {
            self.tools.remove(i);
        } else {
            self.tools.push(AssistTool::new(kind, at));
        }
    }
    pub fn is_shown(&self, kind: AssistKind) -> bool {
        self.tools.iter().any(|t| t.kind == kind)
    }
    /// Topmost tool hit at p.
    pub fn hit(&self, p: Vec2, edge_tol: f32, for_pen: bool) -> Option<(usize, AssistHit)> {
        self.tools.iter().enumerate().rev().find_map(|(i, t)| t.hit(p, edge_tol, for_pen).map(|h| (i, h)))
    }
    pub fn bring_to_front(&mut self, i: usize) -> usize {
        if i + 1 < self.tools.len() {
            let t = self.tools.remove(i);
            self.tools.push(t);
        }
        self.tools.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_angles() {
        let t = AssistTool::new(AssistKind::Triangle30, v2(500.0, 500.0));
        let o = t.outline();
        let ang = |p: Vec2, q: Vec2, r: Vec2| ((q - p).normalized().dot((r - p).normalized())).acos().to_degrees();
        assert!((ang(o[0], o[1], o[2]) - 90.0).abs() < 0.01);
        assert!((ang(o[1], o[0], o[2]) - 30.0).abs() < 0.01);
        assert!((ang(o[2], o[0], o[1]) - 60.0).abs() < 0.01);
        let t = AssistTool::new(AssistKind::Triangle45, v2(0.0, 0.0));
        let o = t.outline();
        assert!((ang(o[1], o[0], o[2]) - 45.0).abs() < 0.01);
    }

    #[test]
    fn ruler_ticks_are_cm() {
        let t = AssistTool { kind: AssistKind::Ruler, pos: v2(0.0, 0.0), angle: 0.0, scale: 1.0, radius: 0.0, reading: 0.0 };
        let ticks = t.ticks();
        let majors: Vec<&Tick> = ticks.iter().filter(|t| t.major).collect();
        assert_eq!(majors.len(), 21);
        assert!((majors[1].a.x - PX_PER_CM).abs() < 1e-3);
        assert_eq!(majors[3].label.as_ref().unwrap().0, "3");
    }

    #[test]
    fn hole_is_pass_through() {
        let t = AssistTool::new(AssistKind::Triangle45, v2(400.0, 400.0));
        let hole = t.hole();
        assert_eq!(hole.len(), 3);
        let c = hole.iter().fold(Vec2::ZERO, |a, p| a + *p) * (1.0 / 3.0);
        assert!(!t.body_contains(c));
        assert!(t.hit(c, 10.0, false).is_none());
        // Point just inside an edge is body.
        let o = t.outline();
        let mid = o[0].lerp(o[1], 0.5);
        let inward = (c - mid).normalized();
        assert!(t.body_contains(mid + inward * 10.0));
    }

    #[test]
    fn ruling_snaps_and_offsets() {
        let t = AssistTool { kind: AssistKind::Ruler, pos: v2(100.0, 100.0), angle: 0.0, scale: 1.0, radius: 0.0, reading: 0.0 };
        let (i, _) = t.nearest_edge(v2(200.0, 95.0), 12.0).unwrap();
        let (a, b) = t.rule(i, v2(200.0, 95.0), v2(300.0, 80.0), 2.0).unwrap();
        assert!((a.y - 98.0).abs() < 1e-3 && (b.y - 98.0).abs() < 1e-3);
        assert!((a.x - 200.0).abs() < 1e-3 && (b.x - 300.0).abs() < 1e-3);
    }

    #[test]
    fn rotation_keeps_shape() {
        let mut t = AssistTool::new(AssistKind::Triangle45, v2(300.0, 300.0));
        let before = t.outline();
        let p = t.pivot();
        t.rotate_about(p, 1.0);
        let after = t.outline();
        assert!((before[0].dist(before[1]) - after[0].dist(after[1])).abs() < 1e-3);
        assert!(t.pivot().dist(p) < 1e-3);
    }

    #[test]
    fn compass_geometry() {
        let mut c = AssistTool::new(AssistKind::Compass, v2(300.0, 300.0));
        c.drag_pencil_to(c.pos + v2(0.0, 200.0));
        assert!((c.radius - 200.0).abs() < 1e-3);
        assert!((c.compass_pencil().dist(c.pos + v2(0.0, 200.0))) < 1e-3);
        let l = c.compass_leg_len();
        assert!((c.compass_hinge().dist(c.compass_needle()) - l).abs() < 1e-2);
        assert!((c.compass_hinge().dist(c.compass_pencil()) - l).abs() < 1e-2);
        assert_eq!(c.hit(c.compass_pencil(), 10.0, true), Some(AssistHit::CompassPencil));
        assert_eq!(c.hit(c.compass_handle(), 10.0, true), Some(AssistHit::CompassHandle));
    }

    #[test]
    fn arc_tracker_full_circle() {
        let mut a = ArcTracker::new(0.0);
        for i in 1..=40 {
            a.update(i as f32 * 0.2);
        }
        assert!(a.is_full_circle());
        let pts = arc_points(Vec2::ZERO, 100.0, 0.0, PI);
        assert!(pts.last().unwrap().dist(v2(-100.0, 0.0)) < 1e-3);
    }

    #[test]
    fn protractor_reading() {
        let mut p = AssistTool::new(AssistKind::Protractor, v2(400.0, 400.0));
        p.set_reading_towards(p.pos + v2(0.0, -100.0));
        assert!((p.reading_deg() - 90.0).abs() < 0.01);
        p.set_reading_towards(p.pos + v2(100.0, 5.0));
        assert_eq!(p.reading_deg(), 0.0);
    }
}
