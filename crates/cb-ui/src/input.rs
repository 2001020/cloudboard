//! Platform-neutral input events and requests back to the platform shell.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Mouse,
    Touch,
    Pen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
}

/// A pointer sample in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pointer {
    pub id: u64,
    pub kind: PointerKind,
    pub x: f32,
    pub y: f32,
    /// 0..1; 1.0 when unknown.
    pub pressure: f32,
    /// Pen eraser end / barrel button.
    pub eraser: bool,
    pub button: Button,
}

impl Pointer {
    pub fn mouse(x: f32, y: f32) -> Pointer {
        Pointer { id: 0, kind: PointerKind::Mouse, x, y, pressure: 1.0, eraser: false, button: Button::Primary }
    }
    pub fn touch(id: u64, x: f32, y: f32) -> Pointer {
        Pointer { id, kind: PointerKind::Touch, x, y, pressure: 1.0, eraser: false, button: Button::Primary }
    }
    pub fn pen(x: f32, y: f32, pressure: f32) -> Pointer {
        Pointer { id: 1, kind: PointerKind::Pen, x, y, pressure, eraser: false, button: Button::Primary }
    }
    pub fn with_button(mut self, b: Button) -> Pointer {
        self.button = b;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    /// Ctrl on Windows, Command on macOS.
    pub cmd: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Escape,
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Tab,
    Space,
    F11,
    Char(char),
}

#[derive(Debug, Clone)]
pub enum Event {
    PointerDown(Pointer),
    PointerMove(Pointer),
    PointerUp(Pointer),
    PointerCancel(u64),
    /// Mouse leaves the window (clears hover).
    PointerLeave,
    /// Wheel/trackpad scroll in physical pixels (`lines` when the delta is in lines).
    Wheel { x: f32, y: f32, dx: f32, dy: f32, lines: bool, mods: Mods },
    /// Trackpad magnify gesture (macOS), factor-1.
    Magnify { x: f32, y: f32, delta: f32 },
    Key { key: Key, mods: Mods },
    /// Committed text (keyboard or IME).
    Text(String),
    Resize { width: u32, height: u32, scale: f32 },
    /// A file was chosen in the open dialog, dropped, or passed on the command line.
    OpenPath(PathBuf),
    /// Path chosen in the save dialog.
    SavePath(PathBuf),
    /// Answer to `Request::SaveVideoDialog` (None = cancelled).
    SaveVideoPath(Option<PathBuf>),
    /// Result of starting the mobile hotspot.
    Hotspot(HotspotResult),
    /// Background work completed (PDF tiles, saves, network); just poll.
    Wake,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HotspotResult {
    Started { ssid: String, passphrase: String },
    Unsupported(String),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Alarm,
    Tick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Default,
    Pointer,
    Crosshair,
    Grab,
    Grabbing,
    Text,
    Move,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    Exit,
    Minimize,
    ToggleFullscreen,
    OpenFileDialog,
    SaveFileDialog { default_name: String },
    /// Ask where to save a finished recording; answer with `Event::SaveVideoPath`.
    SaveVideoDialog { default_name: String },
    PlaySound(Sound),
    SetCursor(CursorIcon),
    /// Start the OS hotspot; answer with `Event::Hotspot`.
    StartHotspot,
    StopHotspot,
    /// Install a downloaded, verified update package and restart.
    InstallUpdate { package: PathBuf },
    OpenUrl(String),
    /// Allow/disallow IME input and position its candidate window.
    Ime { enabled: bool, x: f32, y: f32 },
}

/// Capabilities and environment provided by the platform shell.
#[derive(Clone)]
pub struct Env {
    pub waker: std::sync::Arc<dyn Fn() + Send + Sync>,
    pub pdfium: Option<PathBuf>,
    pub platform: &'static str,
    pub can_hotspot: bool,
    /// Asset name fragment used to pick the update package, e.g. "windows-x64".
    pub update_asset: String,
    pub settings_path: Option<PathBuf>,
    /// Directory for staged updates and autosave.
    pub data_dir: Option<PathBuf>,
    pub system_dark: bool,
    /// System UI font families, most preferred first.
    pub ui_fonts: Vec<String>,
    /// Screen recording support (encoder + audio capture), if any.
    pub recorder: Option<cb_record::Backend>,
    /// Local time offset from UTC in minutes (for file names).
    pub utc_offset_min: i32,
}

impl Env {
    pub fn headless() -> Env {
        Env {
            waker: std::sync::Arc::new(|| {}),
            pdfium: None,
            platform: "test",
            can_hotspot: false,
            update_asset: String::new(),
            settings_path: None,
            data_dir: None,
            system_dark: true,
            ui_fonts: Vec::new(),
            recorder: None,
            utc_offset_min: 0,
        }
    }
}
