//! Display model shared by the PowerPoint and Word importers. Coordinates
//! are in points; every item carries a transform from its local box
//! (0..w, 0..h) to the page.

use std::sync::Arc;
use tiny_skia::{Pixmap, Transform};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Rgba = Rgba { r: 255, g: 255, b: 255, a: 255 };
    pub fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba { r, g, b, a: 255 }
    }
    pub fn with_alpha(self, a: u8) -> Rgba {
        Rgba { a, ..self }
    }
    pub fn to_skia(self) -> tiny_skia::Color {
        tiny_skia::Color::from_rgba8(self.r, self.g, self.b, self.a)
    }
    pub fn to_cosmic(self) -> cosmic_text::Color {
        cosmic_text::Color::rgba(self.r, self.g, self.b, self.a)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Rgba),
    /// Linear gradient in the local box: angle in degrees (0 = left→right,
    /// clockwise), stops (position 0..1, colour).
    Linear { angle: f32, stops: Vec<(f32, Rgba)> },
    /// Radial gradient from the box centre.
    Radial { stops: Vec<(f32, Rgba)> },
    Image { img: Arc<Pixmap> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dash {
    Solid,
    Dash,
    Dot,
    DashDot,
    LongDash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    None,
    Triangle,
    Open,
    Oval,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub width: f32,
    pub color: Rgba,
    pub dash: Dash,
    pub head: Arrow,
    pub tail: Arrow,
}

/// One sub-path of a shape's geometry with its own fill/stroke switches.
#[derive(Debug, Clone)]
pub struct SubPath {
    pub path: tiny_skia::Path,
    pub fill: bool,
    pub stroke: bool,
    /// Fill shading modifier ("darken", "lighten" … in presets), as a factor.
    pub shade: f32,
}

/// Laid-out text: cosmic-text buffers in point units placed in the local box.
#[derive(Debug, Clone)]
pub struct TextBlock {
    pub buf: Arc<cosmic_text::Buffer>,
    /// Only layout runs (lines) in this range are drawn (Word page splits).
    pub lines: (usize, usize),
    /// Offset of the buffer origin in the item's local box.
    pub dx: f32,
    pub dy: f32,
    /// Per-span decorations, indexed by `Attrs::metadata`.
    pub spans: Arc<Vec<SpanDeco>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SpanDeco {
    pub underline: bool,
    pub strike: bool,
    pub highlight: Option<Rgba>,
}

#[derive(Debug, Clone)]
pub enum Kind {
    Shape { geom: Vec<SubPath>, fill: Option<Paint>, line: Option<Line> },
    Image { img: Arc<Pixmap>, crop: [f32; 4], alpha: f32 },
    Text(Vec<TextBlock>),
    /// Content we cannot draw (charts without a cached image, media…).
    Placeholder(String),
}

#[derive(Debug, Clone)]
pub struct Item {
    /// Shape ids this item belongs to (own id plus enclosing groups), for animations.
    pub ids: Vec<u32>,
    pub xf: Transform,
    pub w: f32,
    pub h: f32,
    pub kind: Kind,
}

/// Simplified animation effect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// Entrance; `from` is a unit offset direction for fly/wipe-like entrances.
    In { from: (f32, f32) },
    Out,
    Pulse,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// (shape id, effect) pairs triggered together.
    pub effects: Vec<(u32, Effect)>,
}

#[derive(Debug, Clone)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub background: Option<Paint>,
    pub items: Vec<Item>,
    /// Click steps (PowerPoint build animations).
    pub steps: Vec<Step>,
    /// Effects that run on entering the slide (before the first click).
    pub auto: Vec<(u32, Effect)>,
}

impl Page {
    pub fn new(width: f32, height: f32) -> Page {
        Page { width, height, background: Some(Paint::Solid(Rgba::WHITE)), items: Vec::new(), steps: Vec::new(), auto: Vec::new() }
    }

    /// Shapes hidden before any animation step: those whose first effect is an entrance.
    pub fn initially_hidden(&self) -> Vec<u32> {
        let mut seen = std::collections::HashSet::new();
        let mut hidden = Vec::new();
        for (id, e) in self.auto.iter().chain(self.steps.iter().flat_map(|s| s.effects.iter())) {
            if seen.insert(*id) && matches!(e, Effect::In { .. }) {
                hidden.push(*id);
            }
        }
        hidden
    }

    /// Visible state of every animated shape after `step` click steps.
    pub fn hidden_after(&self, step: usize) -> std::collections::HashSet<u32> {
        let mut h: std::collections::HashSet<u32> = self.initially_hidden().into_iter().collect();
        let apply = |h: &mut std::collections::HashSet<u32>, effs: &[(u32, Effect)]| {
            for (id, e) in effs {
                match e {
                    Effect::In { .. } => {
                        h.remove(id);
                    }
                    Effect::Out => {
                        h.insert(*id);
                    }
                    Effect::Pulse => {}
                }
            }
        };
        apply(&mut h, &self.auto);
        for s in self.steps.iter().take(step) {
            apply(&mut h, &s.effects);
        }
        h
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub pages: Vec<Page>,
}
