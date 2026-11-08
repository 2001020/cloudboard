//! Application state machine: input routing, tools, document operations,
//! background services and frame composition.

use crate::board::{BoardInput, BoardRenderer, PT_TO_UNITS};
use crate::input::{Button, CursorIcon, Env, Event, HotspotResult, Key, Mods, Pointer, PointerKind, Request, Sound};
use crate::view::{Layout, Viewport, ZOOM_MAX, ZOOM_MIN};
use crate::widgets::{self, Action, PanelKind, PopupKind, Widget};
use cb_core::assist::{arc_points, ArcTracker, AssistHit, AssistKind, AssistState};
use cb_core::doc::{Color, Document, ImageItem, InkPoint, Item, LineStyle, Page, PageBackground, PdfSource, Stroke, TextItem, Uid};
use cb_core::erase;
use cb_core::format::{self, ViewState};
use cb_core::geom::{v2, Rect, Vec2};
use cb_core::history::{remove_command, Command, History};
use cb_core::ink::InkBuilder;
use cb_core::settings::{Settings, ThemeMode};
use cb_core::timer::{Countdown, RunState, Stopwatch};
use cb_pdf::{PdfEngine, TileCache, VisiblePage};
use cb_render::text::Fonts;
use cb_render::theme::{metrics, Theme, PEN_COLORS, PEN_WIDTHS};
use cb_render::tiny_skia::{Pixmap, PixmapMut, Transform};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Eraser,
    Pan,
    Select,
    Text,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Dialog {
    Confirm { title: String, message: String, buttons: Vec<(u32, String, widgets::BtnStyle)> },
    Settings,
    Update(UpdateUi),
    Message { title: String, text: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateUi {
    Checking,
    UpToDate,
    Available(cb_update::Release),
    Downloading { done: u64, total: u64, version: String },
    Ready { package: PathBuf, version: String },
    Failed(String),
}

pub const DLG_EXIT_SAVE: u32 = 1;
pub const DLG_EXIT_DISCARD: u32 = 2;
pub const DLG_CANCEL: u32 = 3;
pub const DLG_DELETE_PAGE: u32 = 4;
pub const DLG_RESET: u32 = 5;
pub const DLG_CLEAR: u32 = 6;
pub const DLG_OPEN_DISCARD: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerTab {
    Countdown,
    Stopwatch,
}

pub struct TimerPanel {
    pub pos: Vec2,
    pub tab: TimerTab,
    pub countdown: Countdown,
    pub stopwatch: Stopwatch,
    pub alarm_since: Option<Instant>,
    pub last_alarm_sound: Option<Instant>,
    pub fullscreen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HotspotUi {
    NotAttempted,
    Starting,
    On { ssid: String, passphrase: String },
    Unavailable(String),
}

pub struct RemotePanel {
    pub pos: Vec2,
    pub server: Option<cb_remote::RemoteServer>,
    pub error: Option<String>,
    pub hotspot: HotspotUi,
    pub qr: Option<(usize, Vec<bool>)>,
    pub url: String,
    pub peer: Option<String>,
    pub last_preview: Option<Instant>,
    pub preview_rev: u64,
    pub strokes: HashMap<u32, (usize, Stroke, bool)>,
}

#[derive(Debug, Clone)]
pub struct TextEdit {
    pub page: usize,
    pub uid: Option<Uid>,
    pub pos: Vec2,
    pub text: String,
    pub before: Option<TextItem>,
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub page: usize,
    pub uids: Vec<Uid>,
}

enum Gesture {
    None,
    Widget { idx: usize, id: u64 },
    Slider { idx: usize, id: u64 },
    PanelDrag { which: PanelKind, id: u64, grab: Vec2 },
    Draw { id: u64, page: usize, builder: InkBuilder, last_t: Instant, stroke: Stroke },
    Rule { id: u64, page: usize, tool: usize, edge: usize, start: Vec2, cur: Vec2 },
    Erase { id: u64, page: usize, before: Vec<Stroke>, last: Vec2, whole: bool },
    Pan { id: u64, last: Vec2 },
    Pinch { a: u64, b: u64, start_dist: f32, start_zoom: f32, last_mid: Vec2, tool: Option<(usize, f32, f32)> },
    Rubber { id: u64, page: usize, start: Vec2, cur: Vec2 },
    MoveSel { id: u64, page: usize, last: Vec2, before: Vec<Item>, moved: bool },
    Assist { id: u64, idx: usize, hit: AssistHit, last: Vec2 },
    Compass { id: u64, idx: usize, page: usize, tracker: ArcTracker },
}

struct PdfState {
    cache: TileCache,
    pages_pt: Vec<(f32, f32)>,
    last_plan: Option<(u64, u32, i32, i32, u32, u32)>,
    zoom_changed_at: Instant,
}

pub struct App {
    pub env: Env,
    pub settings: Settings,
    pub theme: Theme,
    pub doc: Document,
    pub history: History,
    pub layout: Layout,
    pub vp: Viewport,
    pub size: (u32, u32),
    pub scale: f32,
    pub page: usize,
    pub tool: Tool,
    pub pen_color: Color,
    pub pen_width: f32,
    pub pen_style: LineStyle,
    pub highlighter: bool,
    pub popup: Option<PopupKind>,
    pub dialog: Option<Dialog>,
    pub timer: Option<TimerPanel>,
    pub remote: Option<RemotePanel>,
    pub assist: AssistState,
    pub selection: Selection,
    clipboard: Vec<Item>,
    pub text_edit: Option<TextEdit>,
    toast: Option<(String, Instant)>,
    gesture: Gesture,
    touches: HashMap<u64, Vec2>,
    pdf_engine: Option<PdfEngine>,
    pdf: Option<PdfState>,
    pub updater: Option<cb_update::Updater>,
    saving: Option<Receiver<Result<PathBuf, String>>>,
    pub project_path: Option<PathBuf>,
    saved_version: u64,
    after_save: Option<AfterSave>,
    requests: Vec<Request>,
    pub board: BoardRenderer,
    pub fonts: Fonts,
    widgets: Vec<Widget>,
    widgets_dirty: bool,
    hover: Option<usize>,
    pressed: Option<usize>,
    redraw: bool,
    pub now: Instant,
    last_click: Option<(Instant, Vec2)>,
    pending_open: Option<PathBuf>,
    started_update_check: bool,
    #[allow(clippy::type_complexity)]
    bar_cache: Option<((Vec<Widget>, Option<usize>, Option<usize>, bool, Color, (u32, u32), u32), Pixmap)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AfterSave {
    Exit,
}

impl App {
    pub fn new(env: Env, width: u32, height: u32, scale: f32) -> App {
        let mut settings = env.settings_path.as_deref().map(Settings::load_from).unwrap_or_default();
        settings.sanitize();
        let dark = match settings.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => env.system_dark,
        };
        let theme = Theme::new(dark, settings.accent as usize);
        let fonts = Fonts::new(&[cb_render::UI_FONT]);
        let updater = env.data_dir.as_ref().filter(|_| !env.update_asset.is_empty()).map(|d| {
            let mut cfg = cb_update::Config::new(&env.update_asset, d.join("updates"));
            cfg.mirror = settings.update_mirror.clone();
            cb_update::Updater::new(cfg, env.waker.clone())
        });
        let mut app = App {
            pen_color: Color::from_argb(settings.pen_color),
            pen_width: settings.pen_width,
            env,
            theme,
            doc: Document::new(),
            history: History::new(),
            layout: Layout::default(),
            vp: Viewport { zoom: 1.0, ui_scale: scale, scroll: Vec2::ZERO, rect: Rect::new(0.0, 0.0, width as f32, height as f32) },
            size: (width, height),
            scale,
            page: 0,
            tool: Tool::Pen,
            pen_style: LineStyle::Solid,
            highlighter: false,
            popup: None,
            dialog: None,
            timer: None,
            remote: None,
            assist: AssistState::default(),
            selection: Selection::default(),
            clipboard: Vec::new(),
            text_edit: None,
            toast: None,
            gesture: Gesture::None,
            touches: HashMap::new(),
            pdf_engine: None,
            pdf: None,
            updater,
            saving: None,
            project_path: None,
            saved_version: 0,
            after_save: None,
            requests: Vec::new(),
            board: BoardRenderer::default(),
            fonts,
            widgets: Vec::new(),
            widgets_dirty: true,
            hover: None,
            pressed: None,
            redraw: true,
            now: Instant::now(),
            last_click: None,
            pending_open: None,
            started_update_check: false,
            bar_cache: None,
            settings,
        };
        app.resize(width, height, scale);
        app
    }

    // ------------------------------------------------------------ basics

    pub fn bar_height(&self) -> f32 {
        metrics::BAR_H * self.scale
    }

    fn board_rect(&self) -> Rect {
        Rect::new(0.0, 0.0, self.size.0 as f32, (self.size.1 as f32 - self.bar_height()).max(1.0))
    }

    pub fn resize(&mut self, w: u32, h: u32, scale: f32) {
        let old_center = self.vp.to_canvas(self.vp.rect.center());
        let scale_changed = (scale - self.scale).abs() > 1e-4;
        self.size = (w.max(1), h.max(1));
        self.scale = scale.max(0.5);
        self.vp.ui_scale = self.scale;
        self.vp.rect = self.board_rect();
        if scale_changed {
            // Keep the same canvas point centred when moving between monitors.
            let s = self.vp.to_screen(old_center);
            self.vp.scroll += s - self.vp.rect.center();
        }
        self.clamp_scroll();
        self.dirty();
    }

    pub fn dirty(&mut self) {
        self.redraw = true;
        self.widgets_dirty = true;
    }

    pub fn needs_redraw(&self) -> bool {
        self.redraw
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), self.now));
        self.dirty();
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.history.version != self.saved_version && self.doc.has_ink()
    }

    fn apply_theme(&mut self) {
        let dark = match self.settings.theme_mode {
            ThemeMode::Dark => true,
            ThemeMode::Light => false,
            ThemeMode::System => self.env.system_dark,
        };
        self.theme = Theme::new(dark, self.settings.accent as usize);
        self.board.invalidate();
        self.dirty();
    }

    pub fn set_system_dark(&mut self, dark: bool) {
        self.env.system_dark = dark;
        if self.settings.theme_mode == ThemeMode::System {
            self.apply_theme();
        }
    }

    pub fn save_settings(&mut self) {
        self.settings.pen_color = self.pen_color.to_argb();
        self.settings.pen_width = self.pen_width;
        if let Some(p) = &self.env.settings_path {
            if let Err(e) = self.settings.save_to(p) {
                log::warn!("settings save failed: {e}");
            }
        }
    }

    pub fn is_pdf(&self) -> bool {
        self.layout.is_strip()
    }

    pub fn page_count(&self) -> usize {
        self.doc.pages.len()
    }

    /// Page shown in the page indicator (PDF: page at viewport centre).
    pub fn display_page(&self) -> usize {
        if self.is_pdf() {
            self.layout.page_at(self.vp.to_canvas(self.vp.rect.center())).unwrap_or(0)
        } else {
            self.page
        }
    }

    fn clamp_scroll(&mut self) {
        if !self.is_pdf() {
            return;
        }
        let k = self.vp.k();
        let t = self.layout.total;
        let (bw, bh) = (self.vp.rect.w(), self.vp.rect.h());
        let cw = t.w() * k;
        if cw <= bw {
            self.vp.scroll.x = t.x0 * k - (bw - cw) * 0.5;
        } else {
            self.vp.scroll.x = self.vp.scroll.x.clamp(t.x0 * k, t.x1 * k - bw);
        }
        let ch = t.h() * k;
        if ch <= bh {
            self.vp.scroll.y = -(bh - ch) * 0.5;
        } else {
            self.vp.scroll.y = self.vp.scroll.y.clamp(t.y0 * k, t.y1 * k - bh);
        }
    }

    // ------------------------------------------------------------ events

    pub fn handle(&mut self, ev: Event) {
        self.now = Instant::now();
        match ev {
            Event::Resize { width, height, scale } => self.resize(width, height, scale),
            Event::PointerDown(p) => self.pointer_down(p),
            Event::PointerMove(p) => self.pointer_move(p),
            Event::PointerUp(p) => self.pointer_up(p),
            Event::PointerCancel(id) => self.pointer_cancel(id),
            Event::PointerLeave => {
                if self.hover.take().is_some() {
                    self.redraw = true;
                }
            }
            Event::Wheel { x, y, dx, dy, lines, mods } => self.wheel(v2(x, y), dx, dy, lines, mods),
            Event::Magnify { x, y, delta } => {
                let z = self.vp.zoom * (1.0 + delta);
                self.zoom_to(z, v2(x, y));
            }
            Event::Key { key, mods } => self.key(key, mods),
            Event::Text(t) => self.text_input(&t),
            Event::OpenPath(p) => self.open_path_checked(p),
            Event::SavePath(p) => self.save_to(p),
            Event::Hotspot(r) => self.on_hotspot(r),
            Event::Wake => {}
        }
        self.poll();
    }

    /// Window close button / Alt+F4.
    pub fn request_close(&mut self) {
        if self.has_unsaved_changes() {
            self.dialog = Some(Dialog::Confirm {
                title: "退出".into(),
                message: "退出前是否保存当前白板？".into(),
                buttons: vec![
                    (DLG_EXIT_SAVE, "保存并退出".into(), widgets::BtnStyle::Primary),
                    (DLG_EXIT_DISCARD, "不保存退出".into(), widgets::BtnStyle::Danger),
                    (DLG_CANCEL, "取消".into(), widgets::BtnStyle::Ghost),
                ],
            });
            self.dirty();
        } else {
            self.shutdown();
            self.requests.push(Request::Exit);
        }
    }

    fn shutdown(&mut self) {
        self.save_settings();
        self.remote = None;
    }

    fn widgets(&mut self) -> &[Widget] {
        if self.widgets_dirty {
            self.widgets = self.build_widgets();
            self.widgets_dirty = false;
        }
        &self.widgets
    }

    fn pointer_down(&mut self, p: Pointer) {
        let pos = v2(p.x, p.y);
        if p.kind == PointerKind::Touch {
            self.touches.insert(p.id, pos);
            if self.touches.len() == 2 && !matches!(self.gesture, Gesture::Widget { .. } | Gesture::Slider { .. } | Gesture::PanelDrag { .. }) {
                self.begin_pinch();
                return;
            }
            if self.touches.len() > 2 {
                return;
            }
        }
        if !matches!(self.gesture, Gesture::None) {
            return;
        }
        // Chrome first.
        let (hit, blocked) = {
            let w = self.widgets();
            widgets::hit(w, p.x, p.y)
        };
        if let Some(i) = hit {
            let w = &self.widgets[i];
            match (&w.kind, &w.action) {
                (widgets::WKind::Slider { .. }, _) => {
                    self.gesture = Gesture::Slider { idx: i, id: p.id };
                    self.slider_to(i, p.x);
                }
                (_, Action::PanelDrag(which)) => {
                    let which = *which;
                    let origin = self.panel_pos(which);
                    self.gesture = Gesture::PanelDrag { which, id: p.id, grab: pos - origin };
                }
                _ => {
                    self.pressed = Some(i);
                    self.gesture = Gesture::Widget { idx: i, id: p.id };
                }
            }
            self.redraw = true;
            return;
        }
        if blocked {
            return;
        }
        if self.dialog.is_some() {
            return;
        }
        // Clicking outside an open popup closes it (and consumes the click).
        if self.popup.is_some() {
            self.popup = None;
            self.dirty();
            if self.text_edit.is_none() {
                return;
            }
        }
        if !self.vp.rect.contains(pos) {
            return;
        }
        self.commit_text_edit();
        let c = self.vp.to_canvas(pos);
        // Secondary/middle button pans.
        if p.kind == PointerKind::Mouse && matches!(p.button, Button::Middle) {
            self.gesture = Gesture::Pan { id: p.id, last: pos };
            return;
        }
        if p.kind == PointerKind::Mouse && p.button == Button::Secondary {
            if !self.selection.uids.is_empty() {
                self.popup = Some(PopupKind::Selection);
                self.dirty();
            } else {
                self.gesture = Gesture::Pan { id: p.id, last: pos };
            }
            return;
        }
        // Teaching aids.
        let is_pen_tool = self.tool == Tool::Pen && !p.eraser;
        let tol = 14.0 * self.scale / self.vp.k();
        if let Some((idx, h)) = self.assist.hit(c, tol, is_pen_tool) {
            let idx = self.assist.bring_to_front(idx);
            match h {
                AssistHit::Close => {
                    self.assist.tools.remove(idx);
                    self.dirty();
                }
                AssistHit::Edge(e) if is_pen_tool => {
                    let page = self.page_for_canvas(c);
                    self.gesture = Gesture::Rule { id: p.id, page, tool: idx, edge: e, start: c, cur: c };
                }
                AssistHit::CompassHandle => {
                    let t = &self.assist.tools[idx];
                    let page = self.page_for_canvas(t.compass_needle());
                    self.gesture = Gesture::Compass { id: p.id, idx, page, tracker: ArcTracker::new(t.angle) };
                }
                h => {
                    self.gesture = Gesture::Assist { id: p.id, idx, hit: h, last: c };
                }
            }
            self.redraw = true;
            return;
        }
        let tool = if p.eraser { Tool::Eraser } else { self.tool };
        match tool {
            Tool::Pen => self.begin_draw(p, c),
            Tool::Eraser => {
                let page = self.page_for_canvas(c);
                let before = self.doc.pages[page].strokes.clone();
                self.gesture = Gesture::Erase { id: p.id, page, before, last: c, whole: !self.settings.eraser_split };
                self.erase_move(c);
            }
            Tool::Pan => self.gesture = Gesture::Pan { id: p.id, last: pos },
            Tool::Select => self.begin_select(p.id, c),
            Tool::Text => self.begin_text(c),
        }
    }

    fn pointer_move(&mut self, p: Pointer) {
        let pos = v2(p.x, p.y);
        if p.kind == PointerKind::Touch {
            if let Some(t) = self.touches.get_mut(&p.id) {
                *t = pos;
            }
        }
        if p.kind == PointerKind::Mouse && matches!(self.gesture, Gesture::None) {
            let h = {
                let w = self.widgets();
                widgets::hit(w, p.x, p.y).0
            };
            if h != self.hover {
                self.hover = h;
                self.redraw = true;
            }
            let cur = if h.is_some() {
                CursorIcon::Pointer
            } else if self.vp.rect.contains(pos) {
                match self.tool {
                    Tool::Pan => CursorIcon::Grab,
                    Tool::Text => CursorIcon::Text,
                    Tool::Select => CursorIcon::Default,
                    _ => CursorIcon::Crosshair,
                }
            } else {
                CursorIcon::Default
            };
            self.requests.push(Request::SetCursor(cur));
            return;
        }
        let c = self.vp.to_canvas(pos);
        let now = self.now;
        match &mut self.gesture {
            Gesture::None => {}
            Gesture::Widget { idx, id } => {
                if *id == p.id {
                    let inside = self.widgets.get(*idx).map(|w| w.rect.contains(pos)).unwrap_or(false);
                    let np = if inside { Some(*idx) } else { None };
                    if np != self.pressed {
                        self.pressed = np;
                        self.redraw = true;
                    }
                }
            }
            Gesture::Slider { idx, id } => {
                if *id == p.id {
                    let i = *idx;
                    self.slider_to(i, p.x);
                }
            }
            Gesture::PanelDrag { which, id, grab } => {
                if *id == p.id {
                    let (w, g) = (*which, *grab);
                    self.set_panel_pos(w, pos - g);
                }
            }
            Gesture::Draw { id, page, builder, last_t, stroke } => {
                if *id != p.id {
                    return;
                }
                let origin = self.layout.origin(*page);
                let lc = c - origin;
                let dt = now.saturating_duration_since(*last_t).as_millis().min(u16::MAX as u128) as u16;
                *last_t = now;
                let pr = if p.kind == PointerKind::Pen && self.settings.pressure { p.pressure } else { 1.0 };
                if builder.push(lc.x, lc.y, pr, dt) {
                    stroke.points = builder.points.clone();
                    self.redraw = true;
                }
            }
            Gesture::Rule { id, cur, .. } => {
                if *id == p.id {
                    *cur = c;
                    self.redraw = true;
                }
            }
            Gesture::Erase { id, .. } => {
                if *id == p.id {
                    self.erase_move(c);
                }
            }
            Gesture::Pan { id, last } => {
                if *id == p.id {
                    let d = pos - *last;
                    *last = pos;
                    self.vp.scroll -= d;
                    self.clamp_scroll();
                    self.redraw = true;
                }
            }
            Gesture::Pinch { .. } => self.update_pinch(),
            Gesture::Rubber { id, cur, .. } => {
                if *id == p.id {
                    *cur = c;
                    self.redraw = true;
                }
            }
            Gesture::MoveSel { id, page, last, moved, .. } => {
                if *id == p.id {
                    let d = c - *last;
                    *last = c;
                    *moved = true;
                    let pg = *page;
                    for u in self.selection.uids.clone() {
                        if let Some(mut it) = self.doc.get_item(pg, u) {
                            it.translate(d);
                            self.doc.replace_item(pg, it);
                        }
                    }
                    self.doc.touch();
                    self.dirty();
                }
            }
            Gesture::Assist { id, idx, hit, last } => {
                if *id != p.id {
                    return;
                }
                let (i, h, l) = (*idx, *hit, *last);
                *last = c;
                let t = &mut self.assist.tools[i];
                match h {
                    AssistHit::Rotate => {
                        let pv = t.pivot();
                        let a0 = (l - pv).angle();
                        let a1 = (c - pv).angle();
                        let mut da = cb_core::geom::normalize_angle_delta(a1 - a0);
                        let target = t.angle + da;
                        // Snap to 15° when close.
                        let step = 15f32.to_radians();
                        let snapped = (target / step).round() * step;
                        if (snapped - target).abs() < 1.5f32.to_radians() {
                            da = snapped - t.angle;
                        }
                        t.rotate_about(pv, da);
                    }
                    AssistHit::CompassPencil => t.drag_pencil_to(c),
                    AssistHit::ProtractorPointer => t.set_reading_towards(c),
                    _ => t.translate(c - l),
                }
                self.redraw = true;
            }
            Gesture::Compass { id, idx, page, tracker } => {
                if *id != p.id {
                    return;
                }
                let t = &mut self.assist.tools[*idx];
                let a = (c - t.pos).angle();
                // The handle rotates the whole compass around the needle.
                let before = tracker.sweep;
                tracker.update(a);
                let d = tracker.sweep - before;
                t.angle += d;
                let _ = page;
                self.redraw = true;
            }
        }
    }

    fn pointer_up(&mut self, p: Pointer) {
        if p.kind == PointerKind::Touch {
            self.touches.remove(&p.id);
        }
        let pos = v2(p.x, p.y);
        let g = std::mem::replace(&mut self.gesture, Gesture::None);
        match g {
            Gesture::Pinch { a, b, .. } => {
                if p.id != a && p.id != b {
                    self.gesture = g;
                    return;
                }
                if let Some(pdf) = &mut self.pdf {
                    pdf.zoom_changed_at = self.now;
                }
                self.redraw = true;
                // Remaining finger does nothing until lifted.
                return;
            }
            Gesture::Widget { idx, id } => {
                if id != p.id {
                    self.gesture = Gesture::Widget { idx, id };
                    return;
                }
                self.pressed = None;
                let act = self.widgets.get(idx).filter(|w| w.rect.contains(pos)).map(|w| w.action.clone());
                if let Some(a) = act {
                    self.perform(a);
                }
                self.dirty();
            }
            Gesture::Slider { .. } | Gesture::PanelDrag { .. } => {
                self.save_settings();
                self.dirty();
            }
            Gesture::Draw { page, mut stroke, builder, .. } => {
                stroke.points = builder.points;
                self.commit_stroke(page, stroke);
            }
            Gesture::Rule { page, tool, edge, start, cur, .. } => {
                let t = self.assist.tools[tool];
                if let Some((a, b)) = t.rule(edge, start, cur, self.pen_width * 0.5) {
                    if a.dist(b) > 2.0 {
                        let o = self.layout.origin(page);
                        let mut s = self.new_stroke();
                        s.points = vec![InkPoint::new(a.x - o.x, a.y - o.y), InkPoint::new(b.x - o.x, b.y - o.y)];
                        self.commit_stroke_raw(page, s);
                    }
                }
                self.redraw = true;
            }
            Gesture::Erase { page, before, .. } => {
                if let Some(cmd) = erase::diff_strokes(page, &before, &self.doc.pages[page].strokes) {
                    self.history.record(cmd);
                }
                self.dirty();
            }
            Gesture::Rubber { page, start, cur, .. } => {
                let o = self.layout.origin(page);
                let r = Rect::from_points(start - o, cur - o);
                self.select_in_rect(page, r);
            }
            Gesture::MoveSel { page, before, moved, .. } => {
                if moved {
                    let after: Vec<Item> = before.iter().filter_map(|b| self.doc.get_item(page, b.uid())).collect();
                    self.history.record(Command::Modify { page, before, after });
                }
                self.dirty();
            }
            Gesture::Assist { .. } => self.redraw = true,
            Gesture::Compass { idx, page, tracker, .. } => {
                let t = self.assist.tools[idx];
                if tracker.sweep.abs() > 0.02 {
                    let o = self.layout.origin(page);
                    let pts = arc_points(t.pos - o, t.radius, tracker.start, tracker.sweep);
                    let mut s = self.new_stroke();
                    s.points = pts.into_iter().map(|q| InkPoint::new(q.x, q.y)).collect();
                    self.commit_stroke_raw(page, s);
                }
                self.redraw = true;
            }
            Gesture::Pan { .. } | Gesture::None => {}
        }
    }

    fn pointer_cancel(&mut self, id: u64) {
        self.touches.remove(&id);
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Erase { page, before, .. } => {
                if let Some(cmd) = erase::diff_strokes(page, &before, &self.doc.pages[page].strokes) {
                    self.history.record(cmd);
                }
            }
            Gesture::MoveSel { page, before, moved: true, .. } => {
                let after: Vec<Item> = before.iter().filter_map(|b| self.doc.get_item(page, b.uid())).collect();
                self.history.record(Command::Modify { page, before, after });
            }
            _ => {}
        }
        self.pressed = None;
        self.dirty();
    }

    fn begin_pinch(&mut self) {
        // Cancel whatever single-finger gesture was running (without committing ink).
        match std::mem::replace(&mut self.gesture, Gesture::None) {
            Gesture::Erase { page, before, .. } => {
                self.doc.pages[page].strokes = before;
                self.doc.touch();
            }
            Gesture::MoveSel { page, before, moved: true, .. } => {
                for it in before {
                    self.doc.replace_item(page, it);
                }
                self.doc.touch();
            }
            _ => {}
        }
        let ids: Vec<u64> = self.touches.keys().copied().take(2).collect();
        let (pa, pb) = (self.touches[&ids[0]], self.touches[&ids[1]]);
        let mid = pa.lerp(pb, 0.5);
        // Two fingers on a teaching aid rotate/scale the aid instead of the board.
        let tool = self.assist.hit(self.vp.to_canvas(mid), 0.0, false).map(|(i, _)| (i, (pb - pa).angle(), pa.dist(pb)));
        self.gesture = Gesture::Pinch { a: ids[0], b: ids[1], start_dist: pa.dist(pb).max(1.0), start_zoom: self.vp.zoom, last_mid: mid, tool };
        self.redraw = true;
    }

    fn update_pinch(&mut self) {
        let Gesture::Pinch { a, b, start_dist, start_zoom, last_mid, tool } = &mut self.gesture else { return };
        let (Some(pa), Some(pb)) = (self.touches.get(a).copied(), self.touches.get(b).copied()) else { return };
        let mid = pa.lerp(pb, 0.5);
        if let Some((i, ang0, d0)) = tool {
            let t = &mut self.assist.tools[*i];
            let ang = (pb - pa).angle();
            let d = pa.dist(pb).max(1.0);
            let k = self.vp.k();
            let pv = t.pivot();
            t.rotate_about(pv, cb_core::geom::normalize_angle_delta(ang - *ang0));
            t.scale_about(pv, d / *d0);
            t.translate((mid - *last_mid) * (1.0 / k));
            *ang0 = ang;
            *d0 = d;
            *last_mid = mid;
            self.redraw = true;
            return;
        }
        let z = *start_zoom * pa.dist(pb) / *start_dist;
        let pan = mid - *last_mid;
        *last_mid = mid;
        self.vp.scroll -= pan;
        self.vp.zoom_about(z, mid);
        if let Some(pdf) = &mut self.pdf {
            pdf.zoom_changed_at = self.now;
        }
        self.clamp_scroll();
        self.dirty();
    }

    fn wheel(&mut self, at: Vec2, dx: f32, dy: f32, lines: bool, mods: Mods) {
        if self.dialog.is_some() {
            return;
        }
        let mul = if lines { 48.0 * self.scale } else { 1.0 };
        if mods.cmd {
            let f = (1.0f32 + 0.0015 * dy * mul).clamp(0.5, 2.0);
            self.zoom_to(self.vp.zoom * f, at);
            return;
        }
        if !self.is_pdf() && self.tool != Tool::Pan && lines {
            // Legacy behaviour: wheel flips pages on blank boards.
            if dy > 0.0 {
                self.perform(Action::PrevPage)
            } else if dy < 0.0 {
                self.perform(Action::NextPage)
            }
            return;
        }
        let (dx, dy) = if mods.shift { (dy, dx) } else { (dx, dy) };
        self.vp.scroll -= v2(dx * mul, dy * mul);
        self.clamp_scroll();
        self.dirty();
    }

    pub fn zoom_to(&mut self, z: f32, at: Vec2) {
        self.vp.zoom_about(z.clamp(ZOOM_MIN, ZOOM_MAX), at);
        if let Some(pdf) = &mut self.pdf {
            pdf.zoom_changed_at = self.now;
        }
        self.clamp_scroll();
        self.dirty();
    }

    fn key(&mut self, key: Key, mods: Mods) {
        if self.text_edit.is_some() {
            match key {
                Key::Escape => self.commit_text_edit(),
                Key::Enter if !mods.shift => self.commit_text_edit(),
                Key::Enter => self.text_input("\n"),
                Key::Backspace => {
                    if let Some(t) = &mut self.text_edit {
                        t.text.pop();
                    }
                    self.redraw = true;
                }
                _ => {}
            }
            return;
        }
        if let Key::Escape = key {
            if self.dialog.is_some() {
                self.dialog = None;
            } else if self.popup.is_some() {
                self.popup = None;
            } else if self.timer.as_ref().is_some_and(|t| t.fullscreen) {
                if let Some(t) = &mut self.timer {
                    t.fullscreen = false;
                }
            } else {
                self.selection.uids.clear();
            }
            self.dirty();
            return;
        }
        if self.dialog.is_some() {
            return;
        }
        match (key, mods.cmd) {
            (Key::Char('z'), true) if mods.shift => self.perform(Action::Redo),
            (Key::Char('z'), true) => self.perform(Action::Undo),
            (Key::Char('y'), true) => self.perform(Action::Redo),
            (Key::Char('s'), true) => self.perform(Action::Save),
            (Key::Char('o'), true) => self.perform(Action::Open),
            (Key::Char('c'), true) => self.perform(Action::SelCopy),
            (Key::Char('v'), true) => self.perform(Action::Paste),
            (Key::Char('a'), true) => {
                let pg = self.page;
                self.select_in_rect(pg, Rect::new(-1e9, -1e9, 1e9, 1e9));
            }
            (Key::Char('0'), true) => self.perform(Action::SetZoom(1.0)),
            (Key::Char('='), true) | (Key::Char('+'), true) => self.zoom_to(self.vp.zoom * 1.25, self.vp.rect.center()),
            (Key::Char('-'), true) => self.zoom_to(self.vp.zoom / 1.25, self.vp.rect.center()),
            (Key::Delete, _) | (Key::Backspace, _) => self.perform(Action::SelDelete),
            (Key::Left, _) | (Key::PageUp, _) => self.perform(Action::PrevPage),
            (Key::Right, _) | (Key::PageDown, _) => self.perform(Action::NextPage),
            (Key::Up, _) if self.is_pdf() => {
                self.vp.scroll.y -= 120.0 * self.scale;
                self.clamp_scroll();
                self.dirty();
            }
            (Key::Down, _) if self.is_pdf() => {
                self.vp.scroll.y += 120.0 * self.scale;
                self.clamp_scroll();
                self.dirty();
            }
            (Key::F11, _) => self.requests.push(Request::ToggleFullscreen),
            (Key::Char('1'), false) => self.perform(Action::SetTool(Tool::Pan)),
            (Key::Char('2'), false) => self.perform(Action::SetTool(Tool::Pen)),
            (Key::Char('3'), false) => self.perform(Action::SetTool(Tool::Eraser)),
            (Key::Char('4'), false) => self.perform(Action::SetTool(Tool::Select)),
            (Key::Char('t'), false) => self.perform(Action::SetTool(Tool::Text)),
            _ => {}
        }
    }

    fn text_input(&mut self, s: &str) {
        if let Some(t) = &mut self.text_edit {
            for ch in s.chars() {
                if ch == '\u{8}' {
                    t.text.pop();
                } else if ch == '\r' {
                    continue;
                } else if !ch.is_control() || ch == '\n' {
                    t.text.push(ch);
                }
            }
            self.redraw = true;
        }
    }

    // ------------------------------------------------------------ drawing

    fn page_for_canvas(&self, c: Vec2) -> usize {
        if self.is_pdf() {
            self.layout.page_at(c).unwrap_or(0)
        } else {
            self.page
        }
    }

    fn new_stroke(&mut self) -> Stroke {
        let mut s = Stroke::new(0, self.pen_color, self.pen_width);
        s.style = self.pen_style;
        if self.highlighter {
            s.highlighter = true;
            s.width = (self.pen_width * 3.0).max(12.0);
            s.style = LineStyle::Solid;
        }
        s
    }

    fn begin_draw(&mut self, p: Pointer, c: Vec2) {
        let page = self.page_for_canvas(c);
        let o = self.layout.origin(page);
        let mut builder = InkBuilder::new(self.vp.k());
        let pr = if p.kind == PointerKind::Pen && self.settings.pressure { p.pressure } else { 1.0 };
        builder.push(c.x - o.x, c.y - o.y, pr, 0);
        let mut stroke = self.new_stroke();
        stroke.points = builder.points.clone();
        self.selection.uids.clear();
        self.gesture = Gesture::Draw { id: p.id, page, builder, last_t: self.now, stroke };
        self.redraw = true;
    }

    fn commit_stroke(&mut self, page: usize, mut stroke: Stroke) {
        if stroke.points.is_empty() {
            return;
        }
        if self.settings.shape_recognition && !stroke.highlighter {
            if let Some((kind, a, b)) = cb_core::shape::recognize(&stroke.points) {
                stroke.shape = Some(kind);
                stroke.a = a;
                stroke.b = b;
                stroke.points = vec![InkPoint::new(a.x, a.y), InkPoint::new(b.x, b.y)];
            }
        }
        let merge = !matches!(self.history_last_page(), Some(p) if p != page);
        stroke.uid = self.doc.alloc_uid();
        let idx = self.doc.pages[page].strokes.len();
        self.doc.pages[page].strokes.push(stroke.clone());
        self.doc.touch();
        // Strokes drawn within 450 ms of each other undo together (legacy behaviour).
        let quick = self.last_click.map(|(t, _)| self.now.duration_since(t) < Duration::from_millis(450)).unwrap_or(false);
        self.history.record_stroke(page, idx, Item::Stroke(stroke), merge && quick);
        self.last_click = Some((self.now, Vec2::ZERO));
        self.dirty();
    }

    fn history_last_page(&self) -> Option<usize> {
        None
    }

    fn commit_stroke_raw(&mut self, page: usize, mut stroke: Stroke) {
        stroke.uid = self.doc.alloc_uid();
        let idx = self.doc.pages[page].strokes.len();
        self.history.execute(&mut self.doc, Command::Add { page, items: vec![(idx, Item::Stroke(stroke))] });
        self.dirty();
    }

    fn erase_move(&mut self, c: Vec2) {
        let Gesture::Erase { page, last, whole, .. } = &mut self.gesture else { return };
        let (pg, from, whole) = (*page, *last, *whole);
        *last = c;
        let o = self.layout.origin(pg);
        let r = self.settings.eraser_size * 0.5 / self.vp.zoom;
        let changed = if whole {
            erase::erase_whole_at(&mut self.doc, pg, c - o, r)
        } else {
            erase::erase_along(&mut self.doc, pg, from - o, c - o, r)
        };
        if changed {
            self.dirty();
        }
        self.redraw = true;
    }

    fn begin_select(&mut self, id: u64, c: Vec2) {
        let page = self.page_for_canvas(c);
        let o = self.layout.origin(page);
        let lc = c - o;
        // Inside current selection → move it.
        if self.selection.page == page && !self.selection.uids.is_empty() {
            let bb = self.selection_bounds();
            if bb.inflate(6.0 / self.vp.k()).contains(lc) {
                let before: Vec<Item> = self.selection.uids.iter().filter_map(|u| self.doc.get_item(page, *u)).collect();
                self.popup = None;
                self.gesture = Gesture::MoveSel { id, page, last: c, before, moved: false };
                return;
            }
        }
        // Tap on an item selects it.
        if let Some(uid) = self.item_at(page, lc) {
            self.selection = Selection { page, uids: vec![uid] };
            let before: Vec<Item> = self.doc.get_item(page, uid).into_iter().collect();
            self.gesture = Gesture::MoveSel { id, page, last: c, before, moved: false };
            self.dirty();
            return;
        }
        self.selection = Selection { page, uids: vec![] };
        self.popup = None;
        self.gesture = Gesture::Rubber { id, page, start: c, cur: c };
        self.dirty();
    }

    fn item_at(&self, page: usize, p: Vec2) -> Option<Uid> {
        let pg = &self.doc.pages[page];
        if let Some(t) = pg.texts.iter().rev().find(|t| self.text_bounds(t).contains(p)) {
            return Some(t.uid);
        }
        if let Some(i) = pg.images.iter().rev().find(|i| i.bounds().contains(p)) {
            return Some(i.uid);
        }
        let tol = 8.0 / self.vp.k();
        pg.strokes
            .iter()
            .rev()
            .find(|s| {
                s.bounds().inflate(tol).contains(p) && {
                    let o = s.outline();
                    o.len() == 1 && o[0].dist(p) < tol + s.width || o.windows(2).any(|w| cb_core::geom::dist_to_segment(p, w[0], w[1]).0 < tol + s.width * 0.5)
                }
            })
            .map(|s| s.uid)
    }

    pub fn text_bounds(&self, t: &TextItem) -> Rect {
        // Approximate: renderer metrics would need &mut Fonts.
        t.approx_bounds()
    }

    fn select_in_rect(&mut self, page: usize, r: Rect) {
        let pg = &self.doc.pages[page];
        let mut uids = Vec::new();
        uids.extend(pg.strokes.iter().filter(|s| r.intersects(&s.bounds()) && s.outline().iter().any(|p| r.contains(*p))).map(|s| s.uid));
        uids.extend(pg.texts.iter().filter(|t| r.intersects(&t.approx_bounds())).map(|t| t.uid));
        uids.extend(pg.images.iter().filter(|i| r.intersects(&i.bounds())).map(|i| i.uid));
        self.selection = Selection { page, uids };
        if !self.selection.uids.is_empty() {
            self.popup = Some(PopupKind::Selection);
        }
        self.dirty();
    }

    pub fn selection_bounds(&self) -> Rect {
        let mut r = Rect::EMPTY;
        for u in &self.selection.uids {
            if let Some(it) = self.doc.get_item(self.selection.page, *u) {
                r = r.union(&it.bounds());
            }
        }
        r
    }

    fn begin_text(&mut self, c: Vec2) {
        let page = self.page_for_canvas(c);
        let o = self.layout.origin(page);
        let lc = c - o;
        // Edit an existing text item if tapped.
        if let Some(t) = self.doc.pages[page].texts.iter().rev().find(|t| t.approx_bounds().contains(lc)).cloned() {
            self.text_edit = Some(TextEdit { page, uid: Some(t.uid), pos: t.pos, text: t.text.clone(), before: Some(t) });
        } else {
            let size = (self.pen_width * 6.0).clamp(20.0, 96.0);
            self.text_edit = Some(TextEdit { page, uid: None, pos: lc - v2(0.0, size * 0.6), text: String::new(), before: None });
        }
        let s = self.vp.to_screen(c);
        self.requests.push(Request::Ime { enabled: true, x: s.x, y: s.y });
        self.dirty();
    }

    pub fn commit_text_edit(&mut self) {
        let Some(te) = self.text_edit.take() else { return };
        self.requests.push(Request::Ime { enabled: false, x: 0.0, y: 0.0 });
        let text = te.text.trim_end().to_string();
        match te.before {
            Some(before) => {
                if text.is_empty() {
                    let cmd = remove_command(&self.doc, te.page, &[before.uid]);
                    self.history.execute(&mut self.doc, cmd);
                } else if text != before.text {
                    let after = TextItem { text, ..before.clone() };
                    self.history.execute(&mut self.doc, Command::Modify { page: te.page, before: vec![Item::Text(before)], after: vec![Item::Text(after)] });
                }
            }
            None => {
                if !text.is_empty() {
                    let size = (self.pen_width * 6.0).clamp(20.0, 96.0);
                    let item = TextItem { uid: self.doc.alloc_uid(), text, pos: te.pos, color: self.pen_color, font_size: size, family: String::new(), bold: false };
                    let idx = self.doc.pages[te.page].texts.len();
                    self.history.execute(&mut self.doc, Command::Add { page: te.page, items: vec![(idx, Item::Text(item))] });
                }
            }
        }
        self.dirty();
    }

    // ------------------------------------------------------------ actions

    pub fn perform(&mut self, a: Action) {
        self.widgets_dirty = true;
        self.redraw = true;
        match a {
            Action::None | Action::PanelDrag(_) => {}
            Action::Exit => self.request_close(),
            Action::Minimize => self.requests.push(Request::Minimize),
            Action::Recenter => {
                self.vp.zoom = 1.0;
                if self.is_pdf() {
                    let pg = self.display_page();
                    self.fit_width();
                    let k = self.vp.k();
                    self.vp.scroll.y = self.layout.pages[pg].y0 * k - 12.0 * self.scale;
                } else {
                    self.vp.scroll = Vec2::ZERO;
                }
                self.clamp_scroll();
                self.toast("已回到中心");
            }
            Action::ZoomFit => {
                self.fit_width();
                self.popup = None;
            }
            Action::OpenSettings => {
                self.popup = None;
                self.dialog = Some(Dialog::Settings);
            }
            Action::Open => {
                self.popup = None;
                self.requests.push(Request::OpenFileDialog);
            }
            Action::Save => {
                self.popup = None;
                self.commit_text_edit();
                match self.project_path.clone() {
                    Some(p) => self.save_to(p),
                    None => self.requests.push(Request::SaveFileDialog { default_name: self.default_save_name() }),
                }
            }
            Action::TogglePopup(k) => {
                self.popup = if self.popup == Some(k) { None } else { Some(k) };
                if matches!(k, PopupKind::Pen) {
                    if self.tool != Tool::Pen && self.popup.is_some() {
                        self.tool = Tool::Pen;
                    }
                } else if matches!(k, PopupKind::Eraser) && self.popup.is_some() {
                    self.tool = Tool::Eraser;
                }
            }
            Action::SetTool(t) => {
                if self.tool == t && matches!(t, Tool::Pen | Tool::Eraser) {
                    let k = if t == Tool::Pen { PopupKind::Pen } else { PopupKind::Eraser };
                    self.popup = if self.popup == Some(k) { None } else { Some(k) };
                } else {
                    self.popup = None;
                }
                if t != Tool::Select {
                    self.selection.uids.clear();
                }
                if t != Tool::Text {
                    self.commit_text_edit();
                }
                self.tool = t;
            }
            Action::PenColor(i) => {
                self.pen_color = PEN_COLORS[i.min(PEN_COLORS.len() - 1)];
                self.tool = Tool::Pen;
                self.save_settings();
            }
            Action::PenWidth(i) => {
                self.pen_width = PEN_WIDTHS[i.min(PEN_WIDTHS.len() - 1)];
                self.save_settings();
            }
            Action::PenStyle(s) => {
                self.pen_style = s;
                self.highlighter = false;
            }
            Action::ToggleHighlighter => self.highlighter = !self.highlighter,
            Action::ToggleShapeRecognition => {
                self.settings.shape_recognition = !self.settings.shape_recognition;
                self.save_settings();
                self.toast(if self.settings.shape_recognition { "图形识别已开启" } else { "图形识别已关闭" });
            }
            Action::EraserSize => {}
            Action::EraserMode(split) => {
                self.settings.eraser_split = split;
                self.save_settings();
            }
            Action::ClearPage => {
                self.popup = None;
                let p = self.current_edit_page();
                if !self.doc.pages[p].is_empty() {
                    let page = self.doc.pages[p].clone();
                    self.history.execute(&mut self.doc, Command::ClearPage { index: p, page });
                    self.toast("已清空本页（可撤销）");
                }
            }
            Action::Undo => {
                self.commit_text_edit();
                if let Some(p) = self.history.undo(&mut self.doc) {
                    self.after_history(p);
                }
            }
            Action::Redo => {
                if let Some(p) = self.history.redo(&mut self.doc) {
                    self.after_history(p);
                }
            }
            Action::PrevPage => self.goto_page(self.display_page().saturating_sub(1)),
            Action::NextPage => {
                let cur = self.display_page();
                if cur + 1 >= self.page_count() && !self.is_pdf() {
                    self.perform(Action::AddPage);
                } else {
                    self.goto_page(cur + 1);
                }
            }
            Action::AddPage => {
                if self.is_pdf() {
                    return;
                }
                if self.page_count() >= cb_core::doc::MAX_PAGES {
                    self.toast(format!("最多 {} 页", cb_core::doc::MAX_PAGES));
                    return;
                }
                let idx = self.page + 1;
                self.history.execute(&mut self.doc, Command::InsertPage { index: idx, page: Page::default() });
                self.goto_page(idx);
                self.toast(format!("已新建第 {} 页", idx + 1));
            }
            Action::DeletePage => {
                if self.is_pdf() {
                    return;
                }
                self.dialog = Some(Dialog::Confirm {
                    title: "删除页面".into(),
                    message: format!("确定删除第 {} 页？", self.page + 1),
                    buttons: vec![(DLG_DELETE_PAGE, "删除".into(), widgets::BtnStyle::Danger), (DLG_CANCEL, "取消".into(), widgets::BtnStyle::Ghost)],
                });
            }
            Action::SetZoom(z) => {
                let c = self.vp.rect.center();
                self.zoom_to(z, c);
                self.popup = None;
                self.toast(format!("缩放 {:.0}%", self.vp.zoom * 100.0));
            }
            Action::ToggleAssist(k) => {
                let c = self.vp.to_canvas(self.vp.rect.center());
                self.assist.toggle(k, c);
                if k == AssistKind::Compass {
                    if let Some(t) = self.assist.tools.iter_mut().find(|t| t.kind == AssistKind::Compass) {
                        t.angle = 0.0;
                    }
                }
            }
            Action::HideAssists => {
                self.assist.tools.clear();
                self.popup = None;
            }
            Action::ToggleTimer => {
                self.popup = None;
                if self.timer.is_some() {
                    self.timer = None;
                } else {
                    let s = self.scale;
                    let pos = v2(self.size.0 as f32 - 380.0 * s, 24.0 * s);
                    let mins = self.settings.timer_last_minutes.max(1);
                    self.timer = Some(TimerPanel {
                        pos,
                        tab: TimerTab::Countdown,
                        countdown: Countdown::new(Duration::from_secs(mins as u64 * 60)),
                        stopwatch: Stopwatch::default(),
                        alarm_since: None,
                        last_alarm_sound: None,
                        fullscreen: false,
                    });
                }
            }
            Action::ToggleRemote => {
                self.popup = None;
                if self.remote.is_some() {
                    self.stop_remote();
                } else {
                    self.start_remote();
                }
            }
            Action::RemoteStop | Action::ClosePanel(PanelKind::Remote) => self.stop_remote(),
            Action::ClosePanel(PanelKind::Timer) => self.timer = None,
            Action::RemoteRetryHotspot => {
                if let Some(r) = &mut self.remote {
                    r.hotspot = HotspotUi::Starting;
                    self.requests.push(Request::StartHotspot);
                }
            }
            Action::SelCopy => {
                let pg = self.selection.page;
                self.clipboard = self.selection.uids.iter().filter_map(|u| self.doc.get_item(pg, *u)).collect();
                if !self.clipboard.is_empty() {
                    self.paste_clipboard();
                    self.toast("已复制");
                }
            }
            Action::Paste | Action::SelPaste => self.paste_clipboard(),
            Action::SelDelete => {
                if !self.selection.uids.is_empty() {
                    let cmd = remove_command(&self.doc, self.selection.page, &self.selection.uids);
                    self.history.execute(&mut self.doc, cmd);
                    self.selection.uids.clear();
                    self.popup = None;
                }
            }
            Action::SelColor(i) => {
                let pg = self.selection.page;
                let before: Vec<Item> = self.selection.uids.iter().filter_map(|u| self.doc.get_item(pg, *u)).collect();
                let after: Vec<Item> = before
                    .iter()
                    .cloned()
                    .map(|mut it| {
                        it.set_color(PEN_COLORS[i]);
                        it
                    })
                    .collect();
                if !before.is_empty() {
                    self.history.execute(&mut self.doc, Command::Modify { page: pg, before, after });
                }
            }
            Action::DialogButton(id) => self.dialog_button(id),
            Action::CloseDialog => {
                if let Some(Dialog::Update(UpdateUi::Downloading { .. })) = &self.dialog {
                    if let Some(u) = &self.updater {
                        u.cancel();
                    }
                }
                self.dialog = None;
            }
            Action::Theme(m) => {
                self.settings.theme_mode = m;
                self.apply_theme();
                self.save_settings();
            }
            Action::Accent(i) => {
                self.settings.accent = i as u8;
                self.apply_theme();
                self.save_settings();
            }
            Action::ToggleSplash => {
                self.settings.show_splash = !self.settings.show_splash;
                self.save_settings();
            }
            Action::TogglePressure => {
                self.settings.pressure = !self.settings.pressure;
                self.board.invalidate();
                self.save_settings();
            }
            Action::ToggleAutoUpdate => {
                self.settings.auto_check_update = !self.settings.auto_check_update;
                self.save_settings();
            }
            Action::CheckUpdate => self.check_update(true),
            Action::DownloadUpdate => {
                if let (Some(Dialog::Update(UpdateUi::Available(rel))), Some(u)) = (&self.dialog, &self.updater) {
                    let rel = rel.clone();
                    self.dialog = Some(Dialog::Update(UpdateUi::Downloading { done: 0, total: rel.asset.size, version: rel.version.clone() }));
                    u.download(rel);
                }
            }
            Action::InstallUpdate => {
                if let Some(Dialog::Update(UpdateUi::Ready { package, .. })) = &self.dialog {
                    let package = package.clone();
                    self.shutdown();
                    self.requests.push(Request::InstallUpdate { package });
                }
            }
            Action::CancelUpdate => {
                if let Some(u) = &self.updater {
                    u.cancel();
                }
                self.dialog = None;
            }
            Action::SkipUpdate => {
                if let Some(Dialog::Update(UpdateUi::Available(rel))) = &self.dialog {
                    self.settings.skipped_version = rel.version.clone();
                    self.save_settings();
                }
                self.dialog = None;
            }
            Action::ResetSettings => {
                self.dialog = Some(Dialog::Confirm {
                    title: "重置设置".into(),
                    message: "确定将所有设置恢复为默认值？".into(),
                    buttons: vec![(DLG_RESET, "重置".into(), widgets::BtnStyle::Danger), (DLG_CANCEL, "取消".into(), widgets::BtnStyle::Ghost)],
                });
            }
            Action::TimerTab(t) => {
                if let Some(tp) = &mut self.timer {
                    tp.tab = if t == 0 { TimerTab::Countdown } else { TimerTab::Stopwatch };
                }
            }
            Action::TimerPreset(m) => {
                if let Some(tp) = &mut self.timer {
                    tp.countdown.set_total(Duration::from_secs(m as u64 * 60));
                    tp.alarm_since = None;
                    self.settings.timer_last_minutes = m;
                }
                self.save_settings();
            }
            Action::TimerAdjust(d) => {
                if let Some(tp) = &mut self.timer {
                    if tp.countdown.state() == RunState::Idle || tp.countdown.state() == RunState::Finished {
                        let cur = tp.countdown.total.as_secs() as i64;
                        let n = (cur + d as i64).clamp(10, 99 * 60 + 59);
                        tp.countdown.set_total(Duration::from_secs(n as u64));
                        tp.alarm_since = None;
                    }
                }
            }
            Action::TimerStartPause => {
                let now = self.now;
                if let Some(tp) = &mut self.timer {
                    match tp.tab {
                        TimerTab::Countdown => {
                            tp.alarm_since = None;
                            tp.countdown.toggle(now)
                        }
                        TimerTab::Stopwatch => tp.stopwatch.toggle(now),
                    }
                }
            }
            Action::TimerReset => {
                if let Some(tp) = &mut self.timer {
                    match tp.tab {
                        TimerTab::Countdown => {
                            tp.countdown.reset();
                            tp.alarm_since = None;
                        }
                        TimerTab::Stopwatch => tp.stopwatch.reset(),
                    }
                }
            }
            Action::TimerLap => {
                let now = self.now;
                if let Some(tp) = &mut self.timer {
                    tp.stopwatch.lap(now);
                }
            }
            Action::TimerFullscreen => {
                if let Some(tp) = &mut self.timer {
                    tp.fullscreen = !tp.fullscreen;
                }
            }
            Action::TimerSound => {
                self.settings.timer_sound = !self.settings.timer_sound;
                self.save_settings();
            }
            Action::TimerDismissAlarm => {
                if let Some(tp) = &mut self.timer {
                    tp.alarm_since = None;
                    tp.countdown.reset();
                }
            }
        }
    }

    fn current_edit_page(&self) -> usize {
        if self.is_pdf() {
            self.display_page()
        } else {
            self.page
        }
    }

    fn after_history(&mut self, page: usize) {
        self.selection.uids.clear();
        if page < self.page_count() && !self.is_pdf() && page != self.page {
            self.page = page;
        }
        if self.page >= self.page_count() {
            self.page = self.page_count() - 1;
        }
        self.dirty();
    }

    fn paste_clipboard(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let page = self.current_edit_page();
        let off = v2(24.0, 24.0);
        let mut items = Vec::new();
        let mut uids = Vec::new();
        for it in self.clipboard.clone() {
            let mut it = it;
            it.translate(off);
            let uid = self.doc.alloc_uid();
            match &mut it {
                Item::Stroke(s) => s.uid = uid,
                Item::Text(t) => t.uid = uid,
                Item::Image(i) => i.uid = uid,
            }
            let idx = match &it {
                Item::Stroke(_) => self.doc.pages[page].strokes.len() + items.iter().filter(|(_, i): &&(usize, Item)| matches!(i, Item::Stroke(_))).count(),
                Item::Text(_) => self.doc.pages[page].texts.len() + items.iter().filter(|(_, i): &&(usize, Item)| matches!(i, Item::Text(_))).count(),
                Item::Image(_) => self.doc.pages[page].images.len() + items.iter().filter(|(_, i): &&(usize, Item)| matches!(i, Item::Image(_))).count(),
            };
            uids.push(uid);
            items.push((idx, it));
        }
        self.clipboard = items.iter().map(|(_, i)| i.clone()).collect();
        self.history.execute(&mut self.doc, Command::Add { page, items });
        self.selection = Selection { page, uids };
        self.tool = Tool::Select;
        self.popup = Some(PopupKind::Selection);
        self.dirty();
    }

    fn goto_page(&mut self, idx: usize) {
        let idx = idx.min(self.page_count().saturating_sub(1));
        self.commit_text_edit();
        self.selection.uids.clear();
        if self.is_pdf() {
            let k = self.vp.k();
            self.vp.scroll.y = self.layout.pages[idx].y0 * k - 12.0 * self.scale;
            self.clamp_scroll();
        } else {
            self.page = idx;
        }
        self.dirty();
    }

    pub fn fit_width(&mut self) {
        if let Some(r) = self.layout.pages.get(self.display_page()) {
            let avail = self.vp.rect.w() - 48.0 * self.scale;
            let z = (avail / (r.w() * self.scale)).clamp(ZOOM_MIN, 2.0);
            let c = self.vp.rect.center();
            self.zoom_to(z, c);
        }
    }

    fn dialog_button(&mut self, id: u32) {
        self.dialog = None;
        match id {
            DLG_EXIT_SAVE => {
                self.after_save = Some(AfterSave::Exit);
                self.perform(Action::Save);
            }
            DLG_EXIT_DISCARD => {
                self.shutdown();
                self.requests.push(Request::Exit);
            }
            DLG_DELETE_PAGE => {
                let idx = self.page;
                let page = self.doc.pages[idx].clone();
                self.history.execute(&mut self.doc, Command::DeletePage { index: idx, page });
                self.page = idx.min(self.page_count() - 1);
                self.toast("已删除页面（可撤销）");
            }
            DLG_RESET => {
                let path = self.env.settings_path.clone();
                self.settings = Settings::default();
                self.pen_color = Color::from_argb(self.settings.pen_color);
                self.pen_width = self.settings.pen_width;
                if let Some(p) = path {
                    let _ = self.settings.save_to(&p);
                }
                self.apply_theme();
                self.toast("设置已重置");
            }
            id if id >= 10_000 => {
                self.popup = None;
                self.goto_page((id - 10_000) as usize);
            }
            DLG_OPEN_DISCARD => {
                if let Some(p) = self.pending_open.take() {
                    self.open_path(p);
                }
            }
            _ => {
                self.after_save = None;
                self.pending_open = None;
            }
        }
        self.dirty();
    }

    // ------------------------------------------------------------ files

    fn default_save_name(&self) -> String {
        let base = self.doc.pdf.as_ref().map(|p| p.file_name.trim_end_matches(".pdf").trim_end_matches(".PDF").to_string()).unwrap_or_else(|| "白板".into());
        format!("{base}.cloudboard")
    }

    fn open_path_checked(&mut self, p: PathBuf) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let is_image = matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp" | "gif");
        if !is_image && self.has_unsaved_changes() {
            self.pending_open = Some(p);
            self.dialog = Some(Dialog::Confirm {
                title: "打开文件".into(),
                message: "当前白板有未保存的内容，打开新文件将丢失这些内容。".into(),
                buttons: vec![(DLG_OPEN_DISCARD, "不保存，继续打开".into(), widgets::BtnStyle::Danger), (DLG_CANCEL, "取消".into(), widgets::BtnStyle::Ghost)],
            });
            self.dirty();
            return;
        }
        self.open_path(p);
    }

    pub fn open_path(&mut self, p: PathBuf) {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => {
                self.error("无法打开文件", &format!("{}\n{e}", p.display()));
                return;
            }
        };
        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "gif" => self.insert_image(bytes),
            "pdf" => {
                let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "document.pdf".into());
                self.open_pdf_bytes(name, Arc::new(bytes));
                self.project_path = None;
            }
            _ => self.open_project(&p, &bytes),
        }
    }

    fn reset_document(&mut self, doc: Document) {
        self.doc = doc;
        self.doc.touch();
        self.history.clear();
        self.saved_version = self.history.version;
        self.selection = Selection::default();
        self.text_edit = None;
        self.page = 0;
        self.layout = Layout::build(&self.doc);
        self.board = BoardRenderer::default();
        self.dirty();
    }

    fn ensure_pdf_engine(&mut self) -> Result<(), String> {
        if self.pdf_engine.is_some() {
            return Ok(());
        }
        let lib = self.env.pdfium.clone().or_else(cb_pdf::locate_library).ok_or_else(|| "找不到 PDF 渲染组件（pdfium）。请重新安装 CloudBoard。".to_string())?;
        let e = PdfEngine::start(&lib, self.env.waker.clone()).map_err(|e| e.to_string())?;
        self.pdf_engine = Some(e);
        Ok(())
    }

    /// Open PDF bytes; returns page sizes (pt).
    fn load_pdf(&mut self, bytes: Arc<Vec<u8>>) -> Result<Vec<(f32, f32)>, String> {
        self.ensure_pdf_engine()?;
        let eng = self.pdf_engine.as_mut().unwrap();
        let info = eng.open(bytes).map_err(|e| e.to_string())?;
        if info.pages.is_empty() {
            return Err("PDF 没有页面".into());
        }
        let mem_budget = 192usize << 20;
        self.pdf = Some(PdfState { cache: TileCache::new(mem_budget), pages_pt: info.pages.clone(), last_plan: None, zoom_changed_at: self.now - Duration::from_secs(1) });
        Ok(info.pages)
    }

    pub fn open_pdf_bytes(&mut self, name: String, bytes: Arc<Vec<u8>>) {
        match self.load_pdf(bytes.clone()) {
            Ok(sizes) => {
                let mut doc = Document::new();
                doc.pdf = Some(PdfSource { file_name: name, bytes });
                doc.pages = sizes
                    .iter()
                    .enumerate()
                    .map(|(i, (w, h))| Page { background: PageBackground::Pdf { index: i as u32, width: w * PT_TO_UNITS, height: h * PT_TO_UNITS }, ..Default::default() })
                    .collect();
                self.reset_document(doc);
                self.vp.zoom = 1.0;
                self.fit_width();
                self.vp.scroll.y = 0.0;
                self.clamp_scroll();
                self.toast(format!("已打开 PDF（{} 页）", sizes.len()));
            }
            Err(e) => self.error("无法打开 PDF", &e),
        }
    }

    fn open_project(&mut self, path: &Path, bytes: &[u8]) {
        let loaded = match format::load(bytes) {
            Ok(l) => l,
            Err(e) => {
                self.error("无法打开工程文件", &e.to_string());
                return;
            }
        };
        let mut doc = loaded.doc;
        if let Some(pdf) = doc.pdf.clone() {
            match self.load_pdf(pdf.bytes.clone()) {
                Ok(sizes) => {
                    if let Some(info) = &loaded.legacy_pdf {
                        format::fixup_legacy_pdf(&mut doc, info, &sizes);
                    }
                }
                Err(e) => {
                    self.error("无法加载工程中的 PDF", &e);
                    doc.pdf = None;
                }
            }
        } else {
            self.pdf = None;
        }
        let view = loaded.view;
        self.reset_document(doc);
        if let Some(a) = loaded.assist {
            self.assist = a;
        }
        self.vp.zoom = view.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        self.vp.scroll = v2(view.scroll_x, view.scroll_y);
        self.page = view.current_page.min(self.page_count() - 1);
        self.clamp_scroll();
        self.project_path = if loaded.version >= format::VERSION_V2 { Some(path.to_path_buf()) } else { None };
        self.toast(if loaded.version < format::VERSION_V2 { "已打开旧版工程（保存时将转换为新格式）" } else { "已打开工程" });
    }

    fn insert_image(&mut self, bytes: Vec<u8>) {
        let Some(px) = cb_render::images::decode(&bytes) else {
            self.error("无法插入图片", "不支持的图片格式");
            return;
        };
        let (w, h) = (px.width() as f32, px.height() as f32);
        let maxs = 600.0f32;
        let k = (maxs / w.max(h)).min(1.0);
        let page = self.current_edit_page();
        let c = self.vp.to_canvas(self.vp.rect.center()) - self.layout.origin(page);
        let n = self.doc.pages[page].images.len() as f32;
        let item = ImageItem { uid: self.doc.alloc_uid(), pos: c - v2(w * k * 0.5, h * k * 0.5) + v2(n * 30.0, n * 30.0), w: w * k, h: h * k, rotate_deg: 0.0, data: Arc::new(bytes) };
        let idx = self.doc.pages[page].images.len();
        self.history.execute(&mut self.doc, Command::Add { page, items: vec![(idx, Item::Image(item))] });
        self.dirty();
    }

    fn save_to(&mut self, mut path: PathBuf) {
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("cloudboard")) != Some(true) {
            path.set_extension("cloudboard");
        }
        let view = ViewState { current_page: self.page, zoom: self.vp.zoom, scroll_x: self.vp.scroll.x, scroll_y: self.vp.scroll.y };
        // Serialize on the UI thread (fast, consistent snapshot), write in the background.
        let bytes = format::save(&self.doc, &view, Some(&self.assist), cb_core::version::APP_VERSION);
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.env.waker.clone();
        let p2 = path.clone();
        std::thread::spawn(move || {
            let tmp = p2.with_extension("cloudboard.tmp");
            let r = std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &p2)).map(|_| p2.clone()).map_err(|e| e.to_string());
            let _ = tx.send(r);
            waker();
        });
        self.saving = Some(rx);
        self.saved_version = self.history.version;
        self.toast("正在保存…");
    }

    fn error(&mut self, title: &str, text: &str) {
        self.dialog = Some(Dialog::Message { title: title.into(), text: text.into() });
        self.dirty();
    }

    // ------------------------------------------------------------ services

    pub fn check_update(&mut self, manual: bool) {
        let Some(u) = &self.updater else {
            if manual {
                self.error("检查更新", "当前版本不支持在线更新。");
            }
            return;
        };
        u.check();
        if manual {
            self.dialog = Some(Dialog::Update(UpdateUi::Checking));
        }
        self.started_update_check = true;
        self.dirty();
    }

    fn start_remote(&mut self) {
        let s = self.scale;
        let pos = v2((self.size.0 as f32 - 400.0 * s) * 0.5, 40.0 * s);
        let mut panel = RemotePanel {
            pos,
            server: None,
            error: None,
            hotspot: HotspotUi::NotAttempted,
            qr: None,
            url: String::new(),
            peer: None,
            last_preview: None,
            preview_rev: u64::MAX,
            strokes: HashMap::new(),
        };
        match cb_remote::RemoteServer::start(self.settings.remote_port, self.env.waker.clone()) {
            Ok(srv) => {
                if let Some(u) = srv.info().primary_url() {
                    panel.qr = cb_remote::qr_matrix(&u);
                    panel.url = u;
                }
                panel.server = Some(srv);
                if self.env.can_hotspot {
                    panel.hotspot = HotspotUi::Starting;
                    self.requests.push(Request::StartHotspot);
                }
            }
            Err(e) => panel.error = Some(format!("无法启动连接服务：{e}")),
        }
        self.remote = Some(panel);
        self.dirty();
    }

    fn stop_remote(&mut self) {
        if let Some(r) = self.remote.take() {
            if matches!(r.hotspot, HotspotUi::On { .. }) {
                self.requests.push(Request::StopHotspot);
            }
        }
        self.dirty();
    }

    fn on_hotspot(&mut self, r: HotspotResult) {
        if let Some(p) = &mut self.remote {
            p.hotspot = match r {
                HotspotResult::Started { ssid, passphrase } => HotspotUi::On { ssid, passphrase },
                HotspotResult::Unsupported(m) | HotspotResult::Failed(m) => HotspotUi::Unavailable(m),
            };
            if let Some(srv) = &mut p.server {
                // Hotspot adds a new interface (192.168.137.1 on Windows).
                srv.refresh_ips();
                if let Some(u) = srv.info().primary_url() {
                    p.qr = cb_remote::qr_matrix(&u);
                    p.url = u;
                }
            }
        }
        self.dirty();
    }

    fn handle_remote(&mut self) {
        let Some(panel) = &mut self.remote else { return };
        let Some(srv) = &panel.server else { return };
        let msgs = srv.poll();
        if msgs.is_empty() {
            return;
        }
        let vis = self.vp.visible();
        let mut actions = Vec::new();
        for m in msgs {
            match m {
                cb_remote::RemoteMsg::Connected { peer } => {
                    panel.peer = Some(peer);
                    panel.preview_rev = u64::MAX;
                    actions.push(Action::None);
                }
                cb_remote::RemoteMsg::Disconnected => {
                    panel.peer = None;
                    panel.strokes.clear();
                }
                cb_remote::RemoteMsg::Client(c) => {
                    use cb_remote::ClientMsg as C;
                    let map = |x: f32, y: f32| v2(vis.x0 + x * vis.w(), vis.y0 + y * vis.h());
                    match c {
                        C::Hello { .. } => {}
                        C::Begin { id, x, y, p, color, w, hl, eraser } => {
                            let c = map(x, y);
                            let page = if self.layout.is_strip() { self.layout.page_at(c).unwrap_or(0) } else { self.page };
                            let o = self.layout.origin(page);
                            let col = cb_remote::proto::parse_color(&color).map(Color::from_argb).unwrap_or(self.pen_color);
                            let mut s = Stroke::new(0, col, (w * vis.w()).clamp(0.5, 200.0));
                            s.highlighter = hl;
                            s.points.push(InkPoint { x: c.x - o.x, y: c.y - o.y, pressure: p, dt_ms: 0 });
                            panel.strokes.insert(id, (page, s, eraser));
                        }
                        C::Move { id, pts } => {
                            if let Some((page, s, eraser)) = panel.strokes.get_mut(&id) {
                                let o = self.layout.origin(*page);
                                for q in pts {
                                    let c = map(q[0], q[1]) - o;
                                    if *eraser {
                                        let last = s.points.last().map(|p| p.pos()).unwrap_or(c);
                                        erase::erase_along(&mut self.doc, *page, last, c, s.width * 0.5);
                                    }
                                    s.points.push(InkPoint { x: c.x, y: c.y, pressure: q[2], dt_ms: 0 });
                                }
                                self.redraw = true;
                                if *eraser {
                                    self.board.invalidate();
                                }
                            }
                        }
                        C::End { id } => {
                            if let Some((page, s, eraser)) = panel.strokes.remove(&id) {
                                if !eraser {
                                    actions.push(Action::None);
                                    let mut s = s;
                                    s.uid = self.doc.alloc_uid();
                                    let idx = self.doc.pages[page].strokes.len();
                                    self.history.execute(&mut self.doc, Command::Add { page, items: vec![(idx, Item::Stroke(s))] });
                                } else {
                                    self.history.break_merge();
                                }
                            }
                        }
                        C::Undo => actions.push(Action::Undo),
                        C::Redo => actions.push(Action::Redo),
                        C::Page { d } => actions.push(if d < 0 { Action::PrevPage } else { Action::NextPage }),
                    }
                }
            }
        }
        for a in actions {
            self.perform(a);
        }
        self.dirty();
    }

    fn push_remote_preview(&mut self) {
        let Some(panel) = &self.remote else { return };
        let Some(srv) = &panel.server else { return };
        if panel.peer.is_none() {
            return;
        }
        let rev = self.doc.revision ^ ((self.vp.scroll.x as i64 as u64) << 20) ^ ((self.vp.scroll.y as i64 as u64) << 40) ^ (self.vp.zoom.to_bits() as u64) ^ (self.page as u64) << 8;
        if rev == panel.preview_rev {
            return;
        }
        if panel.last_preview.is_some_and(|t| self.now.duration_since(t) < Duration::from_millis(120)) {
            return;
        }
        let Some(cache) = self.board.cache() else { return };
        let png = downscale_jpeg(cache, 1024);
        let aspect = self.vp.rect.h() / self.vp.rect.w().max(1.0);
        srv.send_state(&cb_remote::ServerState::new(self.display_page(), self.page_count(), aspect, self.is_pdf()));
        if let Some(png) = png {
            srv.send_preview(png);
        }
        let panel = self.remote.as_mut().unwrap();
        panel.preview_rev = rev;
        panel.last_preview = Some(self.now);
    }

    /// Poll background work (PDF tiles, saves, updates, remote, timers).
    pub fn poll(&mut self) {
        // PDF tiles.
        if let (Some(eng), Some(pdf)) = (&self.pdf_engine, &mut self.pdf) {
            let mut got = false;
            while let Some(r) = eng.try_recv() {
                pdf.cache.insert(r.key, r.pixmap);
                got = true;
            }
            if got {
                self.redraw = true;
            }
        }
        // Save completion.
        if let Some(rx) = &self.saving {
            if let Ok(r) = rx.try_recv() {
                self.saving = None;
                match r {
                    Ok(p) => {
                        self.project_path = Some(p.clone());
                        self.toast(format!("已保存：{}", p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()));
                        if self.after_save.take() == Some(AfterSave::Exit) {
                            self.shutdown();
                            self.requests.push(Request::Exit);
                        }
                    }
                    Err(e) => {
                        self.saved_version = u64::MAX;
                        self.after_save = None;
                        self.error("保存失败", &e);
                    }
                }
            }
        }
        // Updates.
        let update_events: Vec<cb_update::UpdateEvent> = self.updater.as_ref().map(|u| std::iter::from_fn(|| u.poll()).collect()).unwrap_or_default();
        {
            for ev in update_events {
                let manual = matches!(self.dialog, Some(Dialog::Update(_)));
                let ui = match ev {
                    cb_update::UpdateEvent::Checking => Some(UpdateUi::Checking),
                    cb_update::UpdateEvent::UpToDate => Some(UpdateUi::UpToDate),
                    cb_update::UpdateEvent::Available(r) => {
                        if !manual && r.version == self.settings.skipped_version && !r.mandatory {
                            None
                        } else {
                            Some(UpdateUi::Available(r))
                        }
                    }
                    cb_update::UpdateEvent::Progress { done, total } => {
                        let v = match &self.dialog {
                            Some(Dialog::Update(UpdateUi::Downloading { version, .. })) => version.clone(),
                            _ => String::new(),
                        };
                        Some(UpdateUi::Downloading { done, total, version: v })
                    }
                    cb_update::UpdateEvent::Ready { package, version } => Some(UpdateUi::Ready { package, version }),
                    cb_update::UpdateEvent::Failed(e) => Some(UpdateUi::Failed(e)),
                };
                if let Some(ui) = ui {
                    let show = manual || matches!(ui, UpdateUi::Available(_)) && self.dialog.is_none();
                    if show {
                        self.dialog = Some(Dialog::Update(ui));
                        self.dirty();
                    }
                }
            }
        }
        self.handle_remote();
        self.tick_timer();
        if let Some((_, t)) = &self.toast {
            if self.now.duration_since(*t) > Duration::from_millis(2600) {
                self.toast = None;
                self.redraw = true;
            }
        }
    }

    fn tick_timer(&mut self) {
        let now = self.now;
        let sound = self.settings.timer_sound;
        let mut play = false;
        let mut toast = false;
        if let Some(tp) = &mut self.timer {
            if tp.countdown.update(now) {
                tp.alarm_since = Some(now);
                tp.last_alarm_sound = None;
                toast = true;
            }
            if let Some(since) = tp.alarm_since {
                if now.duration_since(since) > Duration::from_secs(30) {
                    tp.alarm_since = None;
                } else if sound && tp.last_alarm_sound.map_or(true, |t| now.duration_since(t) >= Duration::from_millis(1500)) && now.duration_since(since) < Duration::from_secs(9) {
                    tp.last_alarm_sound = Some(now);
                    play = true;
                }
                self.redraw = true;
            }
            if tp.countdown.state() == RunState::Running || tp.stopwatch.state() == RunState::Running {
                self.redraw = true;
                self.widgets_dirty = true;
            }
        }
        if play {
            self.requests.push(Request::PlaySound(Sound::Alarm));
        }
        if toast {
            self.toast("倒计时结束");
        }
    }

    /// When the platform should wake us next even without input.
    pub fn next_deadline(&self) -> Option<Instant> {
        let mut d: Option<Instant> = None;
        let mut take = |t: Instant| d = Some(d.map_or(t, |x: Instant| x.min(t)));
        if let Some((_, t)) = &self.toast {
            take(*t + Duration::from_millis(2650));
        }
        if let Some(tp) = &self.timer {
            if tp.countdown.state() == RunState::Running {
                take(self.now + tp.countdown.next_tick_in(self.now).unwrap_or(Duration::from_millis(250)).min(Duration::from_millis(250)));
            }
            if tp.stopwatch.state() == RunState::Running {
                take(self.now + Duration::from_millis(100));
            }
            if tp.alarm_since.is_some() {
                take(self.now + Duration::from_millis(250));
            }
        }
        if let Some(pdf) = &self.pdf {
            let settle = pdf.zoom_changed_at + Duration::from_millis(160);
            if settle > self.now {
                take(settle);
            }
        }
        if self.saving.is_some() || self.remote.as_ref().is_some_and(|r| r.peer.is_some()) {
            take(self.now + Duration::from_millis(150));
        }
        d
    }

    /// Kick an automatic update check once after startup.
    pub fn startup(&mut self) {
        if self.settings.auto_check_update && !self.started_update_check {
            self.check_update(false);
        }
    }

    // ------------------------------------------------------------ rendering

    fn plan_pdf(&mut self) {
        let (Some(eng), Some(pdf)) = (&self.pdf_engine, &mut self.pdf) else { return };
        let vp = self.vp.snapped();
        let k = vp.k();
        let settled = self.now.duration_since(pdf.zoom_changed_at) > Duration::from_millis(150) && !matches!(self.gesture, Gesture::Pinch { .. });
        let key = (pdf.cache.version, vp.zoom.to_bits(), vp.scroll.x as i32, vp.scroll.y as i32, vp.rect.w() as u32 ^ ((settled as u32) << 31), vp.rect.h() as u32);
        if pdf.last_plan == Some(key) {
            return;
        }
        pdf.last_plan = Some(key);
        let vis = vp.visible();
        let h = vis.h();
        let mut list = Vec::new();
        // Visible pages plus one screen of prefetch above and below.
        let wide = Rect::new(vis.x0, vis.y0 - h, vis.x1, vis.y1 + h);
        for i in self.layout.visible_pages(&wide) {
            let pr = self.layout.pages[i];
            let iv = if pr.intersects(&vis) { pr.intersect(&vis) } else { pr.intersect(&wide) };
            if iv.is_empty() {
                continue;
            }
            let distance = if pr.intersects(&vis) { 0.0 } else { 1.0 };
            list.push(VisiblePage {
                page: i as u32,
                size_pt: pdf.pages_pt[i],
                x0: (iv.x0 - pr.x0) / PT_TO_UNITS,
                y0: (iv.y0 - pr.y0) / PT_TO_UNITS,
                x1: (iv.x1 - pr.x0) / PT_TO_UNITS,
                y1: (iv.y1 - pr.y0) / PT_TO_UNITS,
                distance,
            });
        }
        let jobs = pdf.cache.plan(&list, k * PT_TO_UNITS, &pdf.pages_pt, 60, settled);
        eng.set_wanted(jobs);
    }

    /// Render the full frame.
    pub fn render(&mut self, frame: &mut PixmapMut) {
        self.now = Instant::now();
        self.redraw = false;
        let th = self.theme;
        // The board blit covers the board area; only the bar strip needs a background.
        let (fw, fh) = (frame.width() as f32, frame.height() as f32);
        if (fw, fh) != (self.size.0 as f32, self.size.1 as f32) {
            cb_render::paint::fill_rect(frame, Rect::new(0.0, 0.0, fw, fh), th.window_bg);
        }
        self.vp = Viewport { scroll: v2(self.vp.scroll.x.round(), self.vp.scroll.y.round()), ..self.vp };
        self.plan_pdf();
        let hidden: Vec<Uid> = self.text_edit.as_ref().and_then(|t| t.uid).into_iter().collect();
        {
            let inp = BoardInput {
                doc: &self.doc,
                layout: &self.layout,
                vp: self.vp,
                page: self.page,
                theme: &self.theme,
                pressure: self.settings.pressure,
                tiles: self.pdf.as_mut().map(|p| &mut p.cache),
                hidden: &hidden,
            };
            self.board.render(inp, frame, &mut self.fonts);
        }
        self.draw_overlay(frame);
        self.push_remote_preview();
        if self.widgets_dirty {
            self.widgets = self.build_widgets();
            self.widgets_dirty = false;
        }
        let ws = std::mem::take(&mut self.widgets);
        // Bottom bar: widgets entirely inside the bar strip are cached as a pixmap.
        let bar_top = self.vp.rect.y1;
        let split = ws.iter().position(|w| w.rect.y0 < bar_top - 0.5).unwrap_or(ws.len());
        let (bar, rest) = ws.split_at(split);
        let hover_bar = self.hover.filter(|h| *h < split);
        let pressed_bar = self.pressed.filter(|h| *h < split);
        let key = (bar.to_vec(), hover_bar, pressed_bar, th.dark, th.accent, self.size, self.scale.to_bits());
        let bh = (fh - bar_top).max(1.0) as u32;
        if self.bar_cache.as_ref().map(|(k, _)| k != &key).unwrap_or(true) {
            if let Some(mut px) = Pixmap::new(self.size.0.max(1), bh) {
                {
                    let mut pm = px.as_mut();
                    cb_render::paint::fill_rect(&mut pm, Rect::new(0.0, 0.0, fw, bh as f32), th.window_bg);
                    let shifted: Vec<Widget> = bar.iter().cloned().map(|mut w| {
                        w.rect = w.rect.translate(v2(0.0, -bar_top));
                        w
                    }).collect();
                    let ctx = widgets::DrawCtx { theme: &self.theme, scale: self.scale, hover: hover_bar, pressed: pressed_bar };
                    widgets::draw(&mut pm, &mut self.fonts, &shifted, &ctx);
                }
                self.bar_cache = Some((key, px));
            }
        }
        if let Some((_, px)) = &self.bar_cache {
            cb_render::paint::blit_copy(frame, px, 0, bar_top as i32);
        }
        // Everything else (popups, panels, dialogs) is drawn live; indices offset by `split`.
        let ctx = widgets::DrawCtx { theme: &self.theme, scale: self.scale, hover: self.hover.and_then(|h| h.checked_sub(split)), pressed: self.pressed.and_then(|h| h.checked_sub(split)) };
        widgets::draw(frame, &mut self.fonts, rest, &ctx);
        self.widgets = ws;
        self.draw_after_widgets(frame);
    }

    fn page_transform(&self, page: usize) -> Transform {
        let o = self.layout.origin(page);
        let k = self.vp.k();
        Transform::from_row(k, 0.0, 0.0, k, o.x * k - self.vp.scroll.x + self.vp.rect.x0, o.y * k - self.vp.scroll.y + self.vp.rect.y0)
    }

    fn draw_overlay(&mut self, pm: &mut PixmapMut) {
        let s = self.scale;
        let k = self.vp.k();
        let th = self.theme;
        // Live strokes.
        if let Gesture::Draw { page, stroke, .. } = &self.gesture {
            let t = self.page_transform(*page);
            cb_render::ink::draw_stroke(pm, stroke, t, self.settings.pressure);
        }
        if let Some(r) = &self.remote {
            for (page, st, eraser) in r.strokes.values() {
                if !eraser {
                    cb_render::ink::draw_stroke(pm, st, self.page_transform(*page), self.settings.pressure);
                }
            }
        }
        // Selection.
        if !self.selection.uids.is_empty() {
            let bb = self.selection_bounds();
            if !bb.is_empty() {
                let o = self.layout.origin(self.selection.page);
                let a = self.vp.to_screen(v2(bb.x0, bb.y0) + o);
                let b = self.vp.to_screen(v2(bb.x1, bb.y1) + o);
                let r = Rect::from_points(a, b).inflate(6.0 * s);
                cb_render::paint::fill_rrect(pm, r, 6.0 * s, th.accent.with_alpha(22));
                if let Some(p) = cb_render::paint::rounded_rect_path(r, 6.0 * s) {
                    let st = cb_render::tiny_skia::Stroke { width: 1.5 * s, dash: cb_render::tiny_skia::StrokeDash::new(vec![6.0 * s, 4.0 * s], 0.0), ..Default::default() };
                    pm.stroke_path(&p, &cb_render::paint::paint(th.accent), &st, Transform::identity(), None);
                }
            }
        }
        if let Gesture::Rubber { start, cur, .. } = &self.gesture {
            let r = Rect::from_points(self.vp.to_screen(*start), self.vp.to_screen(*cur));
            cb_render::paint::fill_rect(pm, r, th.accent.with_alpha(30));
            cb_render::paint::stroke_rrect(pm, r, 0.0, 1.0 * s, th.accent);
        }
        // Teaching aids.
        let m = cb_render::assist::Mapper { zoom: k, ox: -self.vp.scroll.x + self.vp.rect.x0, oy: -self.vp.scroll.y + self.vp.rect.y0 };
        let cols = cb_render::assist::AssistColors::new(th.accent);
        for (i, t) in self.assist.tools.iter().enumerate() {
            let (edge, readout) = match &self.gesture {
                Gesture::Rule { tool, edge, start, cur, .. } if *tool == i => {
                    let len = t.rule(*edge, *start, *cur, 0.0).map(|(a, b)| cb_core::assist::AssistTool::length_cm(a, b)).unwrap_or(0.0);
                    (Some(*edge), Some(format!("{len:.1} cm")))
                }
                Gesture::Assist { idx, hit: AssistHit::Rotate, .. } if *idx == i => (None, Some(format!("{:.0}°", (360.0 - t.angle.to_degrees()).rem_euclid(360.0)))),
                Gesture::Compass { idx, tracker, .. } if *idx == i => (None, Some(format!("{:.0}°", tracker.sweep.to_degrees().abs()))),
                _ => (None, None),
            };
            cb_render::assist::draw_tool(pm, &mut self.fonts, t, m, s, &cols, edge, readout.as_deref());
        }
        // Ruling preview.
        if let Gesture::Rule { tool, edge, start, cur, .. } = &self.gesture {
            if let Some((a, b)) = self.assist.tools[*tool].rule(*edge, *start, *cur, self.pen_width * 0.5) {
                let t = Transform::from_row(k, 0.0, 0.0, k, -self.vp.scroll.x + self.vp.rect.x0, -self.vp.scroll.y + self.vp.rect.y0);
                cb_render::ink::draw_segment(pm, a, b, self.pen_width, self.pen_color, t);
            }
        }
        // Compass arc preview.
        if let Gesture::Compass { idx, tracker, .. } = &self.gesture {
            let t = self.assist.tools[*idx];
            let pts = arc_points(t.pos, t.radius, tracker.start, tracker.sweep);
            let mut st = Stroke::new(0, self.pen_color, self.pen_width);
            st.points = pts.into_iter().map(|p| InkPoint::new(p.x, p.y)).collect();
            let tr = Transform::from_row(k, 0.0, 0.0, k, -self.vp.scroll.x + self.vp.rect.x0, -self.vp.scroll.y + self.vp.rect.y0);
            cb_render::ink::draw_stroke(pm, &st, tr, false);
        }
        // Eraser cursor.
        if let Gesture::Erase { last, .. } = &self.gesture {
            let c = self.vp.to_screen(*last);
            let r = self.settings.eraser_size * 0.5 * self.scale;
            cb_render::paint::fill_circle(pm, c, r, Color::rgba(255, 255, 255, 120));
            cb_render::paint::stroke_circle(pm, c, r, 1.5 * s, Color::rgba(60, 60, 70, 200));
        }
        // Text editing box.
        if let Some(te) = &self.text_edit {
            let o = self.layout.origin(te.page);
            let size = te.before.as_ref().map(|b| b.font_size).unwrap_or((self.pen_width * 6.0).clamp(20.0, 96.0));
            let color = te.before.as_ref().map(|b| b.color).unwrap_or(self.pen_color);
            let p = self.vp.to_screen(te.pos + o);
            let st = cb_render::text::TextStyle::new(size * k, color);
            let shown = if te.text.is_empty() { " " } else { te.text.as_str() };
            let (w, h) = self.fonts.measure(shown, st);
            let r = Rect::xywh(p.x - 6.0 * s, p.y - 4.0 * s, w.max(size * k * 2.0) + 12.0 * s, h.max(size * k * 1.2) + 8.0 * s);
            cb_render::paint::fill_rrect(pm, r, 6.0 * s, Color::rgba(255, 255, 255, 200));
            cb_render::paint::stroke_rrect(pm, r, 6.0 * s, 1.5 * s, th.accent);
            self.fonts.draw(pm, &te.text, p.x, p.y, st);
            // Caret at end of last line.
            let last = te.text.rsplit('\n').next().unwrap_or("");
            let (lw, _) = if last.is_empty() { (0.0, 0.0) } else { self.fonts.measure(last, st) };
            let lines = te.text.split('\n').count().max(1) as f32;
            let cy = p.y + (lines - 1.0) * (size * k * 1.28);
            cb_render::paint::fill_rect(pm, Rect::xywh(p.x + lw + 1.0, cy + 2.0 * s, 2.0 * s, size * k * 1.15), th.accent);
        }
        // Toast.
        if let Some((msg, _)) = &self.toast {
            let st = cb_render::text::TextStyle::new(14.0 * s, Color::rgb(255, 255, 255)).weight(500).center();
            let (w, h) = self.fonts.measure(msg, st);
            let cx = self.vp.rect.center().x;
            let r = Rect::xywh(cx - w * 0.5 - 18.0 * s, 18.0 * s, w + 36.0 * s, h + 18.0 * s);
            cb_render::paint::shadow(pm, r, r.h() * 0.5, 10.0 * s, 3.0 * s, Color::rgba(0, 0, 0, 70));
            cb_render::paint::fill_rrect(pm, r, r.h() * 0.5, Color::rgba(28, 32, 44, 236));
            let m = msg.clone();
            self.fonts.draw_vcenter(pm, &m, cx, r.center().y, st);
        }
    }

    fn draw_after_widgets(&mut self, _pm: &mut PixmapMut) {}

    // ------------------------------------------------------------ helpers used by chrome

    fn slider_to(&mut self, idx: usize, x: f32) {
        let Some(w) = self.widgets.get(idx) else { return };
        if let widgets::WKind::Slider { min, max, .. } = w.kind {
            let t = ((x - w.rect.x0) / w.rect.w()).clamp(0.0, 1.0);
            let v = (min + (max - min) * t).round();
            if w.action == Action::EraserSize {
                self.settings.eraser_size = v;
            }
            self.dirty();
        }
    }

    pub fn panel_pos(&self, which: PanelKind) -> Vec2 {
        match which {
            PanelKind::Timer => self.timer.as_ref().map(|t| t.pos).unwrap_or_default(),
            PanelKind::Remote => self.remote.as_ref().map(|t| t.pos).unwrap_or_default(),
        }
    }

    fn set_panel_pos(&mut self, which: PanelKind, p: Vec2) {
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let p = v2(p.x.clamp(-200.0 * self.scale, w - 120.0 * self.scale), p.y.clamp(0.0, h - 60.0 * self.scale));
        match which {
            PanelKind::Timer => {
                if let Some(t) = &mut self.timer {
                    t.pos = p
                }
            }
            PanelKind::Remote => {
                if let Some(t) = &mut self.remote {
                    t.pos = p
                }
            }
        }
        self.dirty();
    }

    /// Current pressed/hovered widget for tests.
    pub fn widget_list(&mut self) -> Vec<Widget> {
        self.widgets().to_vec()
    }

    pub fn active_gesture_name(&self) -> &'static str {
        match self.gesture {
            Gesture::None => "none",
            Gesture::Widget { .. } => "widget",
            Gesture::Slider { .. } => "slider",
            Gesture::PanelDrag { .. } => "panel",
            Gesture::Draw { .. } => "draw",
            Gesture::Rule { .. } => "rule",
            Gesture::Erase { .. } => "erase",
            Gesture::Pan { .. } => "pan",
            Gesture::Pinch { .. } => "pinch",
            Gesture::Rubber { .. } => "rubber",
            Gesture::MoveSel { .. } => "move",
            Gesture::Assist { .. } => "assist",
            Gesture::Compass { .. } => "compass",
        }
    }

    pub fn pdf_pending(&self) -> usize {
        self.pdf_engine.as_ref().map(|e| e.pending()).unwrap_or(0)
    }

    pub fn pdf_cached_tiles(&self) -> usize {
        self.pdf.as_ref().map(|p| p.cache.len()).unwrap_or(0)
    }
}

/// Downscale a premultiplied pixmap to at most `max_w` wide and encode JPEG.
fn downscale_jpeg(src: &Pixmap, max_w: u32) -> Option<Vec<u8>> {
    let k = (max_w as f32 / src.width() as f32).min(1.0);
    let (w, h) = (((src.width() as f32) * k).round().max(1.0) as u32, ((src.height() as f32) * k).round().max(1.0) as u32);
    let mut dst = Pixmap::new(w, h)?;
    let paint = cb_render::tiny_skia::PixmapPaint { quality: cb_render::tiny_skia::FilterQuality::Bilinear, ..Default::default() };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_scale(k, k), None);
    cb_render::images::pixmap_to_jpeg(&dst, 78)
}
