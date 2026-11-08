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

        // Left: Exit, Center, Minimize, Settings (legacy order).
        let side_w = metrics::SIDE_BTN_W * s;
        let mut x = 8.0 * s;
        let narrow = w < 1000.0 * s;
        let very_narrow = w < 760.0 * s;
        let side = [
            (Icon::Exit, "退出", Action::Exit, true),
            (Icon::Center, "回中", Action::Recenter, false),
            (Icon::Minimize, "最小", Action::Minimize, false),
            (Icon::Settings, "设置", Action::OpenSettings, false),
        ];
        for (icon, lab, act, danger) in side {
            if narrow && matches!(act, Action::Minimize) || very_narrow && matches!(act, Action::Recenter) {
                continue;
            }
            let mut b = button(Rect::new(x, y0, x + side_w, y1), Some(icon), lab, BtnStyle::Side, act);
            if danger {
                if let WKind::Button { dot, .. } = &mut b.kind {
                    *dot = None;
                }
            }
            ws.push(b);
            x += side_w + 6.0 * s;
        }

        // Glass panel.
        let gx0 = x + 2.0 * s;
        let gx1 = w - pad;
        ws.push(Widget::new(Rect::new(gx0, y0, gx1, y1), WKind::GlassBar { radius: metrics::RADIUS_L }, Action::None));

        // Right: page navigation.
        let pdf = self.is_pdf();
        // Narrow windows drop the +/- page buttons (still reachable via the page menu / keys).
        let compact_nav = w < 1100.0 * s;
        let show_pm = !pdf && !compact_nav;
        let nav_h = 36.0 * s;
        let ny0 = (y0 + y1) * 0.5 - nav_h * 0.5;
        let nb = 36.0 * s;
        let lbl_w = if compact_nav { 54.0 * s } else { 64.0 * s };
        let gap = 4.0 * s;
        let right_w = nb * 2.0 + lbl_w + gap * 2.0 + if show_pm { (nb + gap) * 2.0 + 6.0 * s } else { 0.0 };
        let mut rx = gx1 - 10.0 * s - right_w;
        let nav_x0 = rx;
        let cur = self.display_page();
        let n = self.page_count();
        let mut prev = button(Rect::new(rx, ny0, rx + nb, ny0 + nav_h), Some(Icon::ChevronLeft), "", BtnStyle::Chip, Action::PrevPage);
        if let WKind::Button { enabled, .. } = &mut prev.kind {
            *enabled = cur > 0;
        }
        ws.push(prev);
        rx += nb + gap;
        let page_lbl = format!("{}/{}", cur + 1, n);
        let r = Rect::new(rx, ny0, rx + lbl_w, ny0 + nav_h);
        ws.push(button(r, None, &page_lbl, BtnStyle::Chip, Action::TogglePopup(PopupKind::Pages)));
        anchors.push((PopupKind::Pages, r));
        rx += lbl_w + gap;
        let mut next = button(Rect::new(rx, ny0, rx + nb, ny0 + nav_h), Some(Icon::ChevronRight), "", BtnStyle::Chip, Action::NextPage);
        if let WKind::Button { enabled, .. } = &mut next.kind {
            *enabled = !pdf || cur + 1 < n;
        }
        ws.push(next);
        rx += nb + gap;
        if show_pm {
            rx += 6.0 * s;
            ws.push(button(Rect::new(rx, ny0, rx + nb, ny0 + nav_h), Some(Icon::Plus), "", BtnStyle::Chip, Action::AddPage));
            rx += nb + gap;
            let mut del = button(Rect::new(rx, ny0, rx + nb, ny0 + nav_h), Some(Icon::Minus), "", BtnStyle::Chip, Action::DeletePage);
            if let WKind::Button { enabled, .. } = &mut del.kind {
                *enabled = n > 1;
            }
            ws.push(del);
        }

        // Centre: tools.
        let zoom_lbl = format!("{:.0}%", self.vp.zoom * 100.0);
        let tools: Vec<(Icon, &str, Action, bool, bool, Option<PopupKind>)> = vec![
            (Icon::FolderOpen, "打开", Action::Open, false, false, None),
            (Icon::Save, "保存", Action::Save, false, false, None),
            (Icon::Zoom, &zoom_lbl, Action::TogglePopup(PopupKind::Zoom), self.popup == Some(PopupKind::Zoom), true, Some(PopupKind::Zoom)),
            (Icon::Pan, "拖动", Action::SetTool(Tool::Pan), self.tool == Tool::Pan, false, None),
            (if self.highlighter { Icon::Highlighter } else { Icon::Pen }, "画笔", Action::SetTool(Tool::Pen), self.tool == Tool::Pen, true, Some(PopupKind::Pen)),
            (Icon::Eraser, "橡皮", Action::SetTool(Tool::Eraser), self.tool == Tool::Eraser, true, Some(PopupKind::Eraser)),
            (Icon::Select, "选择", Action::SetTool(Tool::Select), self.tool == Tool::Select, false, None),
            (Icon::Text, "文字", Action::SetTool(Tool::Text), self.tool == Tool::Text, false, None),
            (Icon::SetSquare, "教具", Action::TogglePopup(PopupKind::Assist), self.popup == Some(PopupKind::Assist) || !self.assist.tools.is_empty(), true, Some(PopupKind::Assist)),
            (Icon::Timer, "计时", Action::ToggleTimer, self.timer.is_some(), false, None),
            (Icon::Phone, "手机", Action::ToggleRemote, self.remote.is_some(), false, None),
            (Icon::Undo, "撤销", Action::Undo, false, false, None),
            (Icon::Redo, "重做", Action::Redo, false, false, None),
        ];
        let count = tools.len() as f32;
        let gap = 2.0 * s;
        let avail = nav_x0 - gx0 - 24.0 * s;
        let mut bw = 54.0 * s;
        let mut labels = true;
        if count * (bw + gap) > avail {
            bw = 46.0 * s;
        }
        if count * (bw + gap) > avail {
            bw = (avail / count - gap).max(26.0 * s);
            labels = bw >= 44.0 * s;
        }
        let group_w = count * (bw + gap) - gap;
        let mut cx = ((gx0 + gx1) * 0.5 - group_w * 0.5).min(nav_x0 - 12.0 * s - group_w).max(gx0 + 8.0 * s);
        let by0 = y0 + 4.0 * s;
        let by1 = y1 - 4.0 * s;
        for (icon, lab, act, active, caret, anchor) in tools {
            let r = Rect::new(cx, by0, cx + bw, by1);
            let enabled = match act {
                Action::Undo => self.history.can_undo(),
                Action::Redo => self.history.can_redo(),
                _ => true,
            };
            let dot = if matches!(act, Action::SetTool(Tool::Pen)) { Some(self.pen_color) } else { None };
            ws.push(Widget::new(r, WKind::Button { icon: Some(icon), label: if labels { lab.to_string() } else { String::new() }, style: BtnStyle::Tool, active, caret, dot, enabled }, act));
            if let Some(k) = anchor {
                anchors.push((k, r));
            }
            cx += bw + gap;
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
                let (w, h) = (360.0 * s, 156.0 * s);
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
                v.push(button(Rect::new(x0, cy - 17.0 * s, r.x1 - 16.0 * s, cy + 17.0 * s), Some(Icon::Trash), "清空本页", BtnStyle::Danger, Action::ClearPage));
            }
            PopupKind::Zoom => {
                let zs = [0.5f32, 0.75, 1.0, 1.5, 2.0, 3.0];
                let extra = if self.is_pdf() { 1 } else { 0 };
                let bw = 58.0 * s;
                let w = 16.0 * s * 2.0 + (zs.len() + extra) as f32 * (bw + 6.0 * s) - 6.0 * s + if extra > 0 { 30.0 * s } else { 0.0 };
                let h = 64.0 * s;
                let r = self.popup_rect(w, h, anchor, bar_top);
                v.push(panel(r));
                let mut x = r.x0 + 16.0 * s;
                let cy = r.center().y;
                for z in zs {
                    let mut b = button(Rect::new(x, cy - 18.0 * s, x + bw, cy + 18.0 * s), None, &format!("{:.0}%", z * 100.0), BtnStyle::Chip, Action::SetZoom(z));
                    if let WKind::Button { active, .. } = &mut b.kind {
                        *active = (self.vp.zoom - z).abs() < 0.01;
                    }
                    v.push(b);
                    x += bw + 6.0 * s;
                }
                if extra > 0 {
                    v.push(button(Rect::new(x, cy - 18.0 * s, x + bw + 30.0 * s, cy + 18.0 * s), None, "适应宽度", BtnStyle::Chip, Action::ZoomFit));
                }
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
                v.push(label(Rect::new(r.x0 + 16.0 * s, y, r.x1 - 140.0 * s, y + 28.0 * s), "拖动移动 · 旋转手柄旋转 · 双指缩放", 12.0, 400, 0));
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
                let r = self.dialog_frame(520.0 * s, 600.0 * s, "设置", &mut v);
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
                let toggles = [
                    ("启动时显示加载画面", self.settings.show_splash, Action::ToggleSplash),
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
                v.push(label(Rect::new(x0, y, x1, y + 20.0 * s), &format!("{} · 版本 {}", cb_core::version::splash_title(), cb_core::version::APP_VERSION), 13.0, 600, 0));
                v.push(button(Rect::new(x1 - 150.0 * s, y - 8.0 * s, x1, y + 28.0 * s), Some(Icon::Download), "检查更新", BtnStyle::Chip, Action::CheckUpdate));
                v.push(label(Rect::new(x0, y + 24.0 * s, x1 - 160.0 * s, y + 44.0 * s), "更新来源：GitHub · 2001020/cloudboard", 12.0, 400, 0));
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
                        let notes = if rel.notes.is_empty() { "· 性能与稳定性改进".to_string() } else { rel.notes.iter().take(5).map(|n| format!("· {n}")).collect::<Vec<_>>().join("\n") };
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
        let (w, h) = (360.0 * s, 318.0 * s);
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
        let dy = r.y0 + 96.0 * s;
        let color = if alarm { Some(self.theme.danger) } else { None };
        v.push(Widget::new(Rect::new(r.x0, dy, r.x1, dy + 70.0 * s), WKind::Label { text: digits, size: 56.0, weight: 700, color: color.or(Some(self.theme.text)), align: 1 }, Action::None));
        let cy = r.y0 + 196.0 * s;
        match t.tab {
            TimerTab::Countdown => {
                let presets = [1u32, 3, 5, 10];
                let bw = 52.0 * s;
                let mut x = r.x0 + 16.0 * s;
                v.push(button(Rect::new(x, cy - 16.0 * s, x + 36.0 * s, cy + 16.0 * s), Some(Icon::Minus), "", BtnStyle::Chip, Action::TimerAdjust(-60)));
                x += 42.0 * s;
                for p in presets {
                    let mut b = button(Rect::new(x, cy - 16.0 * s, x + bw, cy + 16.0 * s), None, &format!("{p}分"), BtnStyle::Chip, Action::TimerPreset(p));
                    if let WKind::Button { active, .. } = &mut b.kind {
                        *active = t.countdown.total.as_secs() == p as u64 * 60;
                    }
                    v.push(b);
                    x += bw + 6.0 * s;
                }
                v.push(button(Rect::new(x, cy - 16.0 * s, x + 36.0 * s, cy + 16.0 * s), Some(Icon::Plus), "", BtnStyle::Chip, Action::TimerAdjust(60)));
            }
            TimerTab::Stopwatch => {
                let laps: Vec<String> = t.stopwatch.laps.iter().rev().take(2).enumerate().map(|(i, l)| format!("计次 {}  {}", t.stopwatch.laps.len() - i, format_stopwatch(*l))).collect();
                v.push(label(Rect::new(r.x0 + 16.0 * s, cy - 18.0 * s, r.x1 - 16.0 * s, cy + 22.0 * s), &laps.join("    "), 12.5, 400, 1));
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
            WKind::Label { text: digits, size, weight: 700, color: Some(if alarm { self.theme.danger } else { self.theme.text }), align: 1 },
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
        // Once a device is connected the panel collapses to a status chip.
        if let Some(peer) = &p.peer {
            let r = Rect::xywh(p.pos.x, p.pos.y, 320.0 * s, 64.0 * s);
            v.push(Widget::new(r, WKind::Panel { radius: metrics::RADIUS_L, elevated: true }, Action::None));
            v.push(drag_handle(Rect::new(r.x0, r.y0, r.x1 - 52.0 * s, r.y1), PanelKind::Remote));
            v.push(Widget::new(Rect::new(r.x0 + 18.0 * s, r.y0 + 12.0 * s, r.x1 - 56.0 * s, r.y0 + 32.0 * s), WKind::Label { text: "● 手机已连接".into(), size: 14.0, weight: 700, color: Some(cb_core::Color::rgb(34, 166, 110)), align: 0 }, Action::None));
            v.push(label(Rect::new(r.x0 + 18.0 * s, r.y0 + 34.0 * s, r.x1 - 56.0 * s, r.y0 + 54.0 * s), &format!("{peer} · 书写实时同步中"), 12.0, 400, 0));
            v.push(button(Rect::new(r.x1 - 46.0 * s, r.y0 + 14.0 * s, r.x1 - 10.0 * s, r.y0 + 50.0 * s), Some(Icon::Close), "", BtnStyle::Ghost, Action::ClosePanel(PanelKind::Remote)));
            return v;
        }
        let (w, h) = (400.0 * s, 520.0 * s);
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
        let qs = (h - (y - r.y0) - 96.0 * s).min(w - 120.0 * s);
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
        v.push(Widget::new(Rect::new(x0, y, x1, y + 20.0 * s), WKind::Label { text: st, size: 12.5, weight: 500, color: Some(if p.peer.is_some() { cb_core::Color::rgb(34, 166, 110) } else { self.theme.text_dim }), align: 1 }, Action::None));
        v
    }
}

fn drag_handle(r: Rect, which: PanelKind) -> Widget {
    Widget::new(r, WKind::Area, Action::PanelDrag(which))
}
