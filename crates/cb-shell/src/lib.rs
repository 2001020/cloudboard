//! Cross-platform window shell (winit + softbuffer) shared by the Windows
//! and macOS executables. OS specifics are supplied through [`Platform`].

use cb_core::settings::Settings;
use cb_render::text::Fonts;
use cb_render::tiny_skia::Pixmap;
use cb_ui::input::{Button, CursorIcon as UiCursor, Env, Event, HotspotResult, Key, Mods, Pointer, PointerKind, Request, Sound};
use cb_ui::App;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowAttributes, WindowId, WindowLevel};

/// OS integration implemented by each platform crate.
pub trait Platform: 'static {
    fn name(&self) -> &'static str;
    /// Key into the update manifest, e.g. "windows-x64".
    fn update_asset(&self) -> String;
    fn can_hotspot(&self) -> bool {
        false
    }
    /// Start the mobile hotspot on a background thread and call `reply`.
    fn start_hotspot(&self, reply: Box<dyn FnOnce(HotspotResult) + Send>) {
        reply(HotspotResult::Unsupported("此系统不支持自动开启热点".into()))
    }
    fn stop_hotspot(&self) {}
    fn play_sound(&self, _s: Sound) {}
    fn open_dialog(&self, window: &Window) -> Option<PathBuf>;
    fn save_dialog(&self, window: &Window, default_name: &str) -> Option<PathBuf>;
    /// Launch the installer helper for a verified package. The app exits afterwards.
    fn install_update(&self, package: &Path) -> Result<(), String>;
    fn open_url(&self, _url: &str) {}
    /// Called once the main window exists (pen input hooks, window styling).
    fn on_window_created(&self, _window: &Window, _proxy: EventLoopProxy<UserEvent>) {}
    /// Called before the first window (file associations etc.).
    fn startup(&self) {}
    fn data_dir(&self) -> Option<PathBuf>;
    fn settings_path(&self) -> Option<PathBuf> {
        Settings::default_path()
    }
    fn pdfium_path(&self) -> Option<PathBuf> {
        cb_pdf_locate()
    }
    /// Start in borderless fullscreen (classroom default).
    fn start_fullscreen(&self) -> bool {
        true
    }
}

fn cb_pdf_locate() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let names = ["pdfium.dll", "libpdfium.dylib", "libpdfium.so"];
    for n in names {
        for p in [dir.join(n), dir.join("lib").join(n), dir.join("../Frameworks").join(n), dir.join("../Resources").join(n)] {
            if p.exists() {
                return Some(p);
            }
        }
    }
    std::env::var_os("CLOUDBOARD_PDFIUM").map(PathBuf::from)
}

#[derive(Debug)]
pub enum UserEvent {
    Wake,
    Hotspot(HotspotResult),
    /// Events produced by native hooks (e.g. Windows pen input).
    Input(Event),
}

static ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/newicon.png");

fn window_icon() -> Option<winit::window::Icon> {
    let px = cb_render::images::decode(ICON_PNG)?;
    let mut rgba = Vec::with_capacity(px.data().len());
    for p in px.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    winit::window::Icon::from_rgba(rgba, px.width(), px.height()).ok()
}

struct Surface {
    window: Rc<Window>,
    surface: softbuffer::Surface<Rc<Window>, Rc<Window>>,
    frame: Option<Pixmap>,
}

impl Surface {
    fn new(window: Window) -> Option<Surface> {
        let window = Rc::new(window);
        let ctx = softbuffer::Context::new(window.clone()).ok()?;
        let surface = softbuffer::Surface::new(&ctx, window.clone()).ok()?;
        Some(Surface { window, surface, frame: None })
    }

    /// Present an RGBA (premultiplied, opaque) pixmap.
    fn present(&mut self, pm: &Pixmap) {
        let (w, h) = (pm.width(), pm.height());
        let (Some(nw), Some(nh)) = (NonZeroU32::new(w), NonZeroU32::new(h)) else { return };
        if self.surface.resize(nw, nh).is_err() {
            return;
        }
        let Ok(mut buf) = self.surface.buffer_mut() else { return };
        if buf.len() != (w * h) as usize {
            return;
        }
        for (dst, src) in buf.iter_mut().zip(pm.data().chunks_exact(4)) {
            *dst = (src[0] as u32) << 16 | (src[1] as u32) << 8 | src[2] as u32;
        }
        let _ = buf.present();
    }
}

enum Phase {
    Init,
    Loading { splash: Option<Surface>, fonts: Option<Fonts>, started: Instant, step: u8 },
    Running,
}

struct Shell<P: Platform> {
    platform: P,
    proxy: EventLoopProxy<UserEvent>,
    phase: Phase,
    app: Option<App>,
    main: Option<Surface>,
    args: Vec<PathBuf>,
    mods: Mods,
    cursor: PhysicalPosition<f64>,
    last_touch: Option<Instant>,
    settings: Settings,
}

impl<P: Platform> Shell<P> {
    fn env(&self) -> Env {
        let proxy = self.proxy.clone();
        Env {
            waker: Arc::new(move || {
                let _ = proxy.send_event(UserEvent::Wake);
            }),
            pdfium: self.platform.pdfium_path(),
            platform: self.platform.name(),
            can_hotspot: self.platform.can_hotspot(),
            update_asset: self.platform.update_asset(),
            settings_path: self.platform.settings_path(),
            data_dir: self.platform.data_dir(),
            system_dark: true,
        }
    }

    fn create_main_window(&mut self, el: &ActiveEventLoop) -> Option<Surface> {
        let mut attrs = WindowAttributes::default()
            .with_title(format!("{} {}", cb_core::version::APP_NAME, cb_core::version::major()))
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(760.0, 520.0))
            .with_window_icon(window_icon())
            .with_visible(false);
        if self.platform.start_fullscreen() {
            attrs = attrs.with_fullscreen(Some(Fullscreen::Borderless(None))).with_decorations(false);
        } else {
            attrs = attrs.with_maximized(true);
        }
        let w = el.create_window(attrs).ok()?;
        self.platform.on_window_created(&w, self.proxy.clone());
        Surface::new(w)
    }

    fn create_splash(&self, el: &ActiveEventLoop) -> Option<Surface> {
        let mon = el.primary_monitor().or_else(|| el.available_monitors().next());
        let scale = mon.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0) as f32;
        let (w, h) = cb_ui::splash::size(scale);
        let mut attrs = WindowAttributes::default()
            .with_title(cb_core::version::splash_title())
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_window_icon(window_icon())
            .with_inner_size(PhysicalSize::new(w, h));
        if let Some(m) = &mon {
            let ms = m.size();
            let mp = m.position();
            attrs = attrs.with_position(PhysicalPosition::new(mp.x + (ms.width as i32 - w as i32) / 2, mp.y + (ms.height as i32 - h as i32) / 2));
        }
        let win = el.create_window(attrs).ok()?;
        Surface::new(win)
    }

    fn draw_splash(splash: &mut Surface, fonts: &mut Fonts, started: Instant, progress: f32, status: &str) {
        let size = splash.window.inner_size();
        let scale = splash.window.scale_factor() as f32;
        let Some(mut pm) = Pixmap::new(size.width.max(1), size.height.max(1)) else { return };
        cb_ui::splash::draw(&mut pm.as_mut(), fonts, scale, started.elapsed().as_secs_f32(), progress, status);
        splash.present(&pm);
    }

    /// Advance the loading sequence; returns true when finished.
    fn loading_step(&mut self, el: &ActiveEventLoop) -> bool {
        let Phase::Loading { splash, fonts, started, step } = &mut self.phase else { return true };
        let status = ["正在读取设置…", "正在加载字体与界面…", "正在准备画板…", "正在打开文件…", "准备就绪"];
        let s = *step as usize;
        if let (Some(sp), Some(f)) = (splash.as_mut(), fonts.as_mut()) {
            Self::draw_splash(sp, f, *started, (s as f32 + 0.5) / status.len() as f32, status[s.min(status.len() - 1)]);
        }
        let started = *started;
        match s {
            0 => {}
            1 => {
                let env = self.env();
                let app = App::new(env, 1280, 800, 1.0);
                self.app = Some(app);
            }
            2 => {
                self.main = self.create_main_window(el);
                if let (Some(m), Some(app)) = (&self.main, &mut self.app) {
                    let sz = m.window.inner_size();
                    app.resize(sz.width, sz.height, m.window.scale_factor() as f32);
                    if let Some(t) = m.window.theme() {
                        app.set_system_dark(t == winit::window::Theme::Dark);
                    }
                }
            }
            3 => {
                let args = std::mem::take(&mut self.args);
                if let Some(app) = &mut self.app {
                    for a in args {
                        app.open_path(a);
                    }
                }
            }
            _ => {
                let has_splash = matches!(&self.phase, Phase::Loading { splash: Some(_), .. });
                if has_splash && started.elapsed() < Duration::from_millis(cb_ui::splash::MIN_VISIBLE_MS) {
                    return false;
                }
                self.phase = Phase::Running;
                if let Some(m) = &self.main {
                    m.window.set_visible(true);
                    m.window.focus_window();
                    m.window.request_redraw();
                }
                if let Some(app) = &mut self.app {
                    app.startup();
                }
                if self.main.is_none() {
                    log::error!("could not create the main window");
                    el.exit();
                }
                return true;
            }
        }
        if let Phase::Loading { step, .. } = &mut self.phase {
            *step += 1;
        }
        false
    }

    fn handle_requests(&mut self, el: &ActiveEventLoop) {
        let Some(app) = &mut self.app else { return };
        let reqs = app.take_requests();
        let Some(main) = &self.main else { return };
        let win = main.window.clone();
        for r in reqs {
            match r {
                Request::Exit => el.exit(),
                Request::Minimize => win.set_minimized(true),
                Request::ToggleFullscreen => {
                    if win.fullscreen().is_some() {
                        win.set_fullscreen(None);
                        win.set_decorations(true);
                    } else {
                        win.set_decorations(false);
                        win.set_fullscreen(Some(Fullscreen::Borderless(None)));
                    }
                }
                Request::OpenFileDialog => {
                    if let Some(p) = self.platform.open_dialog(&win) {
                        if let Some(app) = &mut self.app {
                            app.handle(Event::OpenPath(p));
                        }
                    }
                }
                Request::SaveFileDialog { default_name } => {
                    if let Some(p) = self.platform.save_dialog(&win, &default_name) {
                        if let Some(app) = &mut self.app {
                            app.handle(Event::SavePath(p));
                        }
                    }
                }
                Request::PlaySound(s) => self.platform.play_sound(s),
                Request::SetCursor(c) => win.set_cursor(match c {
                    UiCursor::Default => CursorIcon::Default,
                    UiCursor::Pointer => CursorIcon::Pointer,
                    UiCursor::Crosshair => CursorIcon::Crosshair,
                    UiCursor::Grab => CursorIcon::Grab,
                    UiCursor::Grabbing => CursorIcon::Grabbing,
                    UiCursor::Text => CursorIcon::Text,
                    UiCursor::Move => CursorIcon::Move,
                }),
                Request::StartHotspot => {
                    let proxy = self.proxy.clone();
                    self.platform.start_hotspot(Box::new(move |r| {
                        let _ = proxy.send_event(UserEvent::Hotspot(r));
                    }));
                }
                Request::StopHotspot => self.platform.stop_hotspot(),
                Request::InstallUpdate { package } => match self.platform.install_update(&package) {
                    Ok(()) => el.exit(),
                    Err(e) => {
                        if let Some(app) = &mut self.app {
                            app.dialog = Some(cb_ui::app::Dialog::Message { title: "无法安装更新".into(), text: e });
                            app.dirty();
                        }
                    }
                },
                Request::OpenUrl(u) => self.platform.open_url(&u),
                Request::Ime { enabled, x, y } => {
                    win.set_ime_allowed(enabled);
                    if enabled {
                        win.set_ime_cursor_area(PhysicalPosition::new(x as f64, y as f64), PhysicalSize::new(4u32, 32u32));
                    }
                }
            }
        }
    }

    fn feed(&mut self, ev: Event) {
        if let Some(app) = &mut self.app {
            app.handle(ev);
        }
    }

    fn redraw(&mut self) {
        let (Some(app), Some(main)) = (&mut self.app, &mut self.main) else { return };
        let size = main.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        if main.frame.as_ref().map(|f| (f.width(), f.height())) != Some((w, h)) {
            main.frame = Pixmap::new(w, h);
        }
        if (app.size.0, app.size.1) != (w, h) {
            app.resize(w, h, main.window.scale_factor() as f32);
        }
        let Some(mut frame) = main.frame.take() else { return };
        app.render(&mut frame.as_mut());
        main.present(&frame);
        main.frame = Some(frame);
    }
}

fn map_key(k: &WKey) -> Option<Key> {
    Some(match k {
        WKey::Named(n) => match n {
            NamedKey::Escape => Key::Escape,
            NamedKey::Enter => Key::Enter,
            NamedKey::Backspace => Key::Backspace,
            NamedKey::Delete => Key::Delete,
            NamedKey::ArrowLeft => Key::Left,
            NamedKey::ArrowRight => Key::Right,
            NamedKey::ArrowUp => Key::Up,
            NamedKey::ArrowDown => Key::Down,
            NamedKey::PageUp => Key::PageUp,
            NamedKey::PageDown => Key::PageDown,
            NamedKey::Home => Key::Home,
            NamedKey::End => Key::End,
            NamedKey::Tab => Key::Tab,
            NamedKey::Space => Key::Space,
            NamedKey::F11 => Key::F11,
            _ => return None,
        },
        WKey::Character(s) => Key::Char(s.chars().next()?.to_ascii_lowercase()),
        _ => return None,
    })
}

impl<P: Platform> ApplicationHandler<UserEvent> for Shell<P> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if !matches!(self.phase, Phase::Init) {
            return;
        }
        self.platform.startup();
        let (splash, fonts) = if self.settings.show_splash {
            let s = self.create_splash(el);
            let f = s.as_ref().map(|_| Fonts::new(&[cb_render::UI_FONT]));
            (s, f)
        } else {
            (None, None)
        };
        self.phase = Phase::Loading { splash, fonts, started: Instant::now(), step: 0 };
        el.set_control_flow(ControlFlow::Poll);
    }

    fn user_event(&mut self, _el: &ActiveEventLoop, ev: UserEvent) {
        match ev {
            UserEvent::Wake => self.feed(Event::Wake),
            UserEvent::Hotspot(r) => self.feed(Event::Hotspot(r)),
            UserEvent::Input(e) => self.feed(e),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let Phase::Loading { splash: Some(sp), fonts: Some(f), started, step } = &mut self.phase {
            if sp.window.id() == id {
                if let WindowEvent::RedrawRequested = event {
                    let st = *step as f32 / 5.0;
                    Self::draw_splash(sp, f, *started, st, "正在启动…");
                }
                return;
            }
        }
        if self.main.as_ref().map(|m| m.window.id()) != Some(id) {
            return;
        }
        let scale = self.main.as_ref().map(|m| m.window.scale_factor() as f32).unwrap_or(1.0);
        match event {
            WindowEvent::CloseRequested => {
                if let Some(app) = &mut self.app {
                    app.request_close();
                }
            }
            WindowEvent::Resized(sz) => self.feed(Event::Resize { width: sz.width, height: sz.height, scale }),
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(m) = &self.main {
                    let sz = m.window.inner_size();
                    self.feed(Event::Resize { width: sz.width, height: sz.height, scale: scale_factor as f32 });
                }
            }
            WindowEvent::ThemeChanged(t) => {
                if let Some(app) = &mut self.app {
                    app.set_system_dark(t == winit::window::Theme::Dark);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::ModifiersChanged(m) => {
                let st = m.state();
                self.mods = Mods { shift: st.shift_key(), cmd: if cfg!(target_os = "macos") { st.super_key() } else { st.control_key() }, alt: st.alt_key() };
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = position;
                // Ignore synthesized mouse input right after touch.
                if self.last_touch.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
                    return;
                }
                self.feed(Event::PointerMove(Pointer::mouse(position.x as f32, position.y as f32)));
            }
            WindowEvent::CursorLeft { .. } => self.feed(Event::PointerLeave),
            WindowEvent::MouseInput { state, button, .. } => {
                if self.last_touch.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
                    return;
                }
                let b = match button {
                    MouseButton::Left => Button::Primary,
                    MouseButton::Right => Button::Secondary,
                    MouseButton::Middle => Button::Middle,
                    _ => return,
                };
                let p = Pointer::mouse(self.cursor.x as f32, self.cursor.y as f32).with_button(b);
                self.feed(if state == ElementState::Pressed { Event::PointerDown(p) } else { Event::PointerUp(p) });
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy, lines) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x, y, true),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32, false),
                };
                self.feed(Event::Wheel { x: self.cursor.x as f32, y: self.cursor.y as f32, dx, dy, lines, mods: self.mods });
            }
            WindowEvent::PinchGesture { delta, .. } => self.feed(Event::Magnify { x: self.cursor.x as f32, y: self.cursor.y as f32, delta: delta as f32 }),
            WindowEvent::Touch(t) => {
                self.last_touch = Some(Instant::now());
                let pressure = t.force.map(|f| f.normalized() as f32).unwrap_or(1.0);
                let p = Pointer { id: t.id + 10, kind: PointerKind::Touch, x: t.location.x as f32, y: t.location.y as f32, pressure, eraser: false, button: Button::Primary };
                self.feed(match t.phase {
                    TouchPhase::Started => Event::PointerDown(p),
                    TouchPhase::Moved => Event::PointerMove(p),
                    TouchPhase::Ended => Event::PointerUp(p),
                    TouchPhase::Cancelled => Event::PointerCancel(p.id),
                });
            }
            WindowEvent::KeyboardInput { event, is_synthetic: false, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                if let Some(k) = map_key(&event.logical_key) {
                    self.feed(Event::Key { key: k.clone(), mods: self.mods });
                }
                if let Some(t) = &event.text {
                    if !self.mods.cmd && !t.chars().any(|c| c.is_control()) {
                        self.feed(Event::Text(t.to_string()));
                    }
                }
            }
            WindowEvent::Ime(winit::event::Ime::Commit(s)) => self.feed(Event::Text(s)),
            WindowEvent::DroppedFile(p) => self.feed(Event::OpenPath(p)),
            _ => {}
        }
        let _ = el;
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if matches!(self.phase, Phase::Loading { .. }) {
            if !self.loading_step(el) {
                el.set_control_flow(ControlFlow::Poll);
                return;
            }
        }
        if !matches!(self.phase, Phase::Running) {
            return;
        }
        if let Some(app) = &mut self.app {
            app.poll();
        }
        self.handle_requests(el);
        let Some(app) = &self.app else { return };
        if app.needs_redraw() {
            if let Some(m) = &self.main {
                m.window.request_redraw();
            }
        }
        match app.next_deadline() {
            Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// Run the application. `args` are file paths from the command line.
pub fn run<P: Platform>(platform: P, args: Vec<PathBuf>) -> Result<(), String> {
    let el = EventLoop::<UserEvent>::with_user_event().build().map_err(|e| e.to_string())?;
    let proxy = el.create_proxy();
    let settings = platform.settings_path().map(|p| Settings::load_from(&p)).unwrap_or_default();
    let mut shell = Shell { platform, proxy, phase: Phase::Init, app: None, main: None, args, mods: Mods::default(), cursor: PhysicalPosition::new(0.0, 0.0), last_touch: None, settings };
    el.run_app(&mut shell).map_err(|e| e.to_string())
}

/// Parse command-line file arguments (ignores flags).
pub fn file_args() -> Vec<PathBuf> {
    std::env::args_os().skip(1).map(PathBuf::from).filter(|p| !p.to_string_lossy().starts_with("--") && p.exists()).collect()
}
pub use winit;
