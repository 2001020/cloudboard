//! Windows integration: pen input (WM_POINTER), mobile hotspot (WinRT
//! tethering), dialogs, sounds, file associations and update hand-off.

use cb_shell::winit::event_loop::EventLoopProxy;
use cb_shell::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use cb_shell::winit::window::Window;
use cb_shell::{Platform, UserEvent};
use cb_ui::input::{Button, Event, HotspotResult, Pointer, PointerKind, Sound};
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
use windows::Win32::UI::Input::Pointer::{GetPointerPenInfo, GetPointerType, POINTER_FLAG_INCONTACT, POINTER_PEN_INFO};
use windows::Win32::UI::Shell::{DefSubclassProc, SHChangeNotify, SetWindowSubclass, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_OK, POINTER_INPUT_TYPE, PT_PEN, WM_POINTERDOWN, WM_POINTERLEAVE, WM_POINTERUP, WM_POINTERUPDATE,
};

const PEN_FLAG_BARREL: u32 = 0x1;
const PEN_FLAG_INVERTED: u32 = 0x2;
const PEN_FLAG_ERASER: u32 = 0x4;

/// The face name Windows uses for message boxes and dialogs (the system UI
/// font for the current language, e.g. "Microsoft YaHei UI" on Chinese).
fn system_message_font() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut m = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
    unsafe { SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, m.cbSize, Some(&mut m as *mut _ as *mut _), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) }.ok()?;
    let face = &m.lfMessageFont.lfFaceName;
    let n = face.iter().position(|&c| c == 0).unwrap_or(face.len());
    let name = String::from_utf16_lossy(&face[..n]);
    (!name.is_empty()).then_some(name)
}

pub struct WindowsPlatform {
    hotspot_started_by_us: Mutex<bool>,
}

impl WindowsPlatform {
    pub fn new() -> WindowsPlatform {
        WindowsPlatform { hotspot_started_by_us: Mutex::new(false) }
    }
}

fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("CloudBoard"))
}

pub fn init_logging() {
    struct FileLog(Mutex<std::fs::File>);
    impl log::Log for FileLog {
        fn enabled(&self, m: &log::Metadata) -> bool {
            m.level() <= log::Level::Info
        }
        fn log(&self, r: &log::Record) {
            if self.enabled(r.metadata()) {
                if let Ok(mut f) = self.0.lock() {
                    let _ = writeln!(f, "[{}] {}: {}", r.level(), r.target(), r.args());
                }
            }
        }
        fn flush(&self) {}
    }
    if let Some(dir) = local_app_data() {
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(f) = std::fs::File::create(dir.join("cloudboard.log")) {
            let _ = log::set_boxed_logger(Box::new(FileLog(Mutex::new(f))));
            log::set_max_level(log::LevelFilter::Info);
        }
    }
}

pub fn fatal_box(msg: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(format!("CloudBoard 无法启动：\n{msg}")), w!("CloudBoard"), MB_OK | MB_ICONERROR);
    }
}

// ------------------------------------------------------------------ pen input

struct PenState {
    proxy: EventLoopProxy<UserEvent>,
    down: bool,
}

unsafe extern "system" fn pen_subclass(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM, _id: usize, data: usize) -> LRESULT {
    if matches!(msg, WM_POINTERDOWN | WM_POINTERUPDATE | WM_POINTERUP | WM_POINTERLEAVE) {
        let pid = (wparam.0 & 0xFFFF) as u32;
        let mut ty = POINTER_INPUT_TYPE::default();
        if GetPointerType(pid, &mut ty).is_ok() && ty == PT_PEN {
            let st = &mut *(data as *mut PenState);
            let mut info = POINTER_PEN_INFO::default();
            if GetPointerPenInfo(pid, &mut info).is_ok() {
                let mut pt: POINT = info.pointerInfo.ptPixelLocation;
                let _ = ScreenToClient(hwnd, &mut pt);
                let pressure = if info.pressure > 0 { (info.pressure as f32 / 1024.0).clamp(0.0, 1.0) } else { 1.0 };
                let flags = info.penFlags;
                let eraser = flags & (PEN_FLAG_ERASER | PEN_FLAG_INVERTED) != 0;
                let barrel = flags & PEN_FLAG_BARREL != 0;
                let contact = info.pointerInfo.pointerFlags.contains(POINTER_FLAG_INCONTACT);
                let p = Pointer { id: 1_000_000 + pid as u64, kind: PointerKind::Pen, x: pt.x as f32, y: pt.y as f32, pressure, eraser: eraser || barrel, button: Button::Primary };
                let ev = match msg {
                    WM_POINTERDOWN => {
                        st.down = true;
                        Some(Event::PointerDown(p))
                    }
                    WM_POINTERUPDATE if st.down && contact => Some(Event::PointerMove(p)),
                    WM_POINTERUPDATE if st.down && !contact => {
                        st.down = false;
                        Some(Event::PointerUp(p))
                    }
                    WM_POINTERUP | WM_POINTERLEAVE if st.down => {
                        st.down = false;
                        Some(Event::PointerUp(p))
                    }
                    _ => None,
                };
                if let Some(e) = ev {
                    let _ = st.proxy.send_event(UserEvent::Input(e));
                }
            }
            // Consumed: no touch/mouse emulation for pens.
            return LRESULT(0);
        }
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

fn hwnd_of(window: &Window) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut _)),
        _ => None,
    }
}

// ------------------------------------------------------------------ hotspot

fn hotspot_start() -> Result<(String, String), HotspotResult> {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::{NetworkOperatorTetheringManager, TetheringCapability, TetheringOperationStatus, TetheringOperationalState};
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    let unsupported = |m: &str| HotspotResult::Unsupported(m.to_string());
    let profile = NetworkInformation::GetInternetConnectionProfile().map_err(|_| unsupported("电脑当前没有可共享的网络连接"))?;
    let cap = NetworkOperatorTetheringManager::GetTetheringCapabilityFromConnectionProfile(&profile).map_err(|e| unsupported(&e.message().to_string()))?;
    if cap != TetheringCapability::Enabled {
        return Err(unsupported(match cap {
            TetheringCapability::DisabledByGroupPolicy => "移动热点已被组策略禁用",
            TetheringCapability::DisabledByHardwareLimitation => "无线网卡不支持移动热点",
            TetheringCapability::DisabledDueToUnknownCause => "移动热点不可用",
            _ => "移动热点不可用",
        }));
    }
    let mgr = NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(|e| HotspotResult::Failed(e.message().to_string()))?;
    let cfg = mgr.GetCurrentAccessPointConfiguration().map_err(|e| HotspotResult::Failed(e.message().to_string()))?;
    let ssid = cfg.Ssid().map(|s| s.to_string()).unwrap_or_default();
    let pass = cfg.Passphrase().map(|s| s.to_string()).unwrap_or_default();
    if mgr.TetheringOperationalState().ok() != Some(TetheringOperationalState::On) {
        let op = mgr.StartTetheringAsync().map_err(|e| HotspotResult::Failed(e.message().to_string()))?;
        let r = op.get().map_err(|e| HotspotResult::Failed(e.message().to_string()))?;
        if r.Status().ok() != Some(TetheringOperationStatus::Success) {
            let msg = r.AdditionalErrorMessage().map(|s| s.to_string()).unwrap_or_default();
            return Err(HotspotResult::Failed(if msg.is_empty() { "无法开启移动热点".into() } else { msg }));
        }
    }
    Ok((ssid, pass))
}

fn hotspot_stop() {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::NetworkOperatorTetheringManager;
    if let Ok(p) = NetworkInformation::GetInternetConnectionProfile() {
        if let Ok(m) = NetworkOperatorTetheringManager::CreateFromConnectionProfile(&p) {
            if let Ok(op) = m.StopTetheringAsync() {
                let _ = op.get();
            }
        }
    }
}

// ------------------------------------------------------------------ associations

const ASSOC_EXTS: [&str; 1] = [".cloudboard"];
const OFFICE_EXTS: [&str; 3] = [".pptx", ".ppsx", ".docx"];

fn register_associations() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.to_string_lossy().to_string();
    let hk = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((classes, _)) = hk.create_subkey("Software\\Classes") else { return };
    let cmd = format!("\"{exe}\" \"%1\"");
    let icon = format!("\"{exe}\",0");
    for (progid, desc) in [("CloudBoard.Project", "CloudBoard 白板工程"), ("CloudBoard.PDF", "PDF 文档（CloudBoard）"), ("CloudBoard.Office", "Office 文档（CloudBoard）")] {
        if let Ok((k, _)) = classes.create_subkey(progid) {
            let _ = k.set_value("", &desc);
            if let Ok((i, _)) = k.create_subkey("DefaultIcon") {
                let _ = i.set_value("", &icon);
            }
            if let Ok((c, _)) = k.create_subkey("shell\\open\\command") {
                let _ = c.set_value("", &cmd);
            }
        }
    }
    for ext in ASSOC_EXTS {
        if let Ok((k, _)) = classes.create_subkey(ext) {
            let _ = k.set_value("", &"CloudBoard.Project");
        }
    }
    if let Ok((k, _)) = classes.create_subkey(".pdf\\OpenWithProgids") {
        let _ = k.set_value("CloudBoard.PDF", &"");
    }
    // PowerPoint/Word: offered under "Open with" only, never as the default.
    for ext in OFFICE_EXTS {
        if let Ok((k, _)) = classes.create_subkey(format!("{ext}\\OpenWithProgids")) {
            let _ = k.set_value("CloudBoard.Office", &"");
        }
    }
    if let Ok((k, _)) = classes.create_subkey("Applications\\cloudboard.exe\\SupportedTypes") {
        for e in [".cloudboard", ".pdf", ".pptx", ".ppsx", ".docx", ".png", ".jpg", ".jpeg"] {
            let _ = k.set_value(e, &"");
        }
    }
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

pub fn unregister_associations() {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;
    let hk = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(classes) = hk.open_subkey_with_flags("Software\\Classes", winreg::enums::KEY_ALL_ACCESS) {
        let _ = classes.delete_subkey_all("CloudBoard.Project");
        let _ = classes.delete_subkey_all("CloudBoard.PDF");
        let _ = classes.delete_subkey_all("CloudBoard.Office");
        for ext in OFFICE_EXTS {
            if let Ok(k) = classes.open_subkey_with_flags(format!("{ext}\\OpenWithProgids"), winreg::enums::KEY_ALL_ACCESS) {
                let _ = k.delete_value("CloudBoard.Office");
            }
        }
        let _ = classes.delete_subkey_all("Applications\\cloudboard.exe");
        if let Ok(k) = classes.open_subkey_with_flags(".pdf\\OpenWithProgids", winreg::enums::KEY_ALL_ACCESS) {
            let _ = k.delete_value("CloudBoard.PDF");
        }
        if let Ok(k) = classes.open_subkey_with_flags(".cloudboard", winreg::enums::KEY_ALL_ACCESS) {
            if k.get_value::<String, _>("").ok().as_deref() == Some("CloudBoard.Project") {
                drop(k);
                let _ = classes.delete_subkey_all(".cloudboard");
            }
        }
    }
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

// ------------------------------------------------------------------ Platform

impl Platform for WindowsPlatform {
    fn name(&self) -> &'static str {
        "windows"
    }

    fn ui_font_families(&self) -> Vec<String> {
        let mut v = Vec::new();
        if let Some(f) = system_message_font() {
            v.push(f);
        }
        for f in ["Microsoft YaHei UI", "Segoe UI", "Microsoft YaHei"] {
            if !v.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                v.push(f.to_string());
            }
        }
        v
    }

    fn update_asset(&self) -> String {
        if cfg!(target_arch = "aarch64") { "windows-arm64" } else { "windows-x64" }.into()
    }

    fn can_hotspot(&self) -> bool {
        true
    }

    fn start_hotspot(&self, reply: Box<dyn FnOnce(HotspotResult) + Send>) {
        let flag = &self.hotspot_started_by_us as *const Mutex<bool> as usize;
        std::thread::spawn(move || {
            let r = match hotspot_start() {
                Ok((ssid, passphrase)) => {
                    // SAFETY: the platform object lives for the whole program.
                    unsafe { *(*(flag as *const Mutex<bool>)).lock().unwrap() = true };
                    HotspotResult::Started { ssid, passphrase }
                }
                Err(e) => e,
            };
            reply(r);
        });
    }

    fn stop_hotspot(&self) {
        let mut f = self.hotspot_started_by_us.lock().unwrap();
        if *f {
            *f = false;
            std::thread::spawn(hotspot_stop);
        }
    }

    fn play_sound(&self, s: Sound) {
        let alias = match s {
            Sound::Alarm => w!("SystemExclamation"),
            Sound::Tick => w!("SystemAsterisk"),
        };
        unsafe {
            let _ = PlaySoundW(alias, None, SND_ALIAS | SND_ASYNC | SND_NODEFAULT);
        }
    }

    fn open_dialog(&self, window: &Window) -> Option<PathBuf> {
        rfd::FileDialog::new()
            .set_title("打开")
            .add_filter("所有支持的文件", &["cloudboard", "pdf", "pptx", "ppsx", "docx", "ppt", "doc", "png", "jpg", "jpeg", "bmp", "gif"])
            .add_filter("CloudBoard 工程", &["cloudboard"])
            .add_filter("PDF 文档", &["pdf"])
            .add_filter("PowerPoint 演示文稿", &["pptx", "ppsx", "ppt"])
            .add_filter("Word 文档", &["docx", "doc"])
            .add_filter("图片", &["png", "jpg", "jpeg", "bmp", "gif"])
            .set_parent(window)
            .pick_file()
    }

    fn recording_backend(&self) -> Option<cb_record::Backend> {
        Some(crate::record::backend())
    }

    fn save_video_dialog(&self, window: &Window, default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("保存录制").add_filter("MP4 视频", &["mp4"]).set_file_name(default_name).set_parent(window).save_file()
    }

    fn utc_offset_minutes(&self) -> i32 {
        use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        let mut tz = TIME_ZONE_INFORMATION::default();
        let r = unsafe { GetTimeZoneInformation(&mut tz) };
        // 2 = TIME_ZONE_ID_DAYLIGHT
        let bias = tz.Bias + if r == 2 { tz.DaylightBias } else { tz.StandardBias };
        -bias
    }

    fn save_dialog(&self, window: &Window, default_name: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title("保存白板").add_filter("CloudBoard 工程", &["cloudboard"]).set_file_name(default_name).set_parent(window).save_file()
    }

    fn install_update(&self, package: &Path) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let dir = exe.parent().ok_or("无法确定安装目录")?.to_path_buf();
        let updater = dir.join("cloudboard-updater.exe");
        if !updater.exists() {
            return Err("缺少 cloudboard-updater.exe，请重新安装 CloudBoard。".into());
        }
        // Run the updater from a temp copy so it can replace itself.
        let tmp = std::env::temp_dir().join(format!("cloudboard-updater-{}.exe", std::process::id()));
        std::fs::copy(&updater, &tmp).map_err(|e| format!("无法准备更新程序：{e}"))?;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        std::process::Command::new(&tmp)
            .arg("--pid")
            .arg(std::process::id().to_string())
            .arg("--package")
            .arg(package)
            .arg("--dir")
            .arg(&dir)
            .arg("--exe")
            .arg(exe.file_name().unwrap_or_default())
            .creation_flags(DETACHED_PROCESS)
            .spawn()
            .map_err(|e| format!("无法启动更新程序：{e}"))?;
        Ok(())
    }

    fn open_url(&self, url: &str) {
        let _ = std::process::Command::new("explorer").arg(url).spawn();
    }

    fn on_window_created(&self, window: &Window, proxy: EventLoopProxy<UserEvent>) {
        if let Some(hwnd) = hwnd_of(window) {
            let state = Box::into_raw(Box::new(PenState { proxy, down: false }));
            unsafe {
                let _ = SetWindowSubclass(hwnd, Some(pen_subclass), 0xCB27, state as usize);
            }
        }
    }

    fn startup(&self) {
        std::thread::spawn(register_associations);
    }

    fn data_dir(&self) -> Option<PathBuf> {
        local_app_data()
    }
}

#[allow(dead_code)]
fn _pcwstr_unused(_: PCWSTR) {}
