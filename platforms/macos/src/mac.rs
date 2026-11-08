//! macOS integration: dialogs, sounds, update hand-off. macOS has no public
//! API to enable Internet Sharing, so the phone link always uses the LAN.

use cb_shell::winit::window::Window;
use cb_shell::Platform;
use cb_ui::input::Sound;
use std::path::{Path, PathBuf};

pub struct MacPlatform;

fn app_support() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/CloudBoard"))
}

/// The enclosing `.app` bundle of the running executable, if any.
fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.ancestors().find(|p| p.extension().map(|e| e == "app").unwrap_or(false)).map(|p| p.to_path_buf())
}

fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', "'\\''"))
}

impl Platform for MacPlatform {
    fn name(&self) -> &'static str {
        "macos"
    }

    fn ui_font_families(&self) -> Vec<String> {
        // San Francisco is exposed as ".SF NS" / "SF Pro"; CJK falls back to PingFang.
        [".AppleSystemUIFont", "SF Pro Text", "SF Pro", ".SF NS", "PingFang SC", "Helvetica Neue"].iter().map(|s| s.to_string()).collect()
    }

    fn update_asset(&self) -> String {
        "macos-universal".into()
    }

    fn play_sound(&self, s: Sound) {
        let f = match s {
            Sound::Alarm => "/System/Library/Sounds/Glass.aiff",
            Sound::Tick => "/System/Library/Sounds/Tink.aiff",
        };
        let _ = std::process::Command::new("/usr/bin/afplay").arg(f).spawn();
    }

    fn open_dialog(&self, window: &Window) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("打开")
            .add_filter("所有支持的文件", &["cloudboard", "pdf", "png", "jpg", "jpeg", "bmp", "gif"])
            .add_filter("CloudBoard 工程", &["cloudboard"])
            .add_filter("PDF 文档", &["pdf"])
            .set_parent(window)
            .pick_file()
    }

    fn save_dialog(&self, window: &Window, default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("保存白板").add_filter("CloudBoard 工程", &["cloudboard"]).set_file_name(default_name).set_parent(window).save_file()
    }

    /// Replace the .app bundle after this process exits, then relaunch.
    fn install_update(&self, package: &Path) -> Result<(), String> {
        let bundle = bundle_path().ok_or("未从 .app 应用包运行，无法自动更新")?;
        let parent = bundle.parent().ok_or("无法确定安装位置")?;
        let staging = std::env::temp_dir().join(format!("cloudboard-update-{}", std::process::id()));
        let pid = std::process::id();
        let script = format!(
            r#"set -e
while kill -0 {pid} 2>/dev/null; do sleep 0.2; done
rm -rf {st}; mkdir -p {st}
/usr/bin/ditto -x -k {pkg} {st}
NEW=$(/usr/bin/find {st} -maxdepth 2 -name '*.app' -type d | head -n 1)
[ -n "$NEW" ] || exit 1
BK={bundle}.previous
rm -rf "$BK"
mv {bundle} "$BK"
if mv "$NEW" {parent}/; then rm -rf "$BK"; else mv "$BK" {bundle}; fi
/usr/bin/xattr -dr com.apple.quarantine {bundle} 2>/dev/null || true
rm -rf {st} {pkg}
/usr/bin/open {bundle}
"#,
            st = sh_quote(&staging),
            pkg = sh_quote(package),
            bundle = sh_quote(&bundle),
            parent = sh_quote(parent),
        );
        std::process::Command::new("/bin/sh").arg("-c").arg(script).spawn().map_err(|e| format!("无法启动更新：{e}"))?;
        Ok(())
    }

    fn open_url(&self, url: &str) {
        let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    }

    fn data_dir(&self) -> Option<PathBuf> {
        app_support()
    }

    fn start_fullscreen(&self) -> bool {
        false
    }
}
