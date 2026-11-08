//! Minimal SVG path-data parser (M L H V C S Q T A Z, absolute & relative)
//! producing tiny-skia paths. Used for the vector icon set.

use tiny_skia::{Path, PathBuilder};

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Lexer<'a> {
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] == b' ' || self.s[self.i] == b',' || self.s[self.i] == b'\n' || self.s[self.i] == b'\t') {
            self.i += 1;
        }
    }
    fn peek_cmd(&mut self) -> Option<u8> {
        self.skip();
        let c = *self.s.get(self.i)?;
        if c.is_ascii_alphabetic() {
            Some(c)
        } else {
            None
        }
    }
    fn num(&mut self) -> Option<f32> {
        self.skip();
        let start = self.i;
        let s = self.s;
        if self.i < s.len() && (s[self.i] == b'-' || s[self.i] == b'+') {
            self.i += 1;
        }
        let mut dot = false;
        while self.i < s.len() {
            let c = s[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !dot {
                dot = true;
                self.i += 1;
            } else if (c == b'e' || c == b'E') && self.i > start {
                self.i += 1;
                if self.i < s.len() && (s[self.i] == b'-' || s[self.i] == b'+') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        if self.i == start {
            return None;
        }
        std::str::from_utf8(&s[start..self.i]).ok()?.parse().ok()
    }
    fn flag(&mut self) -> Option<bool> {
        self.skip();
        let c = *self.s.get(self.i)?;
        self.i += 1;
        Some(c == b'1')
    }
    fn has_num(&mut self) -> bool {
        self.skip();
        matches!(self.s.get(self.i), Some(c) if c.is_ascii_digit() || *c == b'-' || *c == b'+' || *c == b'.')
    }
}

/// Append an SVG elliptical arc as cubic beziers.
#[allow(clippy::too_many_arguments)]
fn arc_to(pb: &mut PathBuilder, x1: f32, y1: f32, mut rx: f32, mut ry: f32, phi_deg: f32, large: bool, sweep: bool, x2: f32, y2: f32) {
    if rx.abs() < 1e-6 || ry.abs() < 1e-6 {
        pb.line_to(x2, y2);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    let phi = phi_deg.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (x1 - x2) / 2.0;
    let dy = (y1 - y2) / 2.0;
    let x1p = cp * dx + sp * dy;
    let y1p = -sp * dx + cp * dy;
    let lam = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    if lam > 1.0 {
        let s = lam.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1p / ry;
    let cyp = -co * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (x1 + x2) / 2.0;
    let cy = sp * cxp + cp * cyp + (y1 + y2) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let t1 = ang(1.0, 0.0, (x1p - cxp) / rx, (y1p - cyp) / ry);
    let mut dt = ang((x1p - cxp) / rx, (y1p - cyp) / ry, (-x1p - cxp) / rx, (-y1p - cyp) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let segs = (dt.abs() / (std::f32::consts::FRAC_PI_2)).ceil().max(1.0) as usize;
    let d = dt / segs as f32;
    let k = 4.0 / 3.0 * (d / 4.0).tan();
    let pt = |t: f32| {
        let (s, c) = t.sin_cos();
        (cx + rx * c * cp - ry * s * sp, cy + rx * c * sp + ry * s * cp)
    };
    let der = |t: f32| {
        let (s, c) = t.sin_cos();
        (-rx * s * cp - ry * c * sp, -rx * s * sp + ry * c * cp)
    };
    let mut t = t1;
    for _ in 0..segs {
        let (p0x, p0y) = pt(t);
        let (d0x, d0y) = der(t);
        let t2 = t + d;
        let (p3x, p3y) = pt(t2);
        let (d3x, d3y) = der(t2);
        pb.cubic_to(p0x + k * d0x, p0y + k * d0y, p3x - k * d3x, p3y - k * d3y, p3x, p3y);
        t = t2;
    }
}

/// Parse path data; returns None for empty/invalid input.
pub fn parse(d: &str) -> Option<Path> {
    let mut pb = PathBuilder::new();
    append(&mut pb, d);
    pb.finish()
}

pub fn append(pb: &mut PathBuilder, d: &str) {
    let mut lx = Lexer { s: d.as_bytes(), i: 0 };
    let (mut cx, mut cy) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let (mut lcx, mut lcy) = (0.0f32, 0.0f32); // last control point
    let mut last = b' ';
    let mut cmd = b' ';
    loop {
        if let Some(c) = lx.peek_cmd() {
            cmd = c;
            lx.i += 1;
        } else if !lx.has_num() {
            break;
        } else if cmd == b'M' {
            cmd = b'L';
        } else if cmd == b'm' {
            cmd = b'l';
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (cx, cy) } else { (0.0, 0.0) };
        macro_rules! n {
            () => {
                match lx.num() {
                    Some(v) => v,
                    None => return,
                }
            };
        }
        match cmd.to_ascii_uppercase() {
            b'M' => {
                cx = ox + n!();
                cy = oy + n!();
                pb.move_to(cx, cy);
                sx = cx;
                sy = cy;
            }
            b'L' => {
                cx = ox + n!();
                cy = oy + n!();
                pb.line_to(cx, cy);
            }
            b'H' => {
                cx = (if rel { cx } else { 0.0 }) + n!();
                pb.line_to(cx, cy);
            }
            b'V' => {
                cy = (if rel { cy } else { 0.0 }) + n!();
                pb.line_to(cx, cy);
            }
            b'C' => {
                let (x1, y1, x2, y2, x, y) = (ox + n!(), oy + n!(), ox + n!(), oy + n!(), ox + n!(), oy + n!());
                pb.cubic_to(x1, y1, x2, y2, x, y);
                lcx = x2;
                lcy = y2;
                cx = x;
                cy = y;
            }
            b'S' => {
                let (x1, y1) = if matches!(last.to_ascii_uppercase(), b'C' | b'S') { (2.0 * cx - lcx, 2.0 * cy - lcy) } else { (cx, cy) };
                let (x2, y2, x, y) = (ox + n!(), oy + n!(), ox + n!(), oy + n!());
                pb.cubic_to(x1, y1, x2, y2, x, y);
                lcx = x2;
                lcy = y2;
                cx = x;
                cy = y;
            }
            b'Q' => {
                let (x1, y1, x, y) = (ox + n!(), oy + n!(), ox + n!(), oy + n!());
                pb.quad_to(x1, y1, x, y);
                lcx = x1;
                lcy = y1;
                cx = x;
                cy = y;
            }
            b'T' => {
                let (x1, y1) = if matches!(last.to_ascii_uppercase(), b'Q' | b'T') { (2.0 * cx - lcx, 2.0 * cy - lcy) } else { (cx, cy) };
                let (x, y) = (ox + n!(), oy + n!());
                pb.quad_to(x1, y1, x, y);
                lcx = x1;
                lcy = y1;
                cx = x;
                cy = y;
            }
            b'A' => {
                let (rx, ry, rot) = (n!(), n!(), n!());
                let large = lx.flag().unwrap_or(false);
                let sweep = lx.flag().unwrap_or(false);
                let (x, y) = (ox + n!(), oy + n!());
                arc_to(pb, cx, cy, rx, ry, rot, large, sweep, x, y);
                cx = x;
                cy = y;
            }
            b'Z' => {
                pb.close();
                cx = sx;
                cy = sy;
            }
            _ => return,
        }
        last = cmd;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_basic_and_arcs() {
        let p = parse("M3 12h18M12 3v18").unwrap();
        assert_eq!(p.bounds().width(), 18.0);
        let c = parse("M2 12a10 10 0 1 0 20 0a10 10 0 1 0 -20 0").unwrap();
        assert!((c.bounds().width() - 20.0).abs() < 0.1);
        assert!((c.bounds().height() - 20.0).abs() < 0.1);
        assert!(parse("").is_none());
    }
}
