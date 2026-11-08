//! Chrome layout: bottom bar (legacy arrangement), popups, dialogs and
//! floating panels. Produces the widget list for drawing and hit testing.

use crate::app::{App, Dialog, HotspotUi, TimerTab, Tool, UpdateUi};
use crate::widgets::{button, label, Action, BtnStyle, PanelKind, PopupKind, WKind, Widget};
use cb_core::assist::AssistKind;
use cb_core::doc::LineStyle;
use cb_core::geom::Rect;
use cb_core::settings::ThemeMode;
use cb_core::timer::{format_countdown, format_stopwatch, RunState};
use cb_render::icons::Icon;
use cb_render::theme::{metrics, ACCENTS, PEN_COLORS, PEN_WIDTHS};

/// Status colour for a connected phone.
pub const REMOTE_ON: cb_core::Color = cb_core::Color::rgb(34, 166, 110);

/// Zoom range of the slider.
pub const ZOOM_MIN: f32 = 0.25;
pub const ZOOM_MAX: f32 = 4.0;

struct Bar {
    widgets: Vec<Widget>,
    anchors: Vec<(PopupKind, Rect)>,
    top: f32,
}

impl App {
    pub(crate) fn build_widgets(&self) -> Vec<Widget> {
        let mut out = Vec::new();
        let fullscreen_timer = self.timer.as_ref().is_some_and(|t| t.fullscreen);
        let bar = self.layout_bar();
        if !fullscreen_timer {
            out.extend(bar.widgets);
        }
        if let Some(t) = self.recording_elapsed() {
            // Recording indicator (top left): click to stop.
            let s = self.scale;
            let secs = t.as_secs();
            let label = format!("停止录制  {:02}:{:02}", secs / 60, secs % 60);
            out.push(button(Rect::xywh(18.0 * s, 18.0 * s, 176.0 * s, 44.0 * s), Some(Icon::Stop), &label, BtnStyle::Danger, Action::ToggleRecording));
        }
        if let Some(p) = self.popup {
            let anchor = bar.anchors.iter().find(|(k, _)| *k == p).map(|(_, r)| *r);
            out.extend(self.layout_popup(p, anchor, bar.top));
        }
        if let Some(t) = &self.timer {
            if t.fullscreen {
                out.extend(self.layout_timer_fullscreen());
            } else {
                out.extend(self.layout_timer());
            }
        }
        if self.remote.is_some() {
            out.extend(self.layout_remote());
        }
        if let Some(d) = &self.dialog {
            out.extend(self.layout_dialog(d));
        }
        out
    }

    fn layout_bar(&self) -> Bar {
        let s = self.scale;
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let bar_h = metrics::BAR_H * s;
        let top = h - bar_h;
        let pad = metrics::BAR_MARGIN * s;
        let y0 = top + pad;
        let y1 = h - pad;
        let mut ws = Vec::new();
        let mut anchors = Vec::new();

        // Left: exit.
        let side_w = metrics::SIDE_BTN_W * s;
        let exit_r = Rect::new(8.0 * s, y0, 8.0 * s + side_w, y1);
        ws.push(button(exit_r, Some(Icon::Exit), "退出", BtnStyle::Side, Action::Exit));

        // Right: page navigation in its own glass pill.
        let pdf = self.is_pdf();
        let cur = self.display_page();
        let n = self.page_count();
        let nb = 40.0 * s;
        let lbl_w = 64.0 * s;
        let nav_h = 40.0 * s;
        let ny0 = (y0 + y1) * 0.5 - nav_h * 0.5;
        let nav_w = nb * 2.0 + lbl_w + 8.0 * s + 12.0 * s;
        let nav_r = Rect::new(w - pad - nav_w, y0, w - pad, y1);
        ws.push(Widget::new(nav_r, WKind::GlassBar { radius: metrics::RADIUS_L }, Action::None));
        let mut rx = nav_r.x0 + 6.0 * s;
        let mut prev = button(Rect::new(rx, ny0, rx + nb, ny0 + nav_h), Some(Icon::ChevronLeft), "", BtnStyle::Tool, Action::PrevPage);
        if let WKind::Button { enabled, .. } = &mut prev.kind {
            *enabled = cur > 0;
        }
        ws.push(prev);
        rx += nb + 4.0 * s;
        let r = Rect::new(rx, ny0, rx + lbl_w, ny0 + nav_h);
        ws.push(button(r, None, &format!("{}/{}", cur + 1, n), BtnStyle::Tool, Action::TogglePopup(PopupKind::Pages)));
        anchors.push((PopupKind::Pages, r));
        rx += lbl_w + 4.0 * s;
        let mut next = button(Rect::new(rx, ny0, rx + nb, ny0 + nav_h), Some(Icon::ChevronRight), "", BtnStyle::Tool, Action::NextPage);
        if let WKind::Button { enabled, .. } = &mut next.kind {
            *enabled = !pdf || cur + 1 < n;
        }
        ws.push(next);

        // Centre dock: the everyday tools plus "更多".
        let more_active = self.popup == Some(PopupKind::More) || self.popup == Some(PopupKind::Assist);
        let hidden_busy = self.timer.is_some() || !self.assist.tools.is_empty() || self.remote.is_some() || self.recording_active() || matches!(self.tool, Tool::Pan | Tool::Text);
        let tools: Vec<(Icon, &str, Action, bool, bool, Option<PopupKind>)> = vec![
            (if self.highlighter { Icon::Highlighter } else { Icon::Pen }, "画笔", Action::SetTool(Tool::Pen), self.tool == Tool::Pen, true, Some(PopupKind::Pen)),
            (Icon::Eraser, "橡皮", Action::SetTool(Tool::Eraser), self.tool == Tool::Eraser, true, Some(PopupKind::Eraser)),
            (Icon::Select, "选择", Action::SetTool(Tool::Select), self.tool == Tool::Select, false, None),
            (Icon::Undo, "撤销", Action::Undo, false, false, None),
            (Icon::Redo, "重做", Action::Redo, false, false, None),
            (Icon::More, "更多", Action::TogglePopup(PopupKind::More), more_active, false, Some(PopupKind::More)),
        ];
        let sep = 10.0 * s;
        let free = (nav_r.x0 - exit_r.x1 - 24.0 * s).max(200.0 * s);
        let mut bw = 64.0 * s;
        let inner = |bw: f32| tools.len() as f32 * bw + 2.0 * sep + 2.0 * 3.0 * s;
        if inner(bw) + 16.0 * s > free {
            bw = ((free - 16.0 * s - 2.0 * sep - 6.0 * s) / tools.len() as f32).max(40.0 * s);
        }
        let labels = bw >= 48.0 * s;
        let dock_w = inner(bw) + 16.0 * s;
        let cx = (w * 0.5).clamp(exit_r.x1 + 12.0 * s + dock_w * 0.5, nav_r.x0 - 12.0 * s - dock_w * 0.5);
        let dock = Rect::new(cx - dock_w * 0.5, y0, cx + dock_w * 0.5, y1);
        ws.push(Widget::new(dock, WKind::GlassBar { radius: metrics::RADIUS_L }, Action::None));
        let mut x = dock.x0 + 8.0 * s;
        let by0 = y0 + 4.0 * s;
        let by1 = y1 - 4.0 * s;
        for (i, (icon, lab, act, active, caret, anchor)) in tools.into_iter().enumerate() {
            if i == 3 || i == 5 {
                // Group separators: [pen eraser select] | [undo redo] | [more]
                ws.push(Widget::new(Rect::new(x + sep * 0.5 - 0.5, by0 + 10.0 * s, x + sep * 0.5 + 0.5, by1 - 10.0 * s), WKind::Separator, Action::None));
                x += sep;
            }
            let r = Rect::new(x, by0, x + bw, by1);
            let enabled = match act {
                Action::Undo => self.history.can_undo(),
                Action::Redo => self.history.can_redo(),
                _ => true,
            };
            let dot = match act {
                Action::SetTool(Tool::Pen) => Some(self.pen_color),
                Action::TogglePopup(PopupKind::More) if hidden_busy && !active => Some(self.theme.accent),
                _ => None,
            };
            ws.push(Widget::new(r, WKind::Button { icon: Some(icon), label: if labels { lab.to_string() } else { String::new() }, style: BtnStyle::Tool, active, caret, dot, enabled }, act));
            if let Some(k) = anchor {
                anchors.push((k, r));
                if k == PopupKind::More {
                    anchors.push((PopupKind::Assist, r));
                }
            }
            x += bw + 3.0 * s;
        }
        if self.popup == Some(PopupKind::Selection) {
            let bb = self.selection_bounds();
            if !bb.is_empty() {
                let o = self.layout.origin(self.selection.page);
                let a = self.vp.to_screen(cb_core::v2(bb.x0, bb.y0) + o);
                let b = self.vp.to_screen(cb_core::v2(bb.x1, bb.y1) + o);
                anchors.push((PopupKind::Selection, Rect::from_points(a, b)));
            }
        }
        Bar { widgets: ws, anchors, top }
    }

    fn popup_rect(&self, w: f32, h: f32, anchor: Option<Rect>, bar_top: f32) -> Rect {
        let s = self.scale;
        let sw = self.size.0 as f32;
        let ax = anchor.map(|a| a.center().x).unwrap_or(sw * 0.5);
        let x = (ax - w * 0.5).clamp(8.0 * s, (sw - w - 8.0 * s).max(8.0 * s));
        let y = bar_top - metrics::POPUP_GAP * s - h;
        Rect::xywh(x, y, w, h)
    }

    fn layout_popup(&self, kind: PopupKind, anchor: Option<Rect>, bar_top: f32) -> Vec<Widget> {
        let s = self.scale;
        let mut v = Vec::new();
        let panel = |r: Rect| Widget::new(r, WKind::Panel { radius: metrics::RADIUS_M, elevated: true }, Action::None);
        let row_label = |x: f32, cy: f32, t: &str| label(Rect::new(x, cy - 9.0 * s, x + 44.0 * s, cy + 9.0 * s), t, 12.5, 500, 0);
        match kind {
            PopupKind::Pen => {
                let (w, h) = (392.0 * s, 210.0 * s);
                let r = self.popup_rect(w, h, anchor, bar_top);
                v.push(panel(r));
                let x0 = r.x0 + 16.0 * s;
                let lx = x0 + 48.0 * s;
                // Colours.
                let cy = r.y0 + 32.0 * s;
                v.push(row_label(x0, cy, "颜色"));
                for (i, c) in PEN_COLORS.iter().enumerate() {
                    let x = lx + i as f32 * 38.0 * s;
                    v.push(Widget::new(Rect::new(x, cy - 17.0 * s, x + 34.0 * s, cy + 17.0 * s), WKind::Swatch { color: *c, selected: *c == self.pen_color }, Action::PenColor(i)));
                }
                // Widths.
                let cy = r.y0 + 80.0 * s;
                v.push(row_label(x0, cy, "粗细"));
                for (i, wd) in PEN_WIDTHS.iter().enumerate() {
                    let x = lx + i as f32 * 46.0 * s;
                    v.push(Widget::new(Rect::new(x, cy - 18.0 * s, x + 42.0 * s, cy + 18.0 * s), WKind::WidthDot { width: wd + 2.0, selected: (*wd - self.pen_width).abs() < 0.01, color: self.pen_color }, Action::PenWidth(i)));
                }
                // Styles.
                let cy = r.y0 + 128.0 * s;
                v.push(row_label(x0, cy, "线型"));
                for (i, st) in [LineStyle::Solid, LineStyle::Dashed, LineStyle::Dotted].into_iter().enumerate() {
                    let x = lx + i as f32 * 70.0 * s;
                    v.push(Widget::new(Rect::new(x, cy - 17.0 * s, x + 64.0 * s, cy + 17.0 * s), WKind::LineSample { style: st, selected: st == self.pen_style && !self.highlighter }, Action::PenStyle(st)));
                }
                // Toggles.
                let cy = r.y0 + 176.0 * s;
                let mut hl = button(Rect::new(x0, cy - 17.0 * s, x0 + 116.0 * s, cy + 17.0 * s), Some(Icon::Highlighter), "荧光笔", BtnStyle::Chip, Action::ToggleHighlighter);
                if let WKind::Button { active, .. } = &mut hl.kind {
                    *active = self.highlighter;
                }
                v.push(hl);
                let mut sr = button(Rect::new(x0 + 124.0 * s, cy - 17.0 * s, x0 + 260.0 * s, cy + 17.0 * s), Some(Icon::Shapes), "图形识别", BtnStyle::Chip, Action::ToggleShapeRecognition);
                if let WKind::Button { active, .. } = &mut sr.kind {
                    *active = self.settings.shape_recognition;
                }
                v.push(sr);
            }
            PopupKind::Eraser => {
                let (w, h) = (360.0 * s, 166.0 * s);
                let r = self.popup_rect(w, h, anchor, bar_top);
                v.push(panel(r));
                let x0 = r.x0 + 16.0 * s;
                let cy = r.y0 + 30.0 * s;
                v.push(row_label(x0, cy, "大小"));
                let sz = self.settings.eraser_size;
                v.push(Widget::new(
                    Rect::new(x0 + 52.0 * s, cy - 12.0 * s, r.x1 - 20.0 * s, cy + 12.0 * s),
                    WKind::Slider { value: sz, min: 8.0, max: 120.0, label: format!("{:.0}", sz) },
                    Action::EraserSize,
                ));
                let cy = r.y0 + 84.0 * s;
                let bw = 100.0 * s;
                v.push(Widget::new(Rect::new(x0, cy - 17.0 * s, x0 + bw, cy + 17.0 * s), WKind::Radio { on: self.settings.eraser_split, label: "局部擦除".into() }, Action::EraserMode(true)));
                v.push(Widget::new(Rect::new(x0 + bw + 8.0 * s, cy - 17.0 * s, x0 + bw * 2.0 + 8.0 * s, cy + 17.0 * s), WKind::Radio { on: !self.settings.eraser_split, label: "整笔擦除".into() }, Action::EraserMode(false)));
                let cy = r.y0 + 128.0 * s;
                v.push(Widget::new(Rect::new(x0, cy - 21.0 * s, r.x1 - 16.0 * s, cy + 21.0 * s), WKind::SlideConfirm { progress: self.slide_progress(), label: "向右滑动，清空本页".into() }, Action::ClearPage));
            }
            PopupKind::More => {
                let n = self.page_count();
                let pdf = self.is_pdf();
                let items: Vec<(Icon, &str, Action, bool, bool)> = vec![
                    (Icon::FolderOpen, "打开", Action::Open, false, true),
                    (Icon::Save, "保存", Action::Save, false, true),
                    (Icon::Pan, "拖动", Action::SetTool(Tool::Pan), self.tool == Tool::Pan, true),
                    (Icon::Text, "文字", Action::SetTool(Tool::Text), self.tool == Tool::Text, true),
                    (Icon::SetSquare, "教具", Action::TogglePopup(PopupKind::Assist), !self.assist.tools.is_empty(), true),
                    (Icon::Timer, "计时", Action::ToggleTimer, self.timer.is_some(), true),
                    (Icon::Phone, "手机", Action::ToggleRemote, self.remote.is_some(), true),
                    (Icon::Record, "录制", Action::ToggleRecording, self.recording_active(), true),
                    (Icon::Center, "回中", Action::Recenter, false, true),
                    (Icon::Minimize, "最小化", Action::Minimize, false, true),
                    (Icon::Plus, "加页", Action::AddPage, false, !pdf),
                    (Icon::Minus, "删页", Action::DeletePage, false, !pdf && n > 1),
                    (Icon::Settings, "设置", Action::OpenSettings, false, true),
                ];
                let items: Vec<_> = items.into_iter().chain(self.more_extra_items()).collect();
                let cols = 5usize;
                let (tw, th) = (78.0 * s, 66.0 * s);
                let gap = 6.0 * s;
                let rows = items.len().div_ceil(cols);
                let grid_h = rows as f32 * th + (rows - 1) as f32 * gap;
                let w = 32.0 * s + cols as f32 * tw + (cols - 1) as f32 * gap;
                let h = 16.0 * s + grid_h + 18.0 * s + 40.0 * s + 16.0 * s + 46.0 * s + 16.0 * s;
                let r = self.popup_rect(w, h, anchor, bar_top);
                v.push(panel(r));
                let x0 = r.x0 + 16.0 * s;
                let x1 = r.x1 - 16.0 * s;
                for (i, (ic, t, act, active, enabled)) in items.into_iter().enumerate() {
                    let (cx, cy) = (i % cols, i / cols);
                    let x = x0 + cx as f32 * (tw + gap);
                    let y = r.y0 + 16.0 * s + cy as f32 * (th + gap);
                    let mut b = button(Rect::new(x, y, x + tw, y + th), Some(ic), t, BtnStyle::Tool, act);
                    if let WKind::Button { active: a, enabled: e, .. } = &mut b.kind {
                        *a = active;
                        *e = enabled;
                    }
                    v.push(b);
                }
                // Zoom slider (logarithmic, 25%..400%).
                let cy = r.y0 + 16.0 * s + grid_h + 18.0 * s + 20.0 * s;
                v.push(row_label(x0, cy, "缩放"));
                let chip_w = 56.0 * s;
                let pct_w = 52.0 * s;
                let sx0 = x0 + 46.0 * s;
                let sx1 = x1 - chip_w * 2.0 - pct_w - 16.0 * s;
                let z = self.vp.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
                v.push(Widget::new(Rect::new(sx0, cy - 12.0 * s, sx1, cy + 12.0 * s), WKind::Slider { value: z.ln(), min: ZOOM_MIN.ln(), max: ZOOM_MAX.ln(), label: String::new() }, Action::ZoomSlider));
                v.push(label(Rect::new(sx1 + 4.0 * s, cy - 10.0 * s, sx1 + 4.0 * s + pct_w, cy + 10.0 * s), &format!("{:.0}%", self.vp.zoom * 100.0), 13.5, 600, 1));
                let cx0 = x1 - chip_w * 2.0 - 6.0 * s;
                v.push(button(Rect::new(cx0, cy - 16.0 * s, cx0 + chip_w, cy + 16.0 * s), None, "100%", BtnStyle::Chip, Action::SetZoom(1.0)));
                v.push(button(Rect::new(x1 - chip_w, cy - 16.0 * s, x1, cy + 16.0 * s), None, "适应", BtnStyle::Chip, Action::ZoomFit));
                // Clear page: slide to confirm.
                let sy = cy + 20.0 * s + 16.0 * s;
                v.push(Widget::new(Rect::new(x0, sy, x1, sy + 46.0 * s), WKind::SlideConfirm { progress: self.slide_progress(), label: "向右滑动，清空本页".into() }, Action::ClearPage));
            }
            PopupKind::Assist => {
                let items = [
                    (AssistKind::Ruler, Icon::Ruler),
                    (AssistKind::Triangle45, Icon::SetSquare),
                    (AssistKind::Triangle30, Icon::SetSquare),
                    (AssistKind::Protractor, Icon::Protractor),
                    (AssistKind::Compass, Icon::Compass),
                ];
                let bw = 92.0 * s;
                let w = 32.0 * s + items.len() as f32 * (bw + 6.0 * s) - 6.0 * s;
                let h = 128.0 * s;
                let r = self.popup_rect(w, h, anchor, bar_top);
                v.push(panel(r));
                let mut x = r.x0 + 16.0 * s;
                let y = r.y0 + 14.0 * s;
                for (k, ic) in items {
                    let mut b = button(Rect::new(x, y, x + bw, y + 62.0 * s), Some(ic), k.label(), BtnStyle::Tool, Action::ToggleAssist(k));
                    if let WKind::Button { active, .. } = &mut b.kind {
                        *active = self.assist.is_shown(k);
                    }
                    v.push(b);
                    x += bw + 6.0 * s;
                }
                let y = r.y0 + 86.0 * s;
                v.push(label(Rect::new(r.x0 + 16.0 * s, y, r.x1 - 140.0 * s, y + 28.0 * s), "拖动移动，旋转手柄旋转，双指缩放", 12.0, 400, 0));
                v.push(button(Rect::new(r.x1 - 124.0 * s, y - 2.0 * s, r.x1 - 16.0 * s, y + 30.0 * s), Some(Icon::Close), "全部隐藏", BtnStyle::Chip, Action::HideAssists));
            }
            PopupKind::Selection => {
                let (w, h) = (348.0 * s, 104.0 * s);
                let sw = self.size.0 as f32;
                let r = match anchor {
                    Some(a) => {
                        let x = (a.center().x - w * 0.5).clamp(8.0 * s, sw - w - 8.0 * s);
                        let mut y = a.y0 - 12.0 * s - h;
                        if y < 8.0 * s {
                            y = (a.y1 + 12.0 * s).min(bar_top - h - 8.0 * s);
                        }
                        Rect::xywh(x, y, w, h)
                    }
                    None => self.popup_rect(w, h, None, bar_top),
                };
                v.push(panel(r));
                let x0 = r.x0 + 12.0 * s;
                let y = r.y0 + 12.0 * s;
                let bw = 102.0 * s;
                v.push(button(Rect::new(x0, y, x0 + bw, y + 36.0 * s), Some(Icon::Copy), "复制", BtnStyle::Primary, Action::SelCopy));
                v.push(button(Rect::new(x0 + bw + 8.0 * s, y, x0 + bw * 2.0 + 8.0 * s, y + 36.0 * s), Some(Icon::Trash), "删除", BtnStyle::Danger, Action::SelDelete));
                v.push(button(Rect::new(x0 + bw * 2.0 + 16.0 * s, y, r.x1 - 12.0 * s, y + 36.0 * s), Some(Icon::Close), "取消", BtnStyle::Ghost, Action::SetTool(Tool::Select)));
                let cy = r.y0 + 74.0 * s;
                for (i, c) in PEN_COLORS.iter().enumerate() {
                    let x = x0 + i as f32 * 40.0 * s;
                    v.push(Widget::new(Rect::new(x, cy - 17.0 * s, x + 34.0 * s, cy + 17.0 * s), WKind::Swatch { color: *c, selected: false }, Action::SelColor(i)));
                }
            }
            PopupKind::Pages => {
                let n = self.page_count();
                let cur = self.display_page();
                let cols = 8usize;
                let max_show = 48usize;
                let start = if n > max_show { cur.saturating_sub(max_show / 2).min(n - max_show) } else { 0 };
                let shown = n.min(max_show);
                let rows = shown.div_ceil(cols);
                let bw = 44.0 * s;
                let w = 32.0 * s + cols.min(shown) as f32 * (bw + 6.0 * s) - 6.0 * s;
                let h = 48.0 * s + rows as f32 * (36.0 * s + 6.0 * s);
                let r = self.popup_rect(w.max(220.0 * s), h, anchor, bar_top);
                v.push(panel(r));
                v.push(label(Rect::new(r.x0 + 16.0 * s, r.y0 + 12.0 * s, r.x1 - 16.0 * s, r.y0 + 32.0 * s), &format!("跳转页面（共 {n} 页）"), 13.0, 600, 0));
                for i in 0..shown {
                    let p = start + i;
                    let (cx, cy) = (i % cols, i / cols);
                    let x = r.x0 + 16.0 * s + cx as f32 * (bw + 6.0 * s);
                    let y = r.y0 + 40.0 * s + cy as f32 * (42.0 * s);
                    let mut b = button(Rect::new(x, y, x + bw, y + 36.0 * s), None, &(p + 1).to_string(), BtnStyle::Chip, Action::DialogButton(10_000 + p as u32));
                    if let WKind::Button { active, .. } = &mut b.kind {
                        *active = p == cur;
                    }
                    v.push(b);
                }
            }
        }
        v
    }

    fn dialog_frame(&self, w: f32, h: f32, title: &str, v: &mut Vec<Widget>) -> Rect {
        let s = self.scale;
        let (sw, sh) = (self.size.0 as f32, self.size.1 as f32);
        let w = w.min(sw - 24.0 * s);
        let h = h.min(sh - 24.0 * s);
        v.push(Widget::new(Rect::new(0.0, 0.0, sw, sh), WKind::Scrim, Action::None));
        let r = Rect::xywh((sw - w) * 0.5, (sh - h) * 0.5, w, h);
        v.push(Widget::new(r, WKind::Panel { radius: metrics::RADIUS_L, elevated: true }, Action::None));
        v.push(label(Rect::new(r.x0 + 24.0 * s, r.y0 + 20.0 * s, r.x1 - 60.0 * s, r.y0 + 44.0 * s), title, metrics::TITLE, 700, 0));
        v.push(button(Rect::new(r.x1 - 52.0 * s, r.y0 + 14.0 * s, r.x1 - 16.0 * s, r.y0 + 50.0 * s), Some(Icon::Close), "", BtnStyle::Ghost, Action::CloseDialog));
        r
    }

    fn layout_dialog(&self, d: &Dialog) -> Vec<Widget> {
        let s = self.scale;
        let mut v = Vec::new();
        match d {
            Dialog::Confirm { title, message, buttons } => {
                let stacked = buttons.len() > 2;
                let h = if stacked { 150.0 + buttons.len() as f32 * 50.0 } else { 196.0 } * s;
                let r = self.dialog_frame(400.0 * s, h, title, &mut v);
                v.push(label(Rect::new(r.x0 + 24.0 * s, r.y0 + 64.0 * s, r.x1 - 24.0 * s, r.y0 + 110.0 * s), message, metrics::BODY, 400, 0));
                if stacked {
                    let mut y = r.y0 + 112.0 * s;
                    for (id, t, st) in buttons {
                        v.push(button(Rect::new(r.x0 + 24.0 * s, y, r.x1 - 24.0 * s, y + 42.0 * s), None, t, *st, Action::DialogButton(*id)));
                        y += 50.0 * s;
                    }
                } else {
                    let bw = 120.0 * s;
                    let mut x = r.x1 - 24.0 * s;
                    for (id, t, st) in buttons.iter().rev() {
                        v.push(button(Rect::new(x - bw, r.y1 - 64.0 * s, x, r.y1 - 22.0 * s), None, t, *st, Action::DialogButton(*id)));
                        x -= bw + 10.0 * s;
                    }
                }
            }
            Dialog::Message { title, text } => {
                let lines = text.lines().count().max(1) as f32;
                let r = self.dialog_frame(460.0 * s, (150.0 + lines * 22.0) * s, title, &mut v);
                v.push(label(Rect::new(r.x0 + 24.0 * s, r.y0 + 64.0 * s, r.x1 - 24.0 * s, r.y1 - 70.0 * s), text, metrics::BODY, 400, 0));
                v.push(button(Rect::new(r.x1 - 144.0 * s, r.y1 - 62.0 * s, r.x1 - 24.0 * s, r.y1 - 20.0 * s), None, "确定", BtnStyle::Primary, Action::CloseDialog));
            }
            Dialog::Settings => {
                let r = self.dialog_frame(520.0 * s, 650.0 * s, "设置", &mut v);
                let x0 = r.x0 + 24.0 * s;
                let x1 = r.x1 - 24.0 * s;
                let mut y = r.y0 + 66.0 * s;
                v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), "界面颜色", 13.0, 600, 0));
                y += 26.0 * s;
                let bw = (x1 - x0 - 16.0 * s) / 3.0;
                for (i, (m, t)) in [(ThemeMode::Dark, "深色"), (ThemeMode::Light, "浅色"), (ThemeMode::System, "跟随系统")].into_iter().enumerate() {
                    let x = x0 + i as f32 * (bw + 8.0 * s);
                    v.push(Widget::new(Rect::new(x, y, x + bw, y + 38.0 * s), WKind::Radio { on: self.settings.theme_mode == m, label: t.into() }, Action::Theme(m)));
                }
                y += 54.0 * s;
                v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), "强调色", 13.0, 600, 0));
                y += 26.0 * s;
                for (i, c) in ACCENTS.iter().enumerate() {
                    let x = x0 + i as f32 * 46.0 * s;
                    v.push(Widget::new(Rect::new(x, y, x + 38.0 * s, y + 38.0 * s), WKind::Swatch { color: *c, selected: i == self.settings.accent as usize }, Action::Accent(i)));
                }
                y += 56.0 * s;
                v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), "录制分辨率", 13.0, 600, 0));
                y += 26.0 * s;
                let rw = (x1 - x0 - 24.0 * s) / 4.0;
                for (i, res) in cb_core::settings::RecordResolution::ALL.into_iter().enumerate() {
                    let x = x0 + i as f32 * (rw + 8.0 * s);
                    v.push(Widget::new(Rect::new(x, y, x + rw, y + 38.0 * s), WKind::Radio { on: self.settings.record_resolution == res, label: res.label().into() }, Action::RecordRes(res)));
                }
                y += 56.0 * s;
                let toggles = [
                    ("使用笔压（数位笔）", self.settings.pressure, Action::TogglePressure),
                    ("手绘图形自动识别", self.settings.shape_recognition, Action::ToggleShapeRecognition),
                    ("自动检查更新", self.settings.auto_check_update, Action::ToggleAutoUpdate),
                ];
                for (t, on, a) in toggles {
                    v.push(Widget::new(Rect::new(x0, y, x1, y + 36.0 * s), WKind::Toggle { on, label: t.into() }, a));
                    y += 42.0 * s;
                }
                y += 8.0 * s;
                v.push(Widget::new(Rect::new(x0, y, x1, y + 1.0), WKind::Separator, Action::None));
                y += 16.0 * s;
                v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), &format!("{}，版本 {}", cb_core::version::brand_title(), cb_core::version::APP_VERSION), 13.0, 600, 0));
                v.push(button(Rect::new(x1 - 150.0 * s, y - 8.0 * s, x1, y + 28.0 * s), Some(Icon::Download), "检查更新", BtnStyle::Chip, Action::CheckUpdate));
                v.push(label(Rect::new(x0, y + 24.0 * s, x1 - 160.0 * s, y + 44.0 * s), "更新来源：GitHub 2001020/cloudboard", 12.0, 400, 0));
                v.push(button(Rect::new(x0, r.y1 - 64.0 * s, x0 + 150.0 * s, r.y1 - 22.0 * s), Some(Icon::Reset), "重置设置", BtnStyle::Ghost, Action::ResetSettings));
                v.push(button(Rect::new(x1 - 120.0 * s, r.y1 - 64.0 * s, x1, r.y1 - 22.0 * s), None, "完成", BtnStyle::Primary, Action::CloseDialog));
            }
            Dialog::Update(u) => {
                let r = self.dialog_frame(480.0 * s, 300.0 * s, "软件更新", &mut v);
                let x0 = r.x0 + 24.0 * s;
                let x1 = r.x1 - 24.0 * s;
                let y = r.y0 + 66.0 * s;
                let by0 = r.y1 - 64.0 * s;
                let by1 = r.y1 - 22.0 * s;
                match u {
                    UpdateUi::Checking => {
                        v.push(label(Rect::new(x0, y, x1, y + 40.0 * s), "正在检查 GitHub 上的新版本…", metrics::BODY, 400, 0));
                        v.push(Widget::new(Rect::new(x0, y + 50.0 * s, x1, y + 58.0 * s), WKind::Progress { frac: ((self.now.elapsed().as_millis() % 1000) as f32) / 1000.0 }, Action::None));
                    }
                    UpdateUi::UpToDate => {
                        v.push(label(Rect::new(x0, y, x1, y + 60.0 * s), &format!("已是最新版本（{}）。", cb_core::version::APP_VERSION), metrics::BODY, 400, 0));
                        v.push(button(Rect::new(x1 - 120.0 * s, by0, x1, by1), None, "好的", BtnStyle::Primary, Action::CloseDialog));
                    }
                    UpdateUi::Available(rel) => {
                        let mb = rel.asset.size as f32 / 1_048_576.0;
                        v.push(label(Rect::new(x0, y, x1, y + 22.0 * s), &format!("发现新版本 {}（{:.1} MB）", rel.version, mb), 15.0, 600, 0));
                        let notes = if rel.notes.is_empty() { "性能与稳定性改进".to_string() } else { rel.notes.iter().take(5).cloned().collect::<Vec<_>>().join("\n") };
                        v.push(label(Rect::new(x0, y + 32.0 * s, x1, by0 - 8.0 * s), &notes, 13.0, 400, 0));
                        v.push(button(Rect::new(x1 - 140.0 * s, by0, x1, by1), Some(Icon::Download), "下载更新", BtnStyle::Primary, Action::DownloadUpdate));
                        if !rel.mandatory {
                            v.push(button(Rect::new(x1 - 270.0 * s, by0, x1 - 150.0 * s, by1), None, "跳过此版本", BtnStyle::Ghost, Action::SkipUpdate));
                        }
                    }
                    UpdateUi::Downloading { done, total, version } => {
                        let frac = if *total > 0 { *done as f32 / *total as f32 } else { 0.0 };
                        v.push(label(Rect::new(x0, y, x1, y + 22.0 * s), &format!("正在下载 {version}… {:.0}%", frac * 100.0), metrics::BODY, 400, 0));
                        v.push(Widget::new(Rect::new(x0, y + 40.0 * s, x1, y + 50.0 * s), WKind::Progress { frac }, Action::None));
                        v.push(label(Rect::new(x0, y + 60.0 * s, x1, y + 80.0 * s), &format!("{:.1} / {:.1} MB", *done as f32 / 1_048_576.0, *total as f32 / 1_048_576.0), 12.0, 400, 0));
                        v.push(button(Rect::new(x1 - 120.0 * s, by0, x1, by1), None, "取消", BtnStyle::Ghost, Action::CancelUpdate));
                    }
                    UpdateUi::Ready { version, .. } => {
                        v.push(label(Rect::new(x0, y, x1, y + 60.0 * s), &format!("{version} 已下载并通过校验。\n安装时 CloudBoard 会自动重启。"), metrics::BODY, 400, 0));
                        v.push(button(Rect::new(x1 - 160.0 * s, by0, x1, by1), None, "立即重启安装", BtnStyle::Primary, Action::InstallUpdate));
                        v.push(button(Rect::new(x1 - 270.0 * s, by0, x1 - 170.0 * s, by1), None, "稍后", BtnStyle::Ghost, Action::CloseDialog));
                    }
                    UpdateUi::Failed(e) => {
                        v.push(label(Rect::new(x0, y, x1, by0 - 8.0 * s), &format!("更新失败：{e}"), metrics::BODY, 400, 0));
                        v.push(button(Rect::new(x1 - 120.0 * s, by0, x1, by1), None, "重试", BtnStyle::Primary, Action::CheckUpdate));
                    }
                }
            }
        }
        v
    }

    fn layout_timer(&self) -> Vec<Widget> {
        let s = self.scale;
        let t = self.timer.as_ref().unwrap();
        let mut v = Vec::new();
        let (w, h) = (360.0 * s, 380.0 * s);
        let r = Rect::xywh(t.pos.x, t.pos.y, w, h);
        let alarm = t.alarm_since.map(|a| (self.now.duration_since(a).as_millis() / 400) % 2 == 0).unwrap_or(false);
        v.push(Widget::new(r, WKind::Panel { radius: metrics::RADIUS_L, elevated: true }, Action::None));
        // Header (drag handle).
        v.push(drag_handle(Rect::new(r.x0, r.y0, r.x1 - 96.0 * s, r.y0 + 48.0 * s), PanelKind::Timer));
        v.push(label(Rect::new(r.x0 + 18.0 * s, r.y0 + 15.0 * s, r.x0 + 120.0 * s, r.y0 + 35.0 * s), "计时器", 15.0, 700, 0));
        v.push(button(Rect::new(r.x1 - 92.0 * s, r.y0 + 8.0 * s, r.x1 - 56.0 * s, r.y0 + 44.0 * s), Some(Icon::Fullscreen), "", BtnStyle::Ghost, Action::TimerFullscreen));
        v.push(button(Rect::new(r.x1 - 48.0 * s, r.y0 + 8.0 * s, r.x1 - 12.0 * s, r.y0 + 44.0 * s), Some(Icon::Close), "", BtnStyle::Ghost, Action::ClosePanel(PanelKind::Timer)));
        // Tabs.
        let ty = r.y0 + 52.0 * s;
        let tw = (w - 36.0 * s) / 2.0;
        v.push(Widget::new(Rect::new(r.x0 + 16.0 * s, ty, r.x0 + 16.0 * s + tw, ty + 34.0 * s), WKind::Radio { on: t.tab == TimerTab::Countdown, label: "倒计时".into() }, Action::TimerTab(0)));
        v.push(Widget::new(Rect::new(r.x0 + 20.0 * s + tw, ty, r.x1 - 16.0 * s, ty + 34.0 * s), WKind::Radio { on: t.tab == TimerTab::Stopwatch, label: "秒表".into() }, Action::TimerTab(1)));
        // Digits.
        let (digits, running) = match t.tab {
            TimerTab::Countdown => (format_countdown(t.countdown.remaining(self.now)), t.countdown.state() == RunState::Running),
            TimerTab::Stopwatch => (format_stopwatch(t.stopwatch.elapsed(self.now)), t.stopwatch.state() == RunState::Running),
        };
        let size = 56.0;
        let dy = r.y0 + 130.0 * s;
        let color = if alarm { self.theme.danger } else { self.theme.text };
        v.push(Widget::new(Rect::new(r.x0, dy, r.x1, dy + 70.0 * s), WKind::Digits { text: digits.clone(), size, color }, Action::None));
        let cx = r.center().x;
        match t.tab {
            TimerTab::Countdown => {
                // Minutes and seconds each get their own up/down buttons while the
                // countdown is not running. Press and hold to repeat.
                let editable = matches!(t.countdown.state(), RunState::Idle | RunState::Finished);
                if editable && digits.len() == 5 {
                    let cell = size * s * 0.62;
                    let colon = size * s * 0.34;
                    let bw = cell * 2.0 - 6.0 * s;
                    for (part_cx, is_min) in [(cx - colon * 0.5 - cell, true), (cx + colon * 0.5 + cell, false)] {
                        let (up, down) = if is_min { (Action::TimerAdjustMin(1), Action::TimerAdjustMin(-1)) } else { (Action::TimerAdjustSec(1), Action::TimerAdjustSec(-1)) };
                        v.push(button(Rect::new(part_cx - bw * 0.5, dy - 34.0 * s, part_cx + bw * 0.5, dy - 4.0 * s), Some(Icon::ChevronUp), "", BtnStyle::Chip, up));
                        v.push(button(Rect::new(part_cx - bw * 0.5, dy + 74.0 * s, part_cx + bw * 0.5, dy + 104.0 * s), Some(Icon::ChevronDown), "", BtnStyle::Chip, down));
                    }
                }
                let cy = r.y0 + 270.0 * s;
                let presets = [1u32, 3, 5, 10];
                let bw = (w - 32.0 * s - 3.0 * 8.0 * s) / 4.0;
                let mut x = r.x0 + 16.0 * s;
                for p in presets {
                    let mut b = button(Rect::new(x, cy - 16.0 * s, x + bw, cy + 16.0 * s), None, &format!("{p} 分钟"), BtnStyle::Chip, Action::TimerPreset(p));
                    if let WKind::Button { active, enabled, .. } = &mut b.kind {
                        *active = t.countdown.total.as_secs() == p as u64 * 60;
                        *enabled = editable;
                    }
                    v.push(b);
                    x += bw + 8.0 * s;
                }
            }
            TimerTab::Stopwatch => {
                let cy = r.y0 + 236.0 * s;
                let laps: Vec<String> = t.stopwatch.laps.iter().rev().take(3).enumerate().map(|(i, l)| format!("计次 {}  {}", t.stopwatch.laps.len() - i, format_stopwatch(*l))).collect();
                v.push(label(Rect::new(r.x0 + 16.0 * s, cy - 18.0 * s, r.x1 - 16.0 * s, cy + 52.0 * s), &laps.join("\n"), 12.5, 400, 1));
            }
        }
        let by = r.y1 - 66.0 * s;
        let bw = (w - 48.0 * s) / 3.0;
        let x0 = r.x0 + 16.0 * s;
        let start_label = if running { "暂停" } else if t.tab == TimerTab::Countdown && t.countdown.state() == RunState::Paused || t.tab == TimerTab::Stopwatch && t.stopwatch.state() == RunState::Paused { "继续" } else { "开始" };
        if t.alarm_since.is_some() {
            v.push(button(Rect::new(x0, by, r.x1 - 16.0 * s, by + 46.0 * s), Some(Icon::Bell), "知道了", BtnStyle::Danger, Action::TimerDismissAlarm));
        } else {
            v.push(button(Rect::new(x0, by, x0 + bw * 1.5, by + 46.0 * s), Some(if running { Icon::Pause } else { Icon::Play }), start_label, BtnStyle::Primary, Action::TimerStartPause));
            let x = x0 + bw * 1.5 + 8.0 * s;
            if t.tab == TimerTab::Stopwatch && running {
                v.push(button(Rect::new(x, by, x + bw * 0.75 - 4.0 * s, by + 46.0 * s), Some(Icon::Flag), "", BtnStyle::Ghost, Action::TimerLap));
                v.push(button(Rect::new(x + bw * 0.75 + 4.0 * s, by, r.x1 - 16.0 * s, by + 46.0 * s), Some(Icon::Reset), "", BtnStyle::Ghost, Action::TimerReset));
            } else {
                v.push(button(Rect::new(x, by, x + bw * 0.75 - 4.0 * s, by + 46.0 * s), Some(Icon::Reset), "", BtnStyle::Ghost, Action::TimerReset));
                let mut snd = button(Rect::new(x + bw * 0.75 + 4.0 * s, by, r.x1 - 16.0 * s, by + 46.0 * s), Some(Icon::Bell), "", BtnStyle::Ghost, Action::TimerSound);
                if let WKind::Button { active, .. } = &mut snd.kind {
                    *active = self.settings.timer_sound;
                }
                v.push(snd);
            }
        }
        v
    }

    fn layout_timer_fullscreen(&self) -> Vec<Widget> {
        let s = self.scale;
        let t = self.timer.as_ref().unwrap();
        let (sw, sh) = (self.size.0 as f32, self.size.1 as f32);
        let mut v = vec![Widget::new(Rect::new(0.0, 0.0, sw, sh), WKind::Panel { radius: 0.0, elevated: false }, Action::None)];
        let alarm = t.alarm_since.map(|a| (self.now.duration_since(a).as_millis() / 400) % 2 == 0).unwrap_or(false);
        let (digits, running) = match t.tab {
            TimerTab::Countdown => (format_countdown(t.countdown.remaining(self.now)), t.countdown.state() == RunState::Running),
            TimerTab::Stopwatch => (format_stopwatch(t.stopwatch.elapsed(self.now)), t.stopwatch.state() == RunState::Running),
        };
        let size = (sh / s * 0.30).min(sw / s / (digits.chars().count() as f32 * 0.62));
        v.push(label(Rect::new(0.0, sh * 0.08, sw, sh * 0.16), if t.tab == TimerTab::Countdown { "倒计时" } else { "秒表" }, 22.0, 600, 1));
        v.push(Widget::new(
            Rect::new(0.0, sh * 0.5 - size * s * 0.7, sw, sh * 0.5 + size * s * 0.7),
            WKind::Digits { text: digits, size, color: if alarm { self.theme.danger } else { self.theme.text } },
            Action::None,
        ));
        let by = sh - 120.0 * s;
        let bw = 160.0 * s;
        let cx = sw * 0.5;
        if t.alarm_since.is_some() {
            v.push(button(Rect::new(cx - bw, by, cx + bw, by + 56.0 * s), Some(Icon::Bell), "知道了", BtnStyle::Danger, Action::TimerDismissAlarm));
        } else {
            v.push(button(Rect::new(cx - bw - 8.0 * s, by, cx - 8.0 * s, by + 56.0 * s), Some(if running { Icon::Pause } else { Icon::Play }), if running { "暂停" } else { "开始" }, BtnStyle::Primary, Action::TimerStartPause));
            v.push(button(Rect::new(cx + 8.0 * s, by, cx + bw + 8.0 * s, by + 56.0 * s), Some(Icon::Reset), "重置", BtnStyle::Ghost, Action::TimerReset));
        }
        v.push(button(Rect::new(sw - 64.0 * s, 20.0 * s, sw - 20.0 * s, 64.0 * s), Some(Icon::Close), "", BtnStyle::Ghost, Action::TimerFullscreen));
        v
    }

    fn layout_remote(&self) -> Vec<Widget> {
        let s = self.scale;
        let p = self.remote.as_ref().unwrap();
        let mut v = Vec::new();
        // Status icon (top right) while the service runs: green = connected.
        let sz = 44.0 * s;
        let sr = Rect::xywh(self.size.0 as f32 - sz - 18.0 * s, 18.0 * s, sz, sz);
        let col = if p.peer.is_some() { REMOTE_ON } else { cb_core::Color::rgb(150, 156, 168) };
        v.push(Widget::new(sr, WKind::StatusDot { icon: Icon::Phone, color: col }, Action::RemotePanel));
        if !p.panel_open {
            return v;
        }
        let (w, h) = (400.0 * s, 572.0 * s);
        let r = Rect::xywh(p.pos.x, p.pos.y, w, h);
        v.push(Widget::new(r, WKind::Panel { radius: metrics::RADIUS_L, elevated: true }, Action::None));
        v.push(drag_handle(Rect::new(r.x0, r.y0, r.x1 - 56.0 * s, r.y0 + 52.0 * s), PanelKind::Remote));
        v.push(label(Rect::new(r.x0 + 20.0 * s, r.y0 + 16.0 * s, r.x1 - 60.0 * s, r.y0 + 38.0 * s), "连接手机 / 平板", 15.0, 700, 0));
        v.push(button(Rect::new(r.x1 - 48.0 * s, r.y0 + 10.0 * s, r.x1 - 12.0 * s, r.y0 + 46.0 * s), Some(Icon::Close), "", BtnStyle::Ghost, Action::ClosePanel(PanelKind::Remote)));
        let x0 = r.x0 + 20.0 * s;
        let x1 = r.x1 - 20.0 * s;
        let mut y = r.y0 + 56.0 * s;
        if let Some(e) = &p.error {
            v.push(label(Rect::new(x0, y, x1, y + 60.0 * s), e, 13.0, 400, 0));
            return v;
        }
        // Step 1: network.
        let net = match &p.hotspot {
            HotspotUi::On { ssid, passphrase } => format!("① 手机连接电脑热点\n    名称：{ssid}\n    密码：{passphrase}"),
            HotspotUi::Starting => "① 正在开启电脑热点…".to_string(),
            HotspotUi::Unavailable(why) => format!("① 请让手机连接与电脑相同的局域网（Wi-Fi）\n    （无法开启热点：{why}）"),
            HotspotUi::NotAttempted => "① 请让手机连接与电脑相同的局域网（Wi-Fi）".to_string(),
        };
        let lines = net.lines().count() as f32;
        v.push(label(Rect::new(x0, y, x1, y + lines * 20.0 * s), &net, 13.5, 600, 0));
        if matches!(p.hotspot, HotspotUi::Unavailable(_)) && self.env.can_hotspot {
            v.push(button(Rect::new(x1 - 96.0 * s, y - 4.0 * s, x1, y + 26.0 * s), Some(Icon::Wifi), "重试", BtnStyle::Chip, Action::RemoteRetryHotspot));
        }
        y += lines * 20.0 * s + 12.0 * s;
        v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), "② 用手机扫描二维码，或在浏览器打开：", 13.5, 600, 0));
        y += 26.0 * s;
        let qs = (h - (y - r.y0) - 148.0 * s).min(w - 120.0 * s);
        if let Some((n, cells)) = &p.qr {
            v.push(Widget::new(Rect::xywh(r.center().x - qs * 0.5, y, qs, qs), WKind::Qr { n: *n, cells: cells.clone() }, Action::None));
        }
        y += qs + 10.0 * s;
        v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), &p.url, 13.0, 600, 1));
        y += 28.0 * s;
        let st = match &p.peer {
            Some(peer) => format!("● 已连接：{peer}（手机上的书写会实时显示在大屏上）"),
            None => "○ 等待手机连接…".to_string(),
        };
        v.push(Widget::new(Rect::new(x0, y, x1, y + 20.0 * s), WKind::Label { text: st, size: 12.5, weight: 500, color: Some(if p.peer.is_some() { REMOTE_ON } else { self.theme.text_dim }), align: 1 }, Action::None));
        v.push(button(Rect::new(x0, r.y1 - 58.0 * s, x1, r.y1 - 16.0 * s), Some(Icon::Close), "停止连接", BtnStyle::Ghost, Action::RemoteStop));
        v
    }
}

fn drag_handle(r: Rect, which: PanelKind) -> Widget {
    Widget::new(r, WKind::Area, Action::PanelDrag(which))
}
