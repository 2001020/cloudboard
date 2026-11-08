//! CloudBoard for Windows.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod record;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--unregister" || a == "/uninstall") {
        win::unregister_associations();
        return;
    }
    win::init_logging();
    if let Err(e) = cb_shell::run(win::WindowsPlatform::new(), cb_shell::file_args()) {
        log::error!("fatal: {e}");
        win::fatal_box(&e);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("cloudboard-windows only runs on Windows; use platforms/macos on macOS.");
}
