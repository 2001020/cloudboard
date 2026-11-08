//! CloudBoard for macOS.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
mod record;

#[cfg(target_os = "macos")]
fn main() {
    if let Err(e) = cb_shell::run(mac::MacPlatform, cb_shell::file_args()) {
        eprintln!("CloudBoard: {e}");
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("cloudboard-macos only runs on macOS; use platforms/windows on Windows.");
}
