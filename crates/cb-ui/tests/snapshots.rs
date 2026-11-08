//! Offscreen UI snapshots at several DPI scales plus scripted interaction.
//! PNGs are written to `target/snapshots/` for visual review.

use cb_core::assist::AssistKind;
use cb_render::tiny_skia::Pixmap;
use cb_ui::input::{Event, Pointer};
use cb_ui::widgets::{Action, PopupKind};
use cb_ui::{App, Env};
use std::path::PathBuf;

fn out_dir() -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/snapshots");
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn shot(app: &mut App, name: &str) -> Pixmap {
    let (w, h) = app.size;
    let mut pm = Pixmap::new(w, h).unwrap();
    app.render(&mut pm.as_mut());
    pm.save_png(out_dir().join(format!("{name}.png"))).unwrap();
    pm
}

fn app(w: u32, h: u32, scale: f32) -> App {
    let mut env = Env::headless();
    env.pdfium = std::env::var_os("CLOUDBOARD_PDFIUM").map(PathBuf::from);
    App::new(env, w, h, scale)
}

fn draw_line(app: &mut App, from: (f32, f32), to: (f32, f32), n: usize) {
    app.handle(Event::PointerDown(Pointer::pen(from.0, from.1, 0.4)));
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let x = from.0 + (to.0 - from.0) * t;
        let y = from.1 + (to.1 - from.1) * t + (t * 9.0).sin() * 20.0;
        app.handle(Event::PointerMove(Pointer::pen(x, y, 0.3 + 0.7 * (t * 3.14).sin())));
    }
    app.handle(Event::PointerUp(Pointer::pen(to.0, to.1, 0.5)));
}

#[test]
fn chrome_at_multiple_scales() {
    for (scale, w, h) in [(1.0f32, 1366u32, 768u32), (1.25, 1600, 900), (1.5, 1920, 1080), (2.0, 2560, 1440), (2.5, 3840, 2160)] {
        let mut a = app(w, h, scale);
        draw_line(&mut a, (200.0 * scale, 200.0 * scale), (700.0 * scale, 260.0 * scale), 40);
        a.perform(Action::TogglePopup(PopupKind::Pen));
        let pm = shot(&mut a, &format!("bar_pen_popup_{}x", (scale * 100.0) as u32));
        // The bottom bar occupies the bottom 64 logical px and differs from the board.
        let y = h - (32.0 * scale) as u32;
        let px = pm.pixel(w / 2, y).unwrap();
        assert!(px.alpha() == 255);
        // The pen stroke made it into the document.
        assert_eq!(a.doc.pages[0].strokes.len(), 1);
    }
}

#[test]
fn dialogs_panels_and_aids() {
    let mut a = app(1600, 900, 1.25);
    a.perform(Action::OpenSettings);
    shot(&mut a, "settings_dialog");
    a.perform(Action::CloseDialog);
    a.perform(Action::ToggleTimer);
    a.perform(Action::TimerStartPause);
    shot(&mut a, "timer_panel");
    a.perform(Action::ToggleTimer);
    for k in [AssistKind::Ruler, AssistKind::Triangle45, AssistKind::Triangle30, AssistKind::Protractor, AssistKind::Compass] {
        a.perform(Action::ToggleAssist(k));
    }
    // Spread them out.
    let n = a.assist.tools.len();
    for (i, t) in a.assist.tools.iter_mut().enumerate() {
        t.pos = cb_core::v2(80.0 + (i % 3) as f32 * 420.0, 120.0 + (i / 3) as f32 * 300.0) + if t.kind == AssistKind::Triangle45 || t.kind == AssistKind::Triangle30 { cb_core::v2(0.0, 420.0) } else { cb_core::v2(0.0, 0.0) };
    }
    assert_eq!(n, 5);
    a.perform(Action::TogglePopup(PopupKind::Assist));
    shot(&mut a, "teaching_aids");
    a.perform(Action::TogglePopup(PopupKind::Assist));
    a.perform(Action::ToggleRemote);
    shot(&mut a, "remote_panel");
    a.perform(Action::ToggleRemote);
    a.perform(Action::TogglePopup(PopupKind::Eraser));
    shot(&mut a, "eraser_popup");
}

#[test]
fn light_theme_and_narrow_window() {
    let mut a = app(1024, 700, 1.0);
    a.perform(Action::Theme(cb_core::settings::ThemeMode::Light));
    a.perform(Action::TogglePopup(PopupKind::Zoom));
    shot(&mut a, "light_narrow_zoom_popup");
    let mut a = app(800, 600, 1.0);
    shot(&mut a, "very_narrow");
    let ws = a.widget_list();
    // No two bar buttons overlap.
    let btns: Vec<_> = ws.iter().filter(|w| matches!(w.kind, cb_ui::widgets::WKind::Button { .. })).collect();
    for (i, x) in btns.iter().enumerate() {
        for y in &btns[i + 1..] {
            assert!(!x.rect.intersects(&y.rect), "overlap: {:?} {:?}", x.action, y.action);
        }
    }
}

#[test]
fn splash_screens() {
    let mut fonts = cb_render::text::Fonts::new(&[cb_render::UI_FONT]);
    for scale in [1.0f32, 1.5, 2.0] {
        let (w, h) = cb_ui::splash::size(scale);
        let mut pm = Pixmap::new(w, h).unwrap();
        cb_ui::splash::draw(&mut pm.as_mut(), &mut fonts, scale, 1.0, 0.6, "正在加载字体…");
        pm.save_png(out_dir().join(format!("splash_{}x.png", (scale * 100.0) as u32))).unwrap();
    }
}

#[test]
fn pdf_document() {
    if std::env::var_os("CLOUDBOARD_PDFIUM").is_none() {
        eprintln!("CLOUDBOARD_PDFIUM not set; skipping");
        return;
    }
    let mut a = app(1600, 900, 1.25);
    let bytes = std::sync::Arc::new(cb_pdf::testpdf::make_pdf(30, 60));
    a.open_pdf_bytes("test.pdf".into(), bytes);
    assert!(a.is_pdf());
    assert_eq!(a.page_count(), 30);
    // Wait for the visible tiles.
    let t0 = std::time::Instant::now();
    loop {
        a.handle(Event::Wake);
        let mut pm = Pixmap::new(1600, 900).unwrap();
        a.render(&mut pm.as_mut());
        if a.pdf_pending() == 0 && a.pdf_cached_tiles() > 2 || t0.elapsed().as_secs() > 20 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    draw_line(&mut a, (400.0, 300.0), (900.0, 400.0), 30);
    shot(&mut a, "pdf_view");
    assert_eq!(a.doc.pages[0].strokes.len(), 1);
}
