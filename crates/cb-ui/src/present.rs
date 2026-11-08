//! Presentation mode for PowerPoint decks: one slide fills the screen, the
//! click steps of build animations play as fades (sliding fades for
//! fly-in/wipe, a small pulse for emphasis), slides cross-fade, and ink
//! written during the show stays on the slide's page.

use cb_office::model::{Effect, Page};
use cb_office::{Filter, Renderer};
use cb_render::tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub const STEP_MS: u64 = 380;
pub const SLIDE_MS: u64 = 320;

#[derive(Debug, Clone)]
enum Anim {
    /// Effects of one click step.
    Step(Vec<(u32, Effect)>),
    /// Cross-fade from the previous slide's last frame.
    Enter(Pixmap),
}

struct Layers {
    key: (usize, usize, u32, u32, bool),
    base: Pixmap,
    moving: Vec<(Effect, Pixmap, (f32, f32, f32, f32))>,
}

pub struct Show {
    pub slide: usize,
    /// Click steps completed on this slide.
    pub step: usize,
    anim: Option<(Instant, Anim)>,
    layers: Option<Layers>,
    /// Composited frame for the board renderer, with a revision counter.
    pub frame: Option<Pixmap>,
    pub rev: u64,
    renderer: Renderer,
    /// View and tool to restore on exit.
    pub saved: (f32, cb_core::geom::Vec2, usize),
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// Bounding box (in points) of the items belonging to `id`.
fn shape_bounds(page: &Page, id: u32) -> (f32, f32, f32, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for it in page.items.iter().filter(|i| i.ids.contains(&id)) {
        for (x, y) in [(0.0, 0.0), (it.w, 0.0), (0.0, it.h), (it.w, it.h)] {
            let mut p = cb_render::tiny_skia::Point::from_xy(x, y);
            it.xf.map_point(&mut p);
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
    }
    if x0 > x1 {
        (0.0, 0.0, page.width, page.height)
    } else {
        (x0, y0, x1, y1)
    }
}

impl Show {
    pub fn new(slide: usize, saved: (f32, cb_core::geom::Vec2, usize)) -> Show {
        Show { slide, step: 0, anim: None, layers: None, frame: None, rev: 1, renderer: Renderer::new(), saved }
    }

    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }

    /// Advance one click (true) or go to the next slide (false = caller moves on).
    pub fn next(&mut self, page: &Page, now: Instant) -> bool {
        self.finish_anim();
        if self.step < page.steps.len() {
            let effects = page.steps[self.step].effects.clone();
            self.step += 1;
            self.anim = Some((now, Anim::Step(effects)));
            self.layers = None;
            true
        } else {
            false
        }
    }

    /// Go back one click without animation; false at the first step.
    pub fn prev(&mut self) -> bool {
        self.finish_anim();
        if self.step > 0 {
            self.step -= 1;
            self.layers = None;
            true
        } else {
            false
        }
    }

    /// Switch to another slide (fading from the current frame).
    pub fn goto(&mut self, slide: usize, step: usize, now: Instant, fade: bool) {
        let from = self.frame.take();
        self.slide = slide;
        self.step = step;
        self.layers = None;
        self.anim = match (fade, from) {
            (true, Some(f)) => Some((now, Anim::Enter(f))),
            _ => None,
        };
    }

    fn finish_anim(&mut self) {
        if self.anim.take().is_some() {
            self.layers = None;
        }
    }

    /// Time of the next frame needed while animating.
    pub fn next_frame(&self, now: Instant) -> Option<Instant> {
        self.anim.as_ref().map(|_| now + Duration::from_millis(16))
    }

    /// Update `frame` for a slide drawn at w×h device pixels.
    pub fn compose(&mut self, fs: &mut cb_render::cosmic_text::FontSystem, page: &Page, w: u32, h: u32, now: Instant) {
        if w == 0 || h == 0 {
            return;
        }
        let (dur, t) = match &self.anim {
            Some((t0, a)) => {
                let d = match a {
                    Anim::Step(_) => STEP_MS,
                    Anim::Enter(_) => SLIDE_MS,
                };
                (d, now.duration_since(*t0).as_millis() as f32 / d as f32)
            }
            None => (0, 1.0),
        };
        if dur > 0 && t >= 1.0 {
            self.anim = None;
            self.layers = None;
        }
        let animating_step = matches!(self.anim, Some((_, Anim::Step(_))));
        let key = (self.slide, self.step, w, h, animating_step);
        if self.anim.is_none() && self.frame.is_some() && self.layers.as_ref().map(|l| l.key) == Some(key) {
            return; // Static and up to date.
        }
        if self.layers.as_ref().map(|l| l.key) != Some(key) {
            let scale = w as f32 / page.width;
            let base_t = Transform::from_scale(scale, scale);
            let mut hidden: HashSet<u32> = page.hidden_after(self.step);
            let mut moving = Vec::new();
            if let Some((_, Anim::Step(effects))) = &self.anim {
                // Animating shapes are drawn on their own layers.
                for (id, e) in effects {
                    hidden.insert(*id);
                    if let Some(mut pm) = Pixmap::new(w, h) {
                        let only = [*id];
                        self.renderer.draw_page(fs, page, &mut pm.as_mut(), base_t, &Filter { hidden: None, only: Some(&only), background: false });
                        moving.push((*e, pm, shape_bounds(page, *id)));
                    }
                }
            }
            let Some(mut base) = Pixmap::new(w, h) else { return };
            base.fill(cb_render::tiny_skia::Color::WHITE);
            self.renderer.draw_page(fs, page, &mut base.as_mut(), base_t, &Filter { hidden: Some(&hidden), only: None, background: true });
            self.layers = Some(Layers { key, base, moving });
        }
        let Some(layers) = &self.layers else { return };
        let mut out = layers.base.clone();
        let p = ease(t);
        let scale = w as f32 / page.width;
        for (e, pm, (x0, y0, x1, y1)) in &layers.moving {
            let (alpha, tf) = match e {
                Effect::In { from } => {
                    let d = (1.0 - p) * 0.12;
                    (p, Transform::from_translate(from.0 * d * w as f32, from.1 * d * h as f32))
                }
                Effect::Out => (1.0 - p, Transform::identity()),
                Effect::Pulse => {
                    let k = 1.0 + 0.08 * (p * std::f32::consts::PI).sin();
                    let (cx, cy) = ((x0 + x1) * 0.5 * scale, (y0 + y1) * 0.5 * scale);
                    (1.0, Transform::from_translate(cx, cy).pre_scale(k, k).pre_translate(-cx, -cy))
                }
            };
            let paint = PixmapPaint { opacity: alpha.clamp(0.0, 1.0), quality: FilterQuality::Bilinear, ..Default::default() };
            out.draw_pixmap(0, 0, pm.as_ref(), &paint, tf, None);
        }
        if let Some((_, Anim::Enter(prev))) = &self.anim {
            if prev.width() == w && prev.height() == h {
                let paint = PixmapPaint { opacity: (1.0 - p).clamp(0.0, 1.0), ..Default::default() };
                out.draw_pixmap(0, 0, prev.as_ref(), &paint, Transform::identity(), None);
            }
        }
        self.frame = Some(out);
        self.rev = self.rev.wrapping_add(1);
    }
}
