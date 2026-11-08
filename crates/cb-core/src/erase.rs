//! Eraser: splits strokes where a circular eraser passes over them.

use crate::doc::{Document, InkPoint, Item, Stroke};
use crate::geom::{Rect, Vec2};
use crate::history::Command;

/// Interval of the segment parameter t∈[0,1] lying inside the circle.
fn segment_circle_interval(a: Vec2, b: Vec2, c: Vec2, r: f32) -> Option<(f32, f32)> {
    let d = b - a;
    let f = a - c;
    let qa = d.len_sq();
    if qa < 1e-12 {
        return if f.len_sq() <= r * r { Some((0.0, 1.0)) } else { None };
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.len_sq() - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc <= 0.0 {
        return None;
    }
    let s = disc.sqrt();
    let t0 = (-qb - s) / (2.0 * qa);
    let t1 = (-qb + s) / (2.0 * qa);
    let (t0, t1) = (t0.max(0.0), t1.min(1.0));
    if t0 >= t1 {
        None
    } else {
        Some((t0, t1))
    }
}

fn lerp_pt(a: &InkPoint, b: &InkPoint, t: f32) -> InkPoint {
    InkPoint { x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t, pressure: a.pressure + (b.pressure - a.pressure) * t, dt_ms: 0 }
}

/// Split a polyline by a circle; returns surviving pieces (each ≥ 2 points),
/// or `None` if the circle does not touch the stroke.
pub fn split_points(pts: &[InkPoint], c: Vec2, r: f32) -> Option<Vec<Vec<InkPoint>>> {
    if pts.is_empty() {
        return None;
    }
    if pts.len() == 1 {
        return if pts[0].pos().dist(c) <= r { Some(vec![]) } else { None };
    }
    let mut touched = false;
    let mut pieces: Vec<Vec<InkPoint>> = Vec::new();
    let mut cur: Vec<InkPoint> = Vec::new();
    let inside0 = pts[0].pos().dist(c) <= r;
    if !inside0 {
        cur.push(pts[0]);
    }
    for i in 0..pts.len() - 1 {
        let (a, b) = (&pts[i], &pts[i + 1]);
        match segment_circle_interval(a.pos(), b.pos(), c, r) {
            None => {
                cur.push(*b);
            }
            Some((t0, t1)) => {
                touched = true;
                if t0 > 0.0 {
                    cur.push(lerp_pt(a, b, t0));
                }
                if cur.len() >= 2 {
                    pieces.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                if t1 < 1.0 {
                    cur.push(lerp_pt(a, b, t1));
                    cur.push(*b);
                }
            }
        }
    }
    if cur.len() >= 2 {
        pieces.push(cur);
    }
    if touched {
        Some(pieces)
    } else {
        None
    }
}

/// Erase with a circle at `c` (radius `r`) on the given page.
/// Returns true if anything changed. New fragments get fresh uids.
pub fn erase_at(doc: &mut Document, page: usize, c: Vec2, r: f32) -> bool {
    let probe = Rect::new(c.x - r, c.y - r, c.x + r, c.y + r);
    let mut changed = false;
    let mut out: Vec<Stroke> = Vec::with_capacity(doc.pages[page].strokes.len());
    let strokes = std::mem::take(&mut doc.pages[page].strokes);
    for s in strokes {
        if !s.bounds().intersects(&probe) {
            out.push(s);
            continue;
        }
        // Shapes are tessellated so they can be partially erased too.
        let pts: Vec<InkPoint> = match s.shape {
            None => s.points.clone(),
            Some(_) => {
                let mut v: Vec<InkPoint> = Vec::new();
                let o = s.outline();
                for w in o.windows(2) {
                    let n = ((w[1] - w[0]).len() / 4.0).ceil().max(1.0) as usize;
                    for k in 0..n {
                        let p = w[0].lerp(w[1], k as f32 / n as f32);
                        v.push(InkPoint::new(p.x, p.y));
                    }
                }
                if let Some(l) = o.last() {
                    v.push(InkPoint::new(l.x, l.y));
                }
                v
            }
        };
        let eff_r = r + s.width * 0.35;
        match split_points(&pts, c, eff_r) {
            None => out.push(s),
            Some(pieces) => {
                changed = true;
                for p in pieces {
                    let mut ns = s.clone();
                    ns.shape = None;
                    ns.points = p;
                    ns.uid = doc.next_uid;
                    doc.next_uid += 1;
                    out.push(ns);
                }
            }
        }
    }
    doc.pages[page].strokes = out;
    if changed {
        doc.touch();
    }
    changed
}

/// Erase along a path from `from` to `to`, sub-stepping so fast moves don't skip.
pub fn erase_along(doc: &mut Document, page: usize, from: Vec2, to: Vec2, r: f32) -> bool {
    let d = from.dist(to);
    let step = (r * 0.45).max(1.5);
    let n = (d / step).ceil().max(1.0) as usize;
    let mut changed = false;
    for i in 1..=n {
        let p = from.lerp(to, i as f32 / n as f32);
        changed |= erase_at(doc, page, p, r);
    }
    changed
}

/// Remove whole strokes touched by the circle (stroke eraser mode).
pub fn erase_whole_at(doc: &mut Document, page: usize, c: Vec2, r: f32) -> bool {
    let before = doc.pages[page].strokes.len();
    doc.pages[page].strokes.retain(|s| {
        let o = s.outline();
        let rr = r + s.width * 0.5;
        if o.len() == 1 {
            return o[0].dist(c) > rr;
        }
        !o.windows(2).any(|w| crate::geom::dist_to_segment(c, w[0], w[1]).0 <= rr)
    });
    let changed = doc.pages[page].strokes.len() != before;
    if changed {
        doc.touch();
    }
    changed
}

/// Build an undoable command from the strokes before/after an erase gesture.
pub fn diff_strokes(page: usize, before: &[Stroke], after: &[Stroke]) -> Option<Command> {
    use std::collections::HashSet;
    let a: HashSet<u64> = after.iter().map(|s| s.uid).collect();
    let b: HashSet<u64> = before.iter().map(|s| s.uid).collect();
    let removed: Vec<(usize, Item)> =
        before.iter().enumerate().filter(|(_, s)| !a.contains(&s.uid)).map(|(i, s)| (i, Item::Stroke(s.clone()))).collect();
    let added: Vec<(usize, Item)> =
        after.iter().enumerate().filter(|(_, s)| !b.contains(&s.uid)).map(|(i, s)| (i, Item::Stroke(s.clone()))).collect();
    if removed.is_empty() && added.is_empty() {
        None
    } else {
        Some(Command::Replace { page, removed, added })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Color;
    use crate::geom::v2;
    use crate::history::History;

    fn line(doc: &mut Document) {
        let mut s = Stroke::new(doc.alloc_uid(), Color::rgb(0, 0, 0), 2.0);
        for i in 0..=100 {
            s.points.push(InkPoint::new(i as f32, 0.0));
        }
        doc.pages[0].strokes.push(s);
    }

    #[test]
    fn splits_in_middle() {
        let mut d = Document::new();
        line(&mut d);
        assert!(erase_at(&mut d, 0, v2(50.0, 0.0), 5.0));
        assert_eq!(d.pages[0].strokes.len(), 2);
        let l = &d.pages[0].strokes[0].points;
        assert!(l.last().unwrap().x < 46.0);
    }

    #[test]
    fn misses() {
        let mut d = Document::new();
        line(&mut d);
        assert!(!erase_at(&mut d, 0, v2(50.0, 30.0), 5.0));
    }

    #[test]
    fn undo_restores() {
        let mut d = Document::new();
        let mut h = History::new();
        line(&mut d);
        let before = d.pages[0].strokes.clone();
        erase_along(&mut d, 0, v2(20.0, -10.0), v2(20.0, 10.0), 4.0);
        erase_along(&mut d, 0, v2(70.0, -10.0), v2(70.0, 10.0), 4.0);
        assert_eq!(d.pages[0].strokes.len(), 3);
        let cmd = diff_strokes(0, &before, &d.pages[0].strokes).unwrap();
        h.record(cmd);
        h.undo(&mut d);
        assert_eq!(d.pages[0].strokes, before);
        h.redo(&mut d);
        assert_eq!(d.pages[0].strokes.len(), 3);
    }

    #[test]
    fn whole_mode() {
        let mut d = Document::new();
        line(&mut d);
        assert!(erase_whole_at(&mut d, 0, v2(10.0, 1.0), 2.0));
        assert!(d.pages[0].strokes.is_empty());
    }
}
