//! Integration test against a real PDFium build. Skipped unless
//! CLOUDBOARD_PDFIUM points at the library.

use cb_pdf::cache::{TileCache, VisiblePage, PREVIEW_LEVEL};
use cb_pdf::{testpdf, PdfEngine};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[test]
fn renders_tiles_in_priority_order() {
    let Some(lib) = std::env::var_os("CLOUDBOARD_PDFIUM") else {
        eprintln!("CLOUDBOARD_PDFIUM not set; skipping");
        return;
    };
    let mut eng = PdfEngine::start(std::path::Path::new(&lib), Arc::new(|| {})).expect("load pdfium");
    let info = eng.open(Arc::new(testpdf::make_pdf(20, 50))).expect("open");
    assert_eq!(info.pages.len(), 20);
    assert!((info.pages[0].0 - 595.0).abs() < 0.5);
    let mut cache = TileCache::new(128 << 20);
    let vp = VisiblePage { page: 0, size_pt: info.pages[0], x0: 0.0, y0: 0.0, x1: 595.0, y1: 842.0, distance: 0.0 };
    let jobs = cache.plan(&[vp], 1.0, &info.pages, 20);
    let n = jobs.len();
    eng.set_wanted(jobs);
    let t0 = Instant::now();
    let mut got = Vec::new();
    while got.len() < n && t0.elapsed() < Duration::from_secs(30) {
        if let Some(r) = eng.try_recv() {
            got.push(r.key);
            cache.insert(r.key, r.pixmap);
        } else {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    assert_eq!(got.len(), n);
    assert_eq!(got[0].level, PREVIEW_LEVEL);
    assert_eq!(got[0].page, 0);
    // The rendered tile is not blank: the red rectangle is near the bottom.
    let dl = cache.draw_list(&vp, 1.0);
    let px: Vec<_> = dl.iter().flat_map(|t| t.pixmap.pixels().to_vec()).collect();
    assert!(px.iter().any(|p| p.red() > 200 && p.green() < 100 && p.blue() < 100), "expected red pixels");
    assert!(px.iter().any(|p| p.red() < 80 && p.green() < 80 && p.blue() < 80), "expected dark text pixels");
    assert!(px.iter().all(|p| p.alpha() == 255));
}
