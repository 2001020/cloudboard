//! PDF rendering for CloudBoard: PDFium on a dedicated thread, a tile
//! pyramid with previews, and an LRU tile cache.

pub mod cache;
pub mod engine;
pub mod ffi;

pub use cache::{DrawTile, TileCache, TileKey, TileReq, VisiblePage};
pub use engine::{PageSource, locate_library, DocInfo, PdfEngine, PdfError, TileResult, Waker};
pub mod testpdf;
