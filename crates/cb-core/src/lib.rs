//! CloudBoard core: platform independent document model and logic.

pub mod assist;
pub mod doc;
pub mod erase;
pub mod format;
pub mod geom;
pub mod history;
pub mod ink;
pub mod settings;
pub mod shape;
pub mod timer;
pub mod version;

pub use doc::{Color, Document, Item, Page, Stroke};
pub use geom::{v2, Rect, Vec2};
