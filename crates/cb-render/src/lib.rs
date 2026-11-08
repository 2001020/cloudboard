//! CloudBoard rendering layer (CPU raster via tiny-skia, text via cosmic-text).

pub mod assist;
pub mod icons;
pub mod images;
pub mod ink;
pub mod paint;
pub mod svgpath;
pub mod text;
pub mod theme;

pub use tiny_skia;

/// Embedded UI font (Latin); CJK falls back to system fonts.
pub static UI_FONT: &[u8] = include_bytes!("../../../assets/fonts/AnthropicSans-Romans-Variable-25x258.ttf");
