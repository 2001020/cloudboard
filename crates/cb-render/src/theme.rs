//! Design tokens. All sizes are logical pixels; multiply by the UI scale.

use cb_core::Color;

pub const ACCENTS: [Color; 6] = [
    Color::rgb(64, 120, 242),  // blue
    Color::rgb(138, 92, 246),  // purple
    Color::rgb(34, 166, 110),  // green
    Color::rgb(240, 138, 36),  // orange
    Color::rgb(229, 72, 77),   // red
    Color::rgb(18, 172, 196),  // cyan
];

pub const ACCENT_NAMES: [&str; 6] = ["蓝", "紫", "绿", "橙", "红", "青"];

/// Pen palette (legacy order).
pub const PEN_COLORS: [Color; 8] = [
    Color::rgb(30, 30, 30),
    Color::rgb(230, 57, 70),
    Color::rgb(247, 127, 0),
    Color::rgb(252, 191, 73),
    Color::rgb(42, 157, 90),
    Color::rgb(36, 99, 235),
    Color::rgb(124, 58, 237),
    Color::rgb(240, 240, 240),
];

pub const PEN_WIDTHS: [f32; 5] = [2.0, 4.0, 7.0, 12.0, 20.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub accent: Color,
    pub accent_strong: Color,
    pub on_accent: Color,
    pub window_bg: Color,
    pub board_bg: Color,
    pub bar_top: Color,
    pub bar_bottom: Color,
    pub bar_border: Color,
    pub surface: Color,
    pub surface_alt: Color,
    pub surface_hover: Color,
    pub border: Color,
    pub text: Color,
    pub text_dim: Color,
    pub danger: Color,
    pub shadow: Color,
    pub scrim: Color,
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color::rgba(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b), f(a.a, b.a))
}

impl Theme {
    pub fn new(dark: bool, accent_idx: usize) -> Theme {
        let accent = ACCENTS[accent_idx.min(5)];
        let accent_strong = mix(accent, Color::rgb(0, 0, 0), 0.22);
        if dark {
            let base = Color::rgb(28, 32, 44);
            Theme {
                dark,
                accent,
                accent_strong,
                on_accent: Color::rgb(255, 255, 255),
                window_bg: Color::rgb(22, 25, 34),
                board_bg: Color::rgb(255, 255, 255),
                bar_top: mix(base, accent, 0.10).with_alpha(236),
                bar_bottom: mix(Color::rgb(20, 23, 32), accent, 0.06).with_alpha(244),
                bar_border: Color::rgba(255, 255, 255, 28),
                surface: Color::rgba(34, 38, 52, 250),
                surface_alt: Color::rgba(255, 255, 255, 14),
                surface_hover: Color::rgba(255, 255, 255, 26),
                border: Color::rgba(255, 255, 255, 30),
                text: Color::rgb(236, 239, 245),
                text_dim: Color::rgb(152, 160, 178),
                danger: Color::rgb(239, 83, 80),
                shadow: Color::rgba(0, 0, 0, 90),
                scrim: Color::rgba(8, 10, 16, 150),
            }
        } else {
            Theme {
                dark,
                accent,
                accent_strong,
                on_accent: Color::rgb(255, 255, 255),
                window_bg: Color::rgb(232, 235, 241),
                board_bg: Color::rgb(255, 255, 255),
                bar_top: mix(Color::rgb(252, 253, 255), accent, 0.05).with_alpha(242),
                bar_bottom: mix(Color::rgb(240, 243, 248), accent, 0.07).with_alpha(248),
                bar_border: Color::rgba(15, 23, 42, 26),
                surface: Color::rgba(255, 255, 255, 252),
                surface_alt: Color::rgba(15, 23, 42, 10),
                surface_hover: Color::rgba(15, 23, 42, 18),
                border: Color::rgba(15, 23, 42, 28),
                text: Color::rgb(24, 28, 38),
                text_dim: Color::rgb(100, 108, 124),
                danger: Color::rgb(220, 53, 69),
                shadow: Color::rgba(15, 23, 42, 46),
                scrim: Color::rgba(15, 23, 42, 110),
            }
        }
    }

    pub fn mix(a: Color, b: Color, t: f32) -> Color {
        mix(a, b, t)
    }
}

/// Logical layout metrics (multiply by scale).
pub mod metrics {
    pub const BAR_H: f32 = 64.0;
    pub const BAR_MARGIN: f32 = 6.0;
    pub const BTN: f32 = 48.0;
    pub const BTN_GAP: f32 = 4.0;
    pub const SIDE_BTN_W: f32 = 46.0;
    pub const RADIUS_S: f32 = 8.0;
    pub const RADIUS_M: f32 = 12.0;
    pub const RADIUS_L: f32 = 16.0;
    pub const ICON: f32 = 20.0;
    pub const LABEL: f32 = 11.0;
    pub const BODY: f32 = 14.0;
    pub const TITLE: f32 = 17.0;
    pub const POPUP_GAP: f32 = 10.0;
}
