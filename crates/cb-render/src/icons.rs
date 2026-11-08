//! Vector icon set on a 24×24 grid, stroked with round caps (line-icon style).

use crate::svgpath;
use cb_core::Color;
use std::collections::HashMap;
use std::sync::OnceLock;
use tiny_skia::{LineCap, LineJoin, Paint, Path, PixmapMut, Stroke, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    Exit,
    Center,
    Minimize,
    Settings,
    FolderOpen,
    Save,
    Zoom,
    Pan,
    Pen,
    Highlighter,
    Eraser,
    Select,
    Undo,
    Redo,
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    Plus,
    Minus,
    Close,
    Check,
    Copy,
    Paste,
    Trash,
    Ruler,
    SetSquare,
    Protractor,
    Compass,
    Timer,
    Phone,
    Text,
    Shapes,
    Play,
    Pause,
    Reset,
    Bell,
    Fullscreen,
    Image,
    Download,
    Wifi,
    Flag,
    Rotate,
    Pages,
}

fn data(icon: Icon) -> &'static str {
    match icon {
        Icon::Exit => "M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4 M16 17l5-5-5-5 M21 12H9",
        Icon::Center => "M12 2v4 M12 18v4 M2 12h4 M18 12h4 M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6 M12 5a7 7 0 1 0 0 14a7 7 0 1 0 0-14",
        Icon::Minimize => "M5 18h14",
        Icon::Settings => "M12 9a3 3 0 1 0 0 6a3 3 0 1 0 0-6 M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.68 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.68a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z",
        Icon::FolderOpen => "M6 14l1.45-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.55 6a2 2 0 0 1-1.94 1.5H4a2 2 0 0 1-2-2V5c0-1.1.9-2 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2a2 2 0 0 0 1.66.9H18a2 2 0 0 1 2 2v2",
        Icon::Save => "M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z M17 21v-8H7v8 M7 3v5h8",
        Icon::Zoom => "M11 3a8 8 0 1 0 0 16a8 8 0 1 0 0-16 M21 21l-4.35-4.35 M11 8v6 M8 11h6",
        Icon::Pan => "M18 11V6a2 2 0 0 0-4 0v0 M14 10V4a2 2 0 0 0-4 0v2 M10 10.5V6a2 2 0 0 0-4 0v8 M18 8a2 2 0 1 1 4 0v6a8 8 0 0 1-8 8h-2c-2.8 0-4.5-.86-5.99-2.34l-3.6-3.6a2 2 0 0 1 2.83-2.82L7 15",
        Icon::Pen => "M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z M15 5l4 4",
        Icon::Highlighter => "M9 11l-6 6v3h9l3-3 M22 12l-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4",
        Icon::Eraser => "M7 21l-4.3-4.3c-1-1-1-2.5 0-3.4l9.6-9.6c1-1 2.5-1 3.4 0l5.6 5.6c1 1 1 2.5 0 3.4L13 21 M22 21H7 M5 11l9 9",
        Icon::Select => "M5 3a2 2 0 0 0-2 2 M19 3a2 2 0 0 1 2 2 M21 19a2 2 0 0 1-2 2 M5 21a2 2 0 0 1-2-2 M9 3h1 M9 21h1 M14 3h1 M14 21h1 M3 9v1 M21 9v1 M3 14v1 M21 14v1",
        Icon::Undo => "M3 7v6h6 M21 17a9 9 0 0 0-9-9 9 9 0 0 0-6 2.3L3 13",
        Icon::Redo => "M21 7v6h-6 M3 17a9 9 0 0 1 9-9 9 9 0 0 1 6 2.3l3 2.7",
        Icon::ChevronLeft => "M15 18l-6-6 6-6",
        Icon::ChevronRight => "M9 18l6-6-6-6",
        Icon::ChevronUp => "M18 15l-6-6-6 6",
        Icon::Plus => "M12 5v14 M5 12h14",
        Icon::Minus => "M5 12h14",
        Icon::Close => "M18 6L6 18 M6 6l12 12",
        Icon::Check => "M20 6L9 17l-5-5",
        Icon::Copy => "M10 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H10a2 2 0 0 1-2-2V10a2 2 0 0 1 2-2z M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2",
        Icon::Paste => "M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2 M9 2h6v4H9z",
        Icon::Trash => "M3 6h18 M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6 M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2 M10 11v6 M14 11v6",
        Icon::Ruler => "M21.3 15.3a2.4 2.4 0 0 1 0 3.4l-2.6 2.6a2.4 2.4 0 0 1-3.4 0L2.7 8.7a2.41 2.41 0 0 1 0-3.4l2.6-2.6a2.41 2.41 0 0 1 3.4 0Z M14.5 12.5l2-2 M11.5 9.5l2-2 M8.5 6.5l2-2 M17.5 15.5l2-2",
        Icon::SetSquare => "M3 21V3l18 18Z M7 17V12.5l4.5 4.5Z",
        Icon::Protractor => "M2 18h20 M2 18a10 10 0 0 1 20 0 M12 18L12 8 M12 18l6.5-6.5 M7 18a5 5 0 0 1 10 0",
        Icon::Compass => "M12 3a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M11 6.8L5 21 M13 6.8L19 21 M7 16a12 12 0 0 0 10 0",
        Icon::Timer => "M10 2h4 M12 14l3-3 M12 6a8 8 0 1 0 0 16a8 8 0 1 0 0-16",
        Icon::Phone => "M7 2h10a2 2 0 0 1 2 2v16a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z M12 18h.01",
        Icon::Text => "M4 7V4h16v3 M9 20h6 M12 4v16",
        Icon::Shapes => "M8.3 10a.7.7 0 0 1-.63-1.05l3.67-6.12a.7.7 0 0 1 1.2 0l3.67 6.12A.7.7 0 0 1 15.6 10z M3 14h7v7H3z M17.5 14a3.5 3.5 0 1 0 0 7a3.5 3.5 0 1 0 0-7",
        Icon::Play => "M6 3l14 9-14 9V3z",
        Icon::Pause => "M6 4h4v16H6z M14 4h4v16h-4z",
        Icon::Reset => "M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8 M3 3v5h5",
        Icon::Bell => "M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9 M10.3 21a1.94 1.94 0 0 0 3.4 0",
        Icon::Fullscreen => "M8 3H5a2 2 0 0 0-2 2v3 M21 8V5a2 2 0 0 0-2-2h-3 M3 16v3a2 2 0 0 0 2 2h3 M16 21h3a2 2 0 0 0 2-2v-3",
        Icon::Image => "M5 3h14a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z M9 7a2 2 0 1 0 0 4a2 2 0 1 0 0-4 M21 15l-5-5L5 21",
        Icon::Download => "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4 M7 10l5 5 5-5 M12 15V3",
        Icon::Wifi => "M5 12.55a11 11 0 0 1 14.08 0 M1.42 9a16 16 0 0 1 21.16 0 M8.53 16.11a6 6 0 0 1 6.95 0 M12 20h.01",
        Icon::Flag => "M4 15s1-1 4-1 5 2 8 2 4-1 4-1V3s-1 1-4 1-5-2-8-2-4 1-4 1z M4 22v-7",
        Icon::Rotate => "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8 M21 3v5h-5",
        Icon::Pages => "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z M14 2v4a2 2 0 0 0 2 2h4",
    }
}

/// Icons drawn filled instead of stroked.
fn filled(icon: Icon) -> bool {
    matches!(icon, Icon::Play | Icon::Pause)
}

fn cache() -> &'static HashMap<Icon, Path> {
    static C: OnceLock<HashMap<Icon, Path>> = OnceLock::new();
    C.get_or_init(|| {
        use Icon::*;
        let all = [
            Exit, Center, Minimize, Settings, FolderOpen, Save, Zoom, Pan, Pen, Highlighter, Eraser, Select, Undo, Redo, ChevronLeft, ChevronRight, ChevronUp, Plus,
            Minus, Close, Check, Copy, Paste, Trash, Ruler, SetSquare, Protractor, Compass, Timer, Phone, Text, Shapes, Play, Pause, Reset, Bell, Fullscreen, Image,
            Download, Wifi, Flag, Rotate, Pages,
        ];
        all.iter().filter_map(|i| svgpath::parse(data(*i)).map(|p| (*i, p))).collect()
    })
}

/// Draw `icon` centred at (cx, cy) with edge length `size` (physical px).
pub fn draw(pm: &mut PixmapMut, icon: Icon, cx: f32, cy: f32, size: f32, color: Color) {
    let Some(path) = cache().get(&icon) else { return };
    let s = size / 24.0;
    let t = Transform::from_row(s, 0.0, 0.0, s, cx - size * 0.5, cy - size * 0.5);
    let mut paint = Paint::default();
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    paint.anti_alias = true;
    if filled(icon) {
        pm.fill_path(path, &paint, tiny_skia::FillRule::Winding, t, None);
    } else {
        // Keep strokes ≥1.25 physical px so small icons stay crisp.
        let w = (2.0f32).max(1.25 / s);
        let st = Stroke { width: w, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        pm.stroke_path(path, &paint, &st, t, None);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_icons_parse() {
        assert_eq!(super::cache().len(), 43);
    }
}
