//! Scroll benchmark: opens a heavy multi-page PDF and measures the UI-thread
//! cost per frame while scrolling continuously (like a fast flick).
//!
//!   CLOUDBOARD_PDFIUM=/path/libpdfium.so taskset -c 0,1 \
//!     cargo run --release -p cb-ui --example pdf_scroll_bench -- [pages] [width] [height] [scale]

use cb_render::tiny_skia::Pixmap;
use cb_ui::input::Event;
use cb_ui::{App, Env};
use std::time::{Duration, Instant};

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let pages: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);
    let (w, h): (u32, u32) = (args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1920), args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1080));
    let scale: f32 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let mut env = Env::headless();
    env.pdfium = std::env::var_os("CLOUDBOARD_PDFIUM").map(Into::into);
    let mut app = App::new(env, w, h, scale);
    let pdf = std::sync::Arc::new(cb_pdf::testpdf::make_pdf(pages, 400));
    let t_open = Instant::now();
    app.open_pdf_bytes("bench.pdf".into(), pdf);
    let open_ms = t_open.elapsed().as_secs_f64() * 1000.0;
    let mut frame = Pixmap::new(w, h).unwrap();
    // First frame with content.
    let t0 = Instant::now();
    loop {
        app.handle(Event::Wake);
        app.render(&mut frame.as_mut());
        if app.pdf_cached_tiles() >= 3 || t0.elapsed() > Duration::from_secs(10) {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let first_ms = t0.elapsed().as_secs_f64() * 1000.0;
    // Scroll: 40 px per frame at 60 fps for 6 seconds (~14k px ≈ 10 pages).
    let mut times = Vec::new();
    let mut full_frames = 0;
    let frames = 360;
    for _ in 0..frames {
        let t = Instant::now();
        app.handle(Event::Wheel { x: w as f32 / 2.0, y: h as f32 / 2.0, dx: 0.0, dy: -40.0, lines: false, mods: Default::default() });
        app.render(&mut frame.as_mut());
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        if app.board.last_full {
            full_frames += 1;
        }
        let spent = t.elapsed();
        if spent < Duration::from_micros(16_667) {
            std::thread::sleep(Duration::from_micros(16_667) - spent);
        }
    }
    let mut t2 = times.clone();
    println!("pages={pages} size={w}x{h} scale={scale}");
    println!("open: {open_ms:.1} ms, first content: {first_ms:.1} ms");
    println!(
        "scroll frame (UI thread): p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms, full redraws {}/{}",
        pct(&mut t2.clone(), 0.5),
        pct(&mut t2.clone(), 0.95),
        pct(&mut t2.clone(), 0.99),
        pct(&mut t2, 1.0),
        full_frames,
        frames
    );
    println!("tiles cached: {}, pending: {}, display page: {}", app.pdf_cached_tiles(), app.pdf_pending(), app.display_page() + 1);
    frame.save_png("target/snapshots/bench_last_frame.png").ok();
}
