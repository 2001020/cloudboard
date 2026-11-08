//! User settings (JSON). Stored in the per-user config directory, not next
//! to the executable (which may be read-only under Program Files).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme_mode: ThemeMode,
    /// 0..=5: blue, purple, green, orange, red, cyan
    pub accent: u8,
    pub show_splash: bool,
    pub pen_color: u32,
    pub pen_width: f32,
    pub eraser_size: f32,
    /// Split strokes (true) or delete whole strokes (false).
    pub eraser_split: bool,
    pub shape_recognition: bool,
    pub pressure: bool,
    pub auto_check_update: bool,
    /// Optional prefix prepended to GitHub download URLs (mirror).
    pub update_mirror: String,
    pub skipped_version: String,
    pub remote_port: u16,
    pub timer_last_minutes: u32,
    pub timer_sound: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme_mode: ThemeMode::Dark,
            accent: 0,
            show_splash: true,
            pen_color: 0xFF1E1E1E,
            pen_width: 4.0,
            eraser_size: 24.0,
            eraser_split: true,
            shape_recognition: false,
            pressure: true,
            auto_check_update: true,
            update_mirror: String::new(),
            skipped_version: String::new(),
            remote_port: 0,
            timer_last_minutes: 5,
            timer_sound: true,
        }
    }
}

impl Settings {
    pub fn sanitize(&mut self) {
        self.accent = self.accent.min(5);
        self.pen_width = self.pen_width.clamp(1.0, 64.0);
        self.eraser_size = self.eraser_size.clamp(5.0, 120.0);
        self.timer_last_minutes = self.timer_last_minutes.clamp(1, 99);
    }

    pub fn default_path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("settings.json"))
    }

    pub fn load_from(path: &Path) -> Settings {
        let mut s = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<Settings>(&t).ok()).unwrap_or_default();
        s.sanitize();
        s
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    /// Import the few keys CloudBoard 26 stored (UTF-16LE JSON next to the exe).
    pub fn import_legacy(bytes: &[u8]) -> Option<Settings> {
        let text = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
            let u: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&u)
        } else {
            String::from_utf8_lossy(bytes).into_owned()
        };
        let v: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
        let mut s = Settings::default();
        if let Some(t) = v.get("colorTheme").and_then(|x| x.as_i64()) {
            s.theme_mode = match t {
                1 => ThemeMode::Light,
                2 => ThemeMode::System,
                _ => ThemeMode::Dark,
            };
        }
        if let Some(t) = v.get("themeColor").and_then(|x| x.as_i64()) {
            s.accent = t.clamp(0, 5) as u8;
        }
        if let Some(b) = v.get("showSplash") {
            s.show_splash = b.as_bool().or_else(|| b.as_i64().map(|i| i != 0)).unwrap_or(true);
        }
        Some(s)
    }
}

/// Per-user application directory (`%APPDATA%\CloudBoard`, `~/Library/Application Support/CloudBoard`).
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("CloudBoard"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support/CloudBoard"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("cloudboard"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_partial() {
        let dir = std::env::temp_dir().join(format!("cbset-{}", std::process::id()));
        let p = dir.join("settings.json");
        let mut s = Settings::default();
        s.accent = 3;
        s.save_to(&p).unwrap();
        assert_eq!(Settings::load_from(&p), s);
        std::fs::write(&p, r#"{"accent": 9, "unknown": 1}"#).unwrap();
        let l = Settings::load_from(&p);
        assert_eq!(l.accent, 5);
        assert!(l.show_splash);
        let _ = std::fs::remove_dir_all(dir);
    }
    #[test]
    fn legacy_utf16() {
        let json = "{\n \"colorTheme\": 1,\n \"themeColor\": 2,\n \"showSplash\": false\n}";
        let mut b = vec![0xFF, 0xFE];
        for u in json.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        let s = Settings::import_legacy(&b).unwrap();
        assert_eq!(s.theme_mode, ThemeMode::Light);
        assert_eq!(s.accent, 2);
        assert!(!s.show_splash);
    }
}
