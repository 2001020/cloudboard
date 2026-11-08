//! Retained-per-frame widget list: chrome layout produces widgets; the same
//! list is used for drawing and hit testing, so they can never disagree.

use cb_core::assist::AssistKind;
use cb_core::geom::{Rect, Vec2};
use cb_core::settings::ThemeMode;
use cb_core::Color;
use cb_render::icons::{self, Icon};
use cb_render::paint::{fill_circle, fill_rrect, fill_rrect_vgradient, shadow, stroke_circle, stroke_rrect};
use cb_render::text::{Fonts, TextStyle};
use cb_render::theme::Theme;
use cb_render::tiny_skia::PixmapMut;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    None,
    Exit,
    Recenter,
    Minimize,
    OpenSettings,
    Open,
    Save,
    TogglePopup(PopupKind),
    SetTool(crate::app::Tool),
    PenColor(usize),
    PenWidth(usize),
    PenStyle(cb_core::doc::LineStyle),
    ToggleHighlighter,
    ToggleShapeRecognition,
    EraserSize,
    EraserMode(bool),
    ClearPage,
    Undo,
    Redo,
    PrevPage,
    NextPage,
    AddPage,
    DeletePage,
    SetZoom(f32),
    /// Zoom slider (logarithmic value).
    ZoomSlider,
    ZoomFit,
    ToggleAssist(AssistKind),
    HideAssists,
    ToggleTimer,
    ToggleRemote,
    SelCopy,
    SelPaste,
    SelDelete,
    SelColor(usize),
    Paste,
    // dialogs
    DialogButton(u32),
    CloseDialog,
    Theme(ThemeMode),
    Accent(usize),
    RecordRes(cb_core::settings::RecordResolution),
    TogglePressure,
    ToggleAutoUpdate,
    CheckUpdate,
    DownloadUpdate,
    InstallUpdate,
    CancelUpdate,
    SkipUpdate,
    ResetSettings,
    // timer
    TimerTab(u8),
    TimerPreset(u32),
    /// Countdown minutes +/- (press and hold repeats).
    TimerAdjustMin(i32),
    /// Countdown seconds +/- (press and hold repeats).
    TimerAdjustSec(i32),
    TimerStartPause,
    TimerReset,
    TimerLap,
    TimerFullscreen,
    TimerSound,
    TimerDismissAlarm,
    PanelDrag(PanelKind),
    ClosePanel(PanelKind),
    // remote
    RemoteRetryHotspot,
    RemoteStop,
    /// Show/hide the phone panel (status icon).
    RemotePanel,
    // recording
    ToggleRecording,
    // presentation
    ShowStart,
    ShowNext,
    ShowPrev,
    ShowExit,
}

impl Action {
    /// Actions that fire on press and repeat while held.
    pub fn repeats(&self) -> bool {
        matches!(self, Action::TimerAdjustMin(_) | Action::TimerAdjustSec(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    More,
    Pen,
    Eraser,
    Assist,
    Selection,
    Pages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    Timer,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtnStyle {
    /// Toolbar button inside the glass bar (icon over label).
    Tool,
    /// Standalone square button on the left of the bar.
    Side,
    Danger,
    Primary,
    Ghost,
    /// Compact text/icon button in popups.
    Chip,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WKind {
    /// Background card (non-interactive unless action set).
    Panel { radius: f32, elevated: bool },
    GlassBar { radius: f32 },
    Button { icon: Option<Icon>, label: String, style: BtnStyle, active: bool, caret: bool, dot: Option<Color>, enabled: bool },
    Swatch { color: Color, selected: bool },
    WidthDot { width: f32, selected: bool, color: Color },
    LineSample { style: cb_core::doc::LineStyle, selected: bool },
    Slider { value: f32, min: f32, max: f32, label: String },
    Label { text: String, size: f32, weight: u16, color: Option<Color>, align: u8 },
    Toggle { on: bool, label: String },
    Radio { on: bool, label: String },
    Progress { frac: f32 },
    Qr { n: usize, cells: Vec<bool> },
    /// Drag the knob to the end to confirm (e.g. clearing a page).
    SlideConfirm { progress: f32, label: String },
    /// Clock digits drawn in fixed-width cells so they never jitter.
    Digits { text: String, size: f32, color: Color },
    /// Small round floating status button (icon + coloured status dot).
    StatusDot { icon: Icon, color: Color },
    Separator,
    Scrim,
    /// Invisible interactive area (drag handles).
    Area,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub rect: Rect,
    pub kind: WKind,
    pub action: Action,
}

impl Widget {
    pub fn new(rect: Rect, kind: WKind, action: Action) -> Widget {
        Widget { rect, kind, action }
    }
    pub fn interactive(&self) -> bool {
        match &self.kind {
            WKind::Button { enabled, .. } => *enabled && self.action != Action::None,
            WKind::Label { .. } | WKind::Separator | WKind::Progress { .. } | WKind::Qr { .. } | WKind::GlassBar { .. } | WKind::Digits { .. } => false,
            _ => self.action != Action::None,
        }
    }
    /// Panels/scrims swallow input even without an action.
    pub fn blocks(&self) -> bool {
        matches!(self.kind, WKind::Panel { .. } | WKind::Scrim | WKind::GlassBar { .. }) || self.interactive()
    }
}

pub fn button(rect: Rect, icon: Option<Icon>, label: &str, style: BtnStyle, action: Action) -> Widget {
    Widget::new(rect, WKind::Button { icon, label: label.into(), style, active: false, caret: false, dot: None, enabled: true }, action)
}

pub fn label(rect: Rect, text: &str, size: f32, weight: u16, align: u8) -> Widget {
    Widget::new(rect, WKind::Label { text: text.into(), size, weight, color: None, align }, Action::None)
}

/// Index of the topmost interactive widget at (x, y) and whether any widget blocks there.
pub fn hit(widgets: &[Widget], x: f32, y: f32) -> (Option<usize>, bool) {
    let p = Vec2 { x, y };
    let mut blocked = false;
    for (i, w) in widgets.iter().enumerate().rev() {
        let r = if matches!(w.kind, WKind::Slider { .. } | WKind::SlideConfirm { .. }) { w.rect.inflate(6.0) } else { w.rect };
        if r.contains(p) {
            if w.interactive() {
                return (Some(i), true);
            }
            if w.blocks() {
                blocked = true;
                if matches!(w.kind, WKind::Scrim) {
                    return (None, true);
                }
            }
        }
    }
    (None, blocked)
}

pub struct DrawCtx<'a> {
    pub theme: &'a Theme,
    pub scale: f32,
    pub hover: Option<usize>,
    pub pressed: Option<usize>,
}

fn dim(c: Color, k: f32) -> Color {
    c.with_alpha((c.a as f32 * k) as u8)
}

pub fn draw(pm: &mut PixmapMut, fonts: &mut Fonts, widgets: &[Widget], cx: &DrawCtx) {
    let s = cx.scale;
    let th = cx.theme;
    for (i, w) in widgets.iter().enumerate() {
        let hovered = cx.hover == Some(i);
        let pressed = cx.pressed == Some(i);
        let r = w.rect;
        match &w.kind {
            WKind::Scrim => cb_render::paint::fill_rect(pm, r, th.scrim),
            WKind::Area => {}
            WKind::Panel { radius, elevated } => {
                if *elevated {
                    shadow(pm, r, radius * s, 18.0 * s, 6.0 * s, th.shadow);
                }
                fill_rrect(pm, r, radius * s, th.surface);
                stroke_rrect(pm, r.inflate(-0.5), radius * s, 1.0, th.border);
            }
            WKind::GlassBar { radius } => {
                shadow(pm, r, radius * s, 14.0 * s, 4.0 * s, th.shadow);
                fill_rrect_vgradient(pm, r, radius * s, th.bar_top, th.bar_bottom);
                stroke_rrect(pm, r.inflate(-0.5), radius * s, 1.0, th.bar_border);
                // Top highlight.
                let hl = Rect::new(r.x0 + radius * s, r.y0 + 1.0, r.x1 - radius * s, r.y0 + 2.0);
                cb_render::paint::fill_rect(pm, hl, Color::rgba(255, 255, 255, if th.dark { 22 } else { 120 }));
            }
            WKind::Separator => {
                cb_render::paint::fill_rect(pm, r, th.border);
            }
            WKind::Button { icon, label, style, active, caret, dot, enabled } => {
                let rad = match style {
                    BtnStyle::Tool | BtnStyle::Side | BtnStyle::Chip => 10.0 * s,
                    _ => 10.0 * s,
                };
                let (bg, fg) = match style {
                    BtnStyle::Primary => (if pressed { th.accent_strong } else if hovered { Theme::mix(th.accent, Color::rgb(255, 255, 255), 0.08) } else { th.accent }, th.on_accent),
                    BtnStyle::Danger => {
                        let base = th.danger;
                        (if pressed { Theme::mix(base, Color::rgb(0, 0, 0), 0.15) } else if hovered { Theme::mix(base, Color::rgb(255, 255, 255), 0.08) } else { base }, Color::rgb(255, 255, 255))
                    }
                    BtnStyle::Ghost => (if pressed { th.surface_hover } else if hovered { th.surface_alt } else { th.surface_alt.with_alpha(0) }, th.text),
                    _ if *active => (if pressed { th.accent_strong } else { th.accent }, th.on_accent),
                    BtnStyle::Side => (if pressed { th.surface_hover } else if hovered { th.surface_hover } else { th.surface_alt }, th.text),
                    _ => (if pressed { th.surface_hover } else if hovered { th.surface_alt } else { Color::rgba(0, 0, 0, 0) }, th.text),
                };
                let fg = if *enabled { fg } else { dim(fg, 0.35) };
                if matches!(style, BtnStyle::Side) {
                    shadow(pm, r, rad, 8.0 * s, 2.0 * s, th.shadow.with_alpha(th.shadow.a / 2));
                    fill_rrect_vgradient(pm, r, rad, th.bar_top, th.bar_bottom);
                    stroke_rrect(pm, r.inflate(-0.5), rad, 1.0, th.bar_border);
                }
                fill_rrect(pm, r, rad, bg);
                if matches!(style, BtnStyle::Ghost | BtnStyle::Chip) && !*active {
                    stroke_rrect(pm, r.inflate(-0.5), rad, 1.0, th.border);
                }
                let has_label = !label.is_empty();
                let vertical = matches!(style, BtnStyle::Tool | BtnStyle::Side) && icon.is_some() && has_label;
                let c = r.center();
                if vertical {
                    let isz = (r.h() * 0.40).min(22.0 * s);
                    let iy = r.y0 + r.h() * 0.38;
                    if let Some(ic) = icon {
                        icons::draw(pm, *ic, c.x, iy, isz, fg);
                    }
                    let st = TextStyle::new(10.5 * s, dim(fg, 0.9)).weight(500).center();
                    fonts.draw_vcenter(pm, label, c.x, r.y0 + r.h() * 0.79, st);
                } else if let Some(ic) = icon {
                    if has_label {
                        let st = TextStyle::new(13.0 * s, fg).weight(600);
                        let (tw, _) = fonts.measure(label, st);
                        let isz = 18.0 * s;
                        let total = isz + 6.0 * s + tw;
                        let x0 = c.x - total * 0.5;
                        icons::draw(pm, *ic, x0 + isz * 0.5, c.y, isz, fg);
                        fonts.draw_vcenter(pm, label, x0 + isz + 6.0 * s, c.y, st);
                    } else {
                        icons::draw(pm, *ic, c.x, c.y, (r.h().min(r.w()) * 0.5).min(22.0 * s), fg);
                    }
                } else {
                    let st = TextStyle::new(if matches!(style, BtnStyle::Tool) { 13.0 * s } else { 14.0 * s }, fg).weight(600).center();
                    fonts.draw_vcenter(pm, label, c.x, c.y, st);
                }
                if *caret {
                    let p = Vec2 { x: r.x1 - 7.0 * s, y: r.y0 + 8.0 * s };
                    icons::draw(pm, Icon::ChevronUp, p.x, p.y, 10.0 * s, dim(fg, 0.7));
                }
                if let Some(dc) = dot {
                    let p = if vertical { Vec2 { x: c.x + 12.0 * s, y: r.y0 + r.h() * 0.38 + 4.5 * s } } else { Vec2 { x: r.x1 - 8.0 * s, y: r.y1 - 9.0 * s } };
                    fill_circle(pm, p, 4.0 * s, *dc);
                    stroke_circle(pm, p, 4.0 * s, 1.2 * s, if *active { Color::rgba(255, 255, 255, 220) } else { th.border });
                }
            }
            WKind::Swatch { color, selected } => {
                let c = r.center();
                let rad = r.w().min(r.h()) * 0.5;
                if *selected {
                    stroke_circle(pm, c, rad - 1.0 * s, 2.0 * s, th.accent);
                }
                fill_circle(pm, c, rad - 4.5 * s, *color);
                stroke_circle(pm, c, rad - 4.5 * s, 1.0, th.border);
                if hovered && !*selected {
                    stroke_circle(pm, c, rad - 1.5 * s, 1.0 * s, th.border);
                }
            }
            WKind::WidthDot { width, selected, color } => {
                let c = r.center();
                fill_rrect(pm, r, 9.0 * s, if *selected { th.accent.with_alpha(48) } else if hovered { th.surface_alt } else { Color::rgba(0, 0, 0, 0) });
                if *selected {
                    stroke_rrect(pm, r.inflate(-0.5), 9.0 * s, 1.5 * s, th.accent);
                }
                let col = if color.r > 220 && color.g > 220 && color.b > 220 { th.text } else { *color };
                fill_circle(pm, c, (width * 0.5 * s).clamp(1.5 * s, r.h() * 0.36), col);
            }
            WKind::LineSample { style, selected } => {
                fill_rrect(pm, r, 9.0 * s, if *selected { th.accent.with_alpha(48) } else if hovered { th.surface_alt } else { Color::rgba(0, 0, 0, 0) });
                if *selected {
                    stroke_rrect(pm, r.inflate(-0.5), 9.0 * s, 1.5 * s, th.accent);
                }
                let (a, b) = (Vec2 { x: r.x0 + 10.0 * s, y: r.center().y }, Vec2 { x: r.x1 - 10.0 * s, y: r.center().y });
                match style {
                    cb_core::doc::LineStyle::Solid => cb_render::paint::line(pm, a, b, 2.5 * s, th.text),
                    cb_core::doc::LineStyle::Dashed => cb_render::paint::dashed_line(pm, a, b, 2.5 * s, 6.0 * s, th.text),
                    cb_core::doc::LineStyle::Dotted => {
                        let n = ((b.x - a.x) / (6.0 * s)) as i32;
                        for k in 0..=n {
                            fill_circle(pm, Vec2 { x: a.x + k as f32 * 6.0 * s, y: a.y }, 1.6 * s, th.text);
                        }
                    }
                }
            }
            WKind::Slider { value, min, max, label } => {
                let t = ((value - min) / (max - min)).clamp(0.0, 1.0);
                let cy = r.center().y;
                let track = Rect::new(r.x0, cy - 3.0 * s, r.x1, cy + 3.0 * s);
                fill_rrect(pm, track, 3.0 * s, th.surface_hover);
                let x = r.x0 + r.w() * t;
                fill_rrect(pm, Rect::new(r.x0, track.y0, x, track.y1), 3.0 * s, th.accent);
                fill_circle(pm, Vec2 { x, y: cy + 1.0 * s }, 11.0 * s, th.shadow);
                fill_circle(pm, Vec2 { x, y: cy }, 10.0 * s, Color::rgb(255, 255, 255));
                stroke_circle(pm, Vec2 { x, y: cy }, 10.0 * s, 2.0 * s, th.accent);
                if !label.is_empty() {
                    fonts.draw(pm, label, x, r.y1 + 2.0 * s, TextStyle::new(11.0 * s, th.text_dim).center());
                }
            }
            WKind::Label { text, size, weight, color, align } => {
                let col = color.unwrap_or(if *weight >= 600 { th.text } else { th.text_dim });
                let mut st = TextStyle::new(size * s, col).weight(*weight);
                let x = match align {
                    1 => {
                        st = st.center();
                        r.center().x
                    }
                    2 => {
                        st = st.right();
                        r.x1
                    }
                    _ => r.x0,
                };
                let mut y = r.y0;
                for line in text.split('\n') {
                    let (_, h) = fonts.draw(pm, line, x, y, st);
                    y += h.max(size * s * 1.25);
                }
            }
            WKind::Toggle { on, label } => {
                fonts.draw_vcenter(pm, label, r.x0, r.center().y, TextStyle::new(14.0 * s, th.text));
                let sw = Rect::new(r.x1 - 44.0 * s, r.center().y - 12.0 * s, r.x1, r.center().y + 12.0 * s);
                fill_rrect(pm, sw, 12.0 * s, if *on { th.accent } else { th.surface_hover });
                let kx = if *on { sw.x1 - 12.0 * s } else { sw.x0 + 12.0 * s };
                fill_circle(pm, Vec2 { x: kx, y: sw.center().y }, 9.5 * s, Color::rgb(255, 255, 255));
            }
            WKind::Radio { on, label } => {
                fill_rrect(pm, r, 10.0 * s, if *on { th.accent.with_alpha(40) } else if hovered { th.surface_alt } else { Color::rgba(0, 0, 0, 0) });
                stroke_rrect(pm, r.inflate(-0.5), 10.0 * s, if *on { 1.5 * s } else { 1.0 }, if *on { th.accent } else { th.border });
                fonts.draw_vcenter(pm, label, r.center().x, r.center().y, TextStyle::new(13.5 * s, th.text).weight(if *on { 600 } else { 400 }).center());
            }
            WKind::Progress { frac } => {
                fill_rrect(pm, r, r.h() * 0.5, th.surface_hover);
                let w = r.w() * frac.clamp(0.0, 1.0);
                if w > 1.0 {
                    fill_rrect(pm, Rect::new(r.x0, r.y0, r.x0 + w.max(r.h()), r.y1), r.h() * 0.5, th.accent);
                }
            }
            WKind::SlideConfirm { progress, label } => {
                let h = r.h();
                let p = progress.clamp(0.0, 1.0);
                fill_rrect(pm, r, h * 0.5, th.surface_alt);
                stroke_rrect(pm, r.inflate(-0.5), h * 0.5, 1.0, th.border);
                let kx = r.x0 + (r.w() - h) * p;
                if p > 0.0 {
                    fill_rrect(pm, Rect::new(r.x0, r.y0, kx + h, r.y1), h * 0.5, th.danger.with_alpha((60.0 + 120.0 * p) as u8));
                }
                let tc = dim(th.text_dim, (1.0 - p * 1.6).max(0.0));
                fonts.draw_vcenter(pm, label, r.center().x + h * 0.25, r.center().y, TextStyle::new(13.5 * s, tc).weight(500).center());
                let kc = Vec2 { x: kx + h * 0.5, y: r.center().y };
                fill_circle(pm, Vec2 { x: kc.x, y: kc.y + 1.5 * s }, h * 0.5 - 3.0 * s, th.shadow);
                fill_circle(pm, kc, h * 0.5 - 3.0 * s, if pressed || p > 0.0 { Theme::mix(th.danger, Color::rgb(0, 0, 0), 0.08) } else { th.danger });
                icons::draw(pm, if p >= 0.999 { Icon::Check } else { Icon::ChevronsRight }, kc.x, kc.y, h * 0.42, Color::rgb(255, 255, 255));
            }
            WKind::Digits { text, size, color } => {
                let px = size * s;
                let cell = |c: char| if c.is_ascii_digit() { px * 0.62 } else { px * 0.34 };
                let total: f32 = text.chars().map(cell).sum();
                let mut x = r.center().x - total * 0.5;
                let st = TextStyle::new(px, *color).weight(700).center();
                let mut buf = [0u8; 4];
                for c in text.chars() {
                    let w = cell(c);
                    fonts.draw_vcenter(pm, c.encode_utf8(&mut buf), x + w * 0.5, r.center().y, st);
                    x += w;
                }
            }
            WKind::StatusDot { icon, color } => {
                let c = r.center();
                let rad = r.w().min(r.h()) * 0.5;
                fill_circle(pm, Vec2 { x: c.x, y: c.y + 2.0 * s }, rad + 1.0 * s, th.shadow);
                fill_circle(pm, c, rad, if hovered || pressed { th.surface_hover } else { th.surface });
                stroke_circle(pm, c, rad - 0.5, 1.0, th.border);
                icons::draw(pm, *icon, c.x, c.y, rad * 1.0, th.text);
                let d = Vec2 { x: c.x + rad * 0.62, y: c.y - rad * 0.62 };
                fill_circle(pm, d, 5.5 * s, *color);
                stroke_circle(pm, d, 5.5 * s, 1.6 * s, th.surface);
            }
            WKind::Qr { n, cells } => {
                fill_rrect(pm, r, 8.0 * s, Color::rgb(255, 255, 255));
                let quiet = 3.0;
                let m = r.w().min(r.h()) / (*n as f32 + quiet * 2.0);
                let ox = r.x0 + (r.w() - m * *n as f32) * 0.5;
                let oy = r.y0 + (r.h() - m * *n as f32) * 0.5;
                for y in 0..*n {
                    for x in 0..*n {
                        if cells[y * n + x] {
                            let q = Rect::new((ox + x as f32 * m).floor(), (oy + y as f32 * m).floor(), (ox + (x + 1) as f32 * m).ceil(), (oy + (y + 1) as f32 * m).ceil());
                            cb_render::paint::fill_rect(pm, q, Color::rgb(0, 0, 0));
                        }
                    }
                }
            }
        }
    }
}
