//! PowerPoint (.pptx) and Word (.docx) rendering without Office: the
//! OOXML package is parsed into a display model (`model`) that is laid out
//! with cosmic-text and drawn with tiny-skia.

pub mod geom;
pub mod model;
pub mod opc;
pub mod render;
pub mod text;

pub use model::{Document, Effect, Page, Step};
pub use render::{Filter, Renderer};
