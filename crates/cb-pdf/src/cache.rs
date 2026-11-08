//! Tile pyramid planning and caching (pure logic, no PDFium needed).
//!
//! * Every page gets a cheap low-resolution preview first, so fast scrolling
//!   always shows page content instead of blank boxes.
//! * Visible regions are rendered as 512 px tiles at the exact display
//!   resolution (blitted 1:1, no resampling cost while scrolling).
//! * While a level is missing, the best cached level (or the preview) is
//!   scaled into place.
//! * A byte budget with LRU eviction bounds memory on low-end machines.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tiny_skia::Pixmap;

pub const TILE: u32 = 512;
/// Preview width in pixels.
pub const PREVIEW_W: f32 = 220.0;
pub const PREVIEW_LEVEL: i32 = i32::MIN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub page: u32,
    pub level: i32,
    pub tx: u32,
    pub ty: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileReq {
    pub key: TileKey,
    /// Device pixels per PDF point for this render.
    pub scale: f32,
    /// Lower is more urgent.
    pub priority: f32,
}

/// Resolution level for a device-pixels-per-point scale: the exact scale in
/// 1/1000 steps, so settled views blit 1:1 without resampling. During zoom
/// gestures the UI keeps showing cached levels scaled.
pub fn level_for(px_per_pt: f32) -> i32 {
    (px_per_pt.clamp(0.02, 40.0) * 1000.0).round() as i32
}

pub fn level_scale(level: i32) -> f32 {
    level as f32 / 1000.0
}

pub fn preview_scale(page_w_pt: f32) -> f32 {
    PREVIEW_W / page_w_pt.max(1.0)
}

/// Visible part of one page, in PDF points relative to the page origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisiblePage {
    pub page: u32,
    pub size_pt: (f32, f32),
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Distance (in screens) from the viewport centre; 0 for visible pages.
    pub distance: f32,
}

/// A pixmap to draw: destination rect in page points.
pub struct DrawTile {
    pub pixmap: Arc<Pixmap>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

struct Entry {
    pix: Arc<Pixmap>,
    used: u64,
}

pub struct TileCache {
    map: HashMap<TileKey, Entry>,
    bytes: usize,
    pub budget: usize,
    clock: u64,
    /// Bumped when any tile arrives (renderers use it to invalidate).
    pub version: u64,
}

impl TileCache {
    pub fn new(budget_bytes: usize) -> TileCache {
        TileCache { map: HashMap::new(), bytes: 0, budget: budget_bytes.max(16 << 20), clock: 0, version: 0 }
    }

    pub fn clear(&mut self) {
        self.map.clear();
        self.bytes = 0;
        self.version += 1;
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn contains(&self, k: &TileKey) -> bool {
        self.map.contains_key(k)
    }

    pub fn insert(&mut self, key: TileKey, pix: Pixmap) {
        self.clock += 1;
        let size = pix.data().len();
        if let Some(old) = self.map.insert(key, Entry { pix: Arc::new(pix), used: self.clock }) {
            self.bytes -= old.pix.data().len();
        }
        self.bytes += size;
        self.version += 1;
        self.evict();
    }

    fn evict(&mut self) {
        if self.bytes <= self.budget {
            return;
        }
        let mut v: Vec<(bool, u64, TileKey)> = self.map.iter().map(|(k, e)| (k.level == PREVIEW_LEVEL, e.used, *k)).collect();
        // Evict full-resolution tiles before previews, oldest first.
        v.sort_by_key(|(p, u, _)| (*p, *u));
        for (_, _, k) in v {
            if self.bytes <= self.budget * 9 / 10 {
                break;
            }
            if let Some(e) = self.map.remove(&k) {
                self.bytes -= e.pix.data().len();
            }
        }
    }

    fn touch(&mut self, k: &TileKey) -> Option<Arc<Pixmap>> {
        self.clock += 1;
        let c = self.clock;
        self.map.get_mut(k).map(|e| {
            e.used = c;
            e.pix.clone()
        })
    }

    fn tiles_for(vp: &VisiblePage, level: i32) -> impl Iterator<Item = (u32, u32)> {
        let s = level_scale(level);
        let full_w = (vp.size_pt.0 * s).ceil();
        let full_h = (vp.size_pt.1 * s).ceil();
        let t = TILE as f32;
        let tx0 = ((vp.x0 * s) / t).floor().max(0.0) as u32;
        let ty0 = ((vp.y0 * s) / t).floor().max(0.0) as u32;
        let tx1 = (((vp.x1 * s).min(full_w - 1.0)) / t).floor().max(0.0) as u32;
        let ty1 = (((vp.y1 * s).min(full_h - 1.0)) / t).floor().max(0.0) as u32;
        (ty0..=ty1).flat_map(move |ty| (tx0..=tx1).map(move |tx| (tx, ty)))
    }

    /// Plan render jobs. `visible` are on-screen pages (distance 0) plus
    /// prefetch candidates (distance > 0). `all_pages` get previews.
    pub fn plan(&self, visible: &[VisiblePage], px_per_pt: f32, all_pages: &[(f32, f32)], max_previews: usize, settled: bool) -> Vec<TileReq> {
        let level = level_for(px_per_pt);
        let scale = level_scale(level);
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for vp in visible {
            let pk = TileKey { page: vp.page, level: PREVIEW_LEVEL, tx: 0, ty: 0 };
            if !self.contains(&pk) && seen.insert(pk) {
                out.push(TileReq { key: pk, scale: preview_scale(vp.size_pt.0), priority: vp.distance * 10.0 });
            }
        }
        for vp in visible.iter().filter(|_| settled) {
            let cx = (vp.x0 + vp.x1) * 0.5 * scale;
            let cy = (vp.y0 + vp.y1) * 0.5 * scale;
            for (tx, ty) in Self::tiles_for(vp, level) {
                let key = TileKey { page: vp.page, level, tx, ty };
                if self.contains(&key) || !seen.insert(key) {
                    continue;
                }
                let dx = (tx as f32 + 0.5) * TILE as f32 - cx;
                let dy = (ty as f32 + 0.5) * TILE as f32 - cy;
                let d = (dx * dx + dy * dy).sqrt() / 4096.0;
                out.push(TileReq { key, scale, priority: 1.0 + vp.distance * 10.0 + d });
            }
        }
        // Background previews for the rest of the document, nearest first.
        if let Some(first) = visible.iter().find(|v| v.distance == 0.0) {
            let mut order: Vec<u32> = (0..all_pages.len() as u32).collect();
            order.sort_by_key(|p| (*p as i64 - first.page as i64).abs());
            for p in order.into_iter().take(max_previews) {
                let pk = TileKey { page: p, level: PREVIEW_LEVEL, tx: 0, ty: 0 };
                if !self.contains(&pk) && seen.insert(pk) {
                    let d = (p as i64 - first.page as i64).abs() as f32;
                    out.push(TileReq { key: pk, scale: preview_scale(all_pages[p as usize].0), priority: 50.0 + d });
                }
            }
        }
        out
    }

    /// Best available pixmaps covering a visible page region, back to front.
    pub fn draw_list(&mut self, vp: &VisiblePage, px_per_pt: f32) -> Vec<DrawTile> {
        let mut out = Vec::new();
        let pk = TileKey { page: vp.page, level: PREVIEW_LEVEL, tx: 0, ty: 0 };
        let target = level_for(px_per_pt);
        let have_all_target = Self::tiles_for(vp, target).all(|(tx, ty)| self.contains(&TileKey { page: vp.page, level: target, tx, ty }));
        if !have_all_target {
            if let Some(p) = self.touch(&pk) {
                out.push(DrawTile { pixmap: p, x: 0.0, y: 0.0, w: vp.size_pt.0, h: vp.size_pt.1 });
            }
            // Other cached levels for this page, farthest from target first.
            let mut levels: Vec<i32> =
                self.map.keys().filter(|k| k.page == vp.page && k.level != PREVIEW_LEVEL && k.level != target).map(|k| k.level).collect::<HashSet<_>>().into_iter().collect();
            levels.sort_by_key(|l| std::cmp::Reverse((l - target).abs()));
            for l in levels {
                self.push_level(vp, l, &mut out);
            }
        }
        self.push_level(vp, target, &mut out);
        out
    }

    fn push_level(&mut self, vp: &VisiblePage, level: i32, out: &mut Vec<DrawTile>) {
        let s = level_scale(level);
        let t = TILE as f32;
        let tiles: Vec<(u32, u32)> = Self::tiles_for(vp, level).collect();
        for (tx, ty) in tiles {
            let key = TileKey { page: vp.page, level, tx, ty };
            if let Some(p) = self.touch(&key) {
                let (w, h) = (p.width() as f32 / s, p.height() as f32 / s);
                out.push(DrawTile { pixmap: p, x: tx as f32 * t / s, y: ty as f32 * t / s, w, h });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp(page: u32) -> VisiblePage {
        VisiblePage { page, size_pt: (600.0, 800.0), x0: 0.0, y0: 0.0, x1: 600.0, y1: 400.0, distance: 0.0 }
    }

    #[test]
    fn levels_round_up() {
        for s in [0.3f32, 1.0, 1.333, 2.0, 3.7] {
            assert!((level_scale(level_for(s)) - s).abs() < 0.001);
        }
    }

    #[test]
    fn plan_previews_first_then_tiles() {
        let c = TileCache::new(64 << 20);
        let pages = vec![(600.0, 800.0); 10];
        let jobs = c.plan(&[vp(3)], 2.0, &pages, 10, true);
        let first = jobs.iter().min_by(|a, b| a.priority.partial_cmp(&b.priority).unwrap()).unwrap();
        assert_eq!(first.key.level, PREVIEW_LEVEL);
        assert_eq!(first.key.page, 3);
        // 600pt*2.0=1200px wide -> 3 tiles across, 400pt*2=800px -> 2 rows.
        let full: Vec<_> = jobs.iter().filter(|j| j.key.level != PREVIEW_LEVEL).collect();
        assert_eq!(full.len(), 6);
        assert_eq!(jobs.iter().filter(|j| j.key.level == PREVIEW_LEVEL).count(), 10);
    }

    #[test]
    fn cached_tiles_not_replanned_and_fallback_draws_preview() {
        let mut c = TileCache::new(64 << 20);
        let pages = vec![(600.0, 800.0); 2];
        c.insert(TileKey { page: 0, level: PREVIEW_LEVEL, tx: 0, ty: 0 }, Pixmap::new(220, 293).unwrap());
        let jobs = c.plan(&[vp(0)], 1.0, &pages, 0, true);
        assert!(jobs.iter().all(|j| j.key.level != PREVIEW_LEVEL));
        let dl = c.draw_list(&vp(0), 1.0);
        assert_eq!(dl.len(), 1);
        assert_eq!(dl[0].w, 600.0);
    }

    #[test]
    fn budget_evicts_full_tiles_before_previews() {
        let mut c = TileCache::new(16 << 20);
        c.insert(TileKey { page: 0, level: PREVIEW_LEVEL, tx: 0, ty: 0 }, Pixmap::new(220, 300).unwrap());
        for i in 0..40 {
            c.insert(TileKey { page: 0, level: 1000, tx: i, ty: 0 }, Pixmap::new(512, 512).unwrap());
        }
        assert!(c.bytes() <= 16 << 20);
        assert!(c.contains(&TileKey { page: 0, level: PREVIEW_LEVEL, tx: 0, ty: 0 }));
        assert!(c.contains(&TileKey { page: 0, level: 1000, tx: 39, ty: 0 }));
        assert!(!c.contains(&TileKey { page: 0, level: 1000, tx: 0, ty: 0 }));
    }
}
