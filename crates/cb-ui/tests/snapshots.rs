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
    a.perform(Action::TogglePopup(PopupKind::More));
    shot(&mut a, "light_narrow_more_popup");
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

#[test]
fn set_square_closeups() {
    for (kind, name) in [(AssistKind::Triangle30, "aid_triangle30"), (AssistKind::Triangle45, "aid_triangle45"), (AssistKind::Ruler, "aid_ruler"), (AssistKind::Protractor, "aid_protractor")] {
        let mut a = app(1400, 900, 2.0);
        a.perform(Action::ToggleAssist(kind));
        shot(&mut a, name);
    }
}

#[test]
fn more_menu_zoom_slider_and_slide_confirm() {
    for (scale, w, h) in [(1.0f32, 1366u32, 768u32), (1.5, 1920, 1080), (2.5, 3840, 2160)] {
        let mut a = app(w, h, scale);
        a.perform(Action::TogglePopup(PopupKind::More));
        shot(&mut a, &format!("more_popup_{}x", (scale * 100.0) as u32));
        let ws = a.widget_list();
        // Zoom slider sets the zoom continuously.
        let sl = ws.iter().find(|w| w.action == Action::ZoomSlider).expect("zoom slider");
        let (x, y) = (sl.rect.x0 + sl.rect.w() * 0.75, sl.rect.center().y);
        a.handle(Event::PointerDown(Pointer::mouse(x, y)));
        a.handle(Event::PointerUp(Pointer::mouse(x, y)));
        assert!(a.vp.zoom > 1.9 && a.vp.zoom < 2.1, "zoom {}", a.vp.zoom);
        // Nothing in the menu overlaps.
        let ws = a.widget_list();
        let items: Vec<_> = ws.iter().filter(|w| matches!(w.kind, cb_ui::widgets::WKind::Button { .. } | cb_ui::widgets::WKind::Slider { .. } | cb_ui::widgets::WKind::SlideConfirm { .. })).collect();
        for (i, x) in items.iter().enumerate() {
            for y in &items[i + 1..] {
                assert!(!x.rect.intersects(&y.rect), "overlap: {:?} {:?}", x.action, y.action);
            }
        }
    }
    // Slide to clear: a partial slide springs back, a full slide clears.
    let mut a = app(1600, 900, 1.25);
    draw_line(&mut a, (300.0, 300.0), (800.0, 360.0), 20);
    assert_eq!(a.doc.pages[0].strokes.len(), 1);
    a.perform(Action::TogglePopup(PopupKind::More));
    let ws = a.widget_list();
    let sc = ws.iter().find(|w| matches!(w.kind, cb_ui::widgets::WKind::SlideConfirm { .. })).unwrap().rect;
    let y = sc.center().y;
    let k0 = sc.x0 + sc.h() * 0.5;
    a.handle(Event::PointerDown(Pointer::mouse(k0, y)));
    a.handle(Event::PointerMove(Pointer::mouse(k0 + sc.w() * 0.5, y)));
    shot(&mut a, "slide_confirm_half");
    a.handle(Event::PointerUp(Pointer::mouse(k0 + sc.w() * 0.5, y)));
    assert_eq!(a.doc.pages[0].strokes.len(), 1, "half slide must not clear");
    std::thread::sleep(std::time::Duration::from_millis(260));
    a.handle(Event::Wake);
    a.perform(Action::TogglePopup(PopupKind::More));
    a.perform(Action::TogglePopup(PopupKind::More));
    a.handle(Event::PointerDown(Pointer::mouse(k0, y)));
    a.handle(Event::PointerMove(Pointer::mouse(sc.x1 + 20.0, y)));
    a.handle(Event::PointerUp(Pointer::mouse(sc.x1 + 20.0, y)));
    assert_eq!(a.doc.pages[0].strokes.len(), 0, "full slide clears");
    a.perform(Action::Undo);
    assert_eq!(a.doc.pages[0].strokes.len(), 1);
}

#[test]
fn timer_minutes_and_seconds() {
    let mut a = app(1600, 900, 1.25);
    a.perform(Action::ToggleTimer);
    shot(&mut a, "timer_idle");
    let total = |a: &App| a.timer.as_ref().unwrap().countdown.total.as_secs();
    let t0 = total(&a);
    a.perform(Action::TimerAdjustSec(1));
    assert_eq!(total(&a), t0 + 1);
    a.perform(Action::TimerAdjustMin(1));
    assert_eq!(total(&a), t0 + 61);
    // The up/down buttons exist for both parts and respond to presses.
    let ws = a.widget_list();
    let up_sec = ws.iter().find(|w| w.action == Action::TimerAdjustSec(1)).unwrap().rect;
    a.handle(Event::PointerDown(Pointer::mouse(up_sec.center().x, up_sec.center().y)));
    a.handle(Event::PointerUp(Pointer::mouse(up_sec.center().x, up_sec.center().y)));
    assert_eq!(total(&a), t0 + 62, "fires once on a tap");
    assert!(ws.iter().any(|w| w.action == Action::TimerAdjustMin(-1)));
}

#[test]
fn bar_has_core_tools_only() {
    for (w, h, scale) in [(800u32, 600u32, 1.0f32), (1366, 768, 1.0), (2560, 1440, 2.0)] {
        let mut a = app(w, h, scale);
        let ws = a.widget_list();
        let bar: Vec<_> = ws.iter().filter(|x| matches!(x.kind, cb_ui::widgets::WKind::Button { .. })).collect();
        let acts: Vec<_> = bar.iter().map(|b| b.action.clone()).collect();
        for want in [Action::Exit, Action::Undo, Action::Redo, Action::PrevPage, Action::NextPage, Action::TogglePopup(PopupKind::More)] {
            assert!(acts.contains(&want), "{want:?} missing at {w}x{h}");
        }
        assert!(!acts.contains(&Action::Open) && !acts.contains(&Action::ToggleTimer));
        for (i, x) in bar.iter().enumerate() {
            for y in &bar[i + 1..] {
                assert!(!x.rect.intersects(&y.rect), "overlap: {:?} {:?}", x.action, y.action);
            }
        }
        shot(&mut a, &format!("bar_{w}x{h}"));
    }
}

#[test]
fn phone_status_icon_and_toast() {
    let mut a = app(1600, 900, 1.25);
    a.perform(Action::ToggleRemote);
    let has_status = |a: &mut App| a.widget_list().iter().any(|w| matches!(w.kind, cb_ui::widgets::WKind::StatusDot { .. }));
    assert!(has_status(&mut a));
    // Simulate a phone connecting: the panel hides, the icon stays, a toast shows.
    if let Some(r) = a.remote.as_mut() {
        r.peer = Some("192.168.137.12".into());
        r.panel_open = false;
    }
    a.toast_for("手机已连接", std::time::Duration::from_secs(2));
    shot(&mut a, "phone_connected_toast");
    assert!(has_status(&mut a));
    assert_eq!(a.toast_text(), Some("手机已连接"));
    // Clicking the icon opens the panel again.
    a.perform(Action::RemotePanel);
    assert!(a.remote.as_ref().unwrap().panel_open);
    shot(&mut a, "phone_panel_connected");
    a.perform(Action::RemoteStop);
    assert!(a.remote.is_none());
}

#[test]
fn recording_flow_with_mock_encoder() {
    use std::sync::{Arc, Mutex};
    struct FileSink(std::path::PathBuf, Arc<Mutex<(u32, u32, usize)>>);
    impl cb_record::VideoSink for FileSink {
        fn video(&mut self, bgra: &[u8], _pts: i64, _dur: i64) -> Result<(), String> {
            self.1.lock().unwrap().2 += 1;
            assert!(!bgra.is_empty());
            Ok(())
        }
        fn audio(&mut self, _pcm: &[i16], _pts: i64, _dur: i64) -> Result<(), String> {
            Ok(())
        }
        fn finish(self: Box<Self>) -> Result<(), String> {
            std::fs::write(&self.0, b"fake mp4").map_err(|e| e.to_string())
        }
    }
    let info = Arc::new(Mutex::new((0u32, 0u32, 0usize)));
    let i2 = info.clone();
    let dir = std::env::temp_dir().join(format!("cb-rec-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut env = Env::headless();
    env.data_dir = Some(dir.clone());
    env.recorder = Some(cb_record::Backend {
        sink: Arc::new(move |c: &cb_record::SinkConfig| {
            *i2.lock().unwrap() = (c.width, c.height, 0);
            Ok(Box::new(FileSink(c.path.clone(), i2.clone())) as Box<dyn cb_record::VideoSink>)
        }),
        microphone: None,
        system_audio: None,
    });
    let mut a = App::new(env, 2560, 1440, 2.0);
    a.settings.record_resolution = cb_core::settings::RecordResolution::P720;
    a.perform(Action::ToggleRecording);
    assert!(a.recording.is_some());
    for _ in 0..10 {
        let mut pm = Pixmap::new(2560, 1440).unwrap();
        a.render(&mut pm.as_mut());
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    shot(&mut a, "recording_indicator");
    let ws = a.widget_list();
    assert!(ws.iter().any(|w| w.action == Action::ToggleRecording && matches!(&w.kind, cb_ui::widgets::WKind::Button { label, .. } if label.contains("停止录制"))));
    a.perform(Action::ToggleRecording);
    assert!(a.recording.is_none());
    let (w, h, frames) = *info.lock().unwrap();
    assert_eq!((w, h), (1280, 720));
    assert!(frames >= 2, "frames {frames}");
    assert!(a.take_requests().iter().any(|r| matches!(r, cb_ui::input::Request::SaveVideoDialog { default_name } if default_name.ends_with(".mp4"))));
    // Cancelling asks again; choosing a path moves the file there.
    a.handle(Event::SaveVideoPath(None));
    assert!(matches!(a.dialog, Some(cb_ui::app::Dialog::Confirm { .. })));
    a.perform(Action::DialogButton(cb_ui::app::DLG_VIDEO_RETRY));
    let dst = dir.join("课堂录制");
    a.handle(Event::SaveVideoPath(Some(dst.clone())));
    let saved = dst.with_extension("mp4");
    assert_eq!(std::fs::read(&saved).unwrap(), b"fake mp4");
    let _ = std::fs::remove_dir_all(&dir);
}

fn wait_tiles(a: &mut App, secs: u64) {
    let t0 = std::time::Instant::now();
    loop {
        a.handle(Event::Wake);
        let mut pm = Pixmap::new(a.size.0, a.size.1).unwrap();
        a.render(&mut pm.as_mut());
        if a.pdf_pending() == 0 && a.pdf_cached_tiles() > 2 && t0.elapsed().as_millis() > 700 || t0.elapsed().as_secs() > secs {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

#[test]
fn office_documents_and_presentation() {
    let data = |n: &str| std::sync::Arc::new(std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cb-office/tests/data").join(n)).unwrap());
    // Word: pages laid out like a PDF strip.
    let mut a = app(1600, 900, 1.25);
    a.open_pdf_bytes("sample.docx".into(), data("sample.docx"));
    assert!(a.is_pdf());
    assert!(a.page_count() >= 3);
    wait_tiles(&mut a, 20);
    shot(&mut a, "office_docx");
    // PowerPoint: six slides, presentation mode with build steps.
    let mut a = app(1600, 900, 1.25);
    a.open_pdf_bytes("sample.pptx".into(), data("sample.pptx"));
    assert_eq!(a.page_count(), 6);
    assert!(a.deck.is_some());
    wait_tiles(&mut a, 20);
    shot(&mut a, "office_pptx");
    // "放映" appears in the More menu.
    a.perform(Action::TogglePopup(PopupKind::More));
    assert!(a.widget_list().iter().any(|w| w.action == Action::ShowStart));
    a.perform(Action::TogglePopup(PopupKind::More));
    a.perform(Action::NextPage);
    a.perform(Action::NextPage);
    a.perform(Action::NextPage);
    a.perform(Action::NextPage);
    a.perform(Action::ShowStart);
    let slide = a.show.as_ref().unwrap().slide;
    assert_eq!(slide, 4, "starts on the current slide");
    let frame = |a: &mut App| {
        let mut pm = Pixmap::new(1600, 900).unwrap();
        a.render(&mut pm.as_mut());
        pm
    };
    let settle = |a: &mut App| {
        std::thread::sleep(std::time::Duration::from_millis(450));
        a.handle(Event::Wake);
    };
    // Step 0: the three boxes are hidden. Count blue-ish pixels as a proxy.
    let blue = |pm: &Pixmap| pm.pixels().iter().filter(|c| c.blue() > 150 && c.red() < 140).count();
    let b0 = blue(&frame(&mut a));
    shot(&mut a, "show_step0");
    a.handle(Event::Key { key: cb_ui::input::Key::Right, mods: Default::default() });
    // Mid-animation frame, then settled.
    std::thread::sleep(std::time::Duration::from_millis(120));
    a.handle(Event::Wake);
    shot(&mut a, "show_step1_mid");
    settle(&mut a);
    let b1 = blue(&frame(&mut a));
    a.handle(Event::Key { key: cb_ui::input::Key::Right, mods: Default::default() });
    settle(&mut a);
    a.handle(Event::Key { key: cb_ui::input::Key::Right, mods: Default::default() });
    settle(&mut a);
    let b3 = blue(&frame(&mut a));
    shot(&mut a, "show_step3");
    assert!(b1 > b0 && b3 > b1, "boxes appear step by step: {b0} {b1} {b3}");
    assert_eq!(a.show.as_ref().unwrap().step, 3);
    // Ink during the show lands on the slide's page.
    let before = a.doc.pages[4].strokes.len();
    draw_line(&mut a, (500.0, 700.0), (1100.0, 720.0), 20);
    assert_eq!(a.doc.pages[4].strokes.len(), before + 1);
    // Next moves to slide 6, Esc exits.
    a.handle(Event::Key { key: cb_ui::input::Key::Right, mods: Default::default() });
    settle(&mut a);
    assert_eq!(a.show.as_ref().unwrap().slide, 5);
    a.handle(Event::Key { key: cb_ui::input::Key::Escape, mods: Default::default() });
    assert!(a.show.is_none());
}

#[test]
fn project_with_embedded_pptx_round_trips() {
    let bytes = std::sync::Arc::new(std::fs::read(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../cb-office/tests/data/sample.pptx")).unwrap());
    let mut a = app(1366, 768, 1.0);
    a.open_pdf_bytes("课件.pptx".into(), bytes);
    draw_line(&mut a, (300.0, 300.0), (700.0, 320.0), 20);
    let view = cb_core::format::ViewState { current_page: 0, zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 };
    let data = cb_core::format::save(&a.doc, &view, Some(&a.assist), "27.0.0");
    let path = std::env::temp_dir().join(format!("cb-pptx-{}.cloudboard", std::process::id()));
    std::fs::write(&path, data).unwrap();
    let mut b = app(1366, 768, 1.0);
    b.open_path(path.clone());
    assert_eq!(b.page_count(), 6);
    assert!(b.deck.is_some(), "deck reloaded for presentation");
    let strokes: usize = b.doc.pages.iter().map(|p| p.strokes.len()).sum();
    assert_eq!(strokes, 1);
    let _ = std::fs::remove_file(path);
}
