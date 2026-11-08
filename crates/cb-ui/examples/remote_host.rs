//! Headless host for the phone-link end-to-end test: starts the app with the
//! LAN server, writes the client URL to a file, then runs for N seconds and
//! reports how many strokes arrived.
//!
//!   cargo run -p cb-ui --example remote_host -- <url-file> <seconds>

use cb_render::tiny_skia::Pixmap;
use cb_ui::input::Event;
use cb_ui::widgets::Action;
use cb_ui::{App, Env};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let url_file = args.get(1).cloned().unwrap_or_else(|| "remote_url.txt".into());
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(20);
    let mut app = App::new(Env::headless(), 1280, 800, 1.0);
    app.perform(Action::ToggleRemote);
    let mut frame = Pixmap::new(1280, 800).unwrap();
    app.render(&mut frame.as_mut());
    let url = app.remote.as_ref().and_then(|r| r.server.as_ref()).map(|s| format!("http://127.0.0.1:{}/?t={}", s.info().port, s.info().token)).expect("server");
    std::fs::write(&url_file, &url).unwrap();
    println!("URL {url}");
    let t0 = Instant::now();
    let mut connected = false;
    let mut images = 0usize;
    let mut undone = false;
    let mut strokes_seen = 0usize;
    while t0.elapsed() < Duration::from_secs(secs) {
        app.handle(Event::Wake);
        if app.needs_redraw() {
            app.render(&mut frame.as_mut());
        }
        let n_img: usize = app.doc.pages.iter().map(|p| p.images.len()).sum();
        if n_img != images {
            images = n_img;
            println!("IMAGES {images}");
            if images >= 3 && !undone {
                undone = true;
                app.perform(Action::Undo);
                let left: usize = app.doc.pages.iter().map(|p| p.images.len()).sum();
                println!("IMAGES_AFTER_UNDO {left}");
                images = left;
            }
        }
        let n_str: usize = app.doc.pages.iter().map(|p| p.strokes.len()).sum();
        if n_str != strokes_seen {
            strokes_seen = n_str;
            // Report the newest stroke in normalised viewport coordinates.
            if let Some(s) = app.doc.pages[app.page].strokes.last() {
                let o = app.layout.origin(app.page);
                let (x0, x1) = s.points.iter().fold((f32::MAX, f32::MIN), |a, p| (a.0.min(p.x), a.1.max(p.x)));
                let y = s.points.iter().map(|p| p.y).sum::<f32>() / s.points.len().max(1) as f32;
                let a = app.vp.to_screen(cb_core::v2(x0, y) + o);
                let b = app.vp.to_screen(cb_core::v2(x1, y) + o);
                let r = app.vp.rect;
                println!("LAST_STROKE_NORM {:.3} {:.3} {:.3}", (a.x - r.x0) / r.w(), (b.x - r.x0) / r.w(), (a.y - r.y0) / r.h());
            }
        }
        let c = app.remote.as_ref().map(|r| r.peer.is_some()).unwrap_or(false);
        if c != connected {
            connected = c;
            println!("CONNECTED {c}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    app.render(&mut frame.as_mut());
    let strokes = app.doc.pages.iter().map(|p| p.strokes.len()).sum::<usize>();
    let pts: usize = app.doc.pages.iter().flat_map(|p| p.strokes.iter()).map(|s| s.points.len()).sum();
    println!("STROKES {strokes} POINTS {pts} PAGES {}", app.page_count());
    frame.save_png("target/snapshots/remote_host_final.png").ok();
}
