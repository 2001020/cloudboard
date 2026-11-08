//! cloudboard-updater: waits for CloudBoard to exit, replaces the installed
//! files with the contents of a verified update package (rolling back on any
//! error), then restarts CloudBoard.
//!
//!   cloudboard-updater --pid 1234 --package update.zip --dir "C:\...\CloudBoard" --exe cloudboard.exe
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

struct Args {
    pid: u32,
    package: PathBuf,
    dir: PathBuf,
    exe: String,
}

fn parse() -> Result<Args, String> {
    let mut it = std::env::args_os().skip(1);
    let (mut pid, mut package, mut dir, mut exe) = (0, None, None, String::from("cloudboard.exe"));
    while let Some(a) = it.next() {
        let v = it.next().ok_or("missing value")?;
        match a.to_string_lossy().as_ref() {
            "--pid" => pid = v.to_string_lossy().parse().map_err(|_| "bad pid")?,
            "--package" => package = Some(PathBuf::from(v)),
            "--dir" => dir = Some(PathBuf::from(v)),
            "--exe" => exe = v.to_string_lossy().to_string(),
            o => return Err(format!("unknown argument {o}")),
        }
    }
    Ok(Args { pid, package: package.ok_or("--package required")?, dir: dir.ok_or("--dir required")?, exe })
}

struct Log(Option<fs::File>);
impl Log {
    fn line(&mut self, s: &str) {
        if let Some(f) = &mut self.0 {
            let _ = writeln!(f, "{s}");
        }
    }
}

#[cfg(windows)]
fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(h) => {
                let r = WaitForSingleObject(h, timeout.as_millis() as u32);
                let _ = CloseHandle(h);
                r == WAIT_OBJECT_0
            }
            Err(_) => true, // already gone
        }
    }
}

#[cfg(not(windows))]
fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let t0 = Instant::now();
    while Path::new(&format!("/proc/{pid}")).exists() {
        if t0.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

/// Common leading directory of all entries (packages may wrap files in a folder).
fn common_prefix(names: &[String]) -> Option<String> {
    let first = names.first()?.split('/').next()?.to_string();
    if names.iter().all(|n| n.starts_with(&format!("{first}/"))) {
        Some(format!("{first}/"))
    } else {
        None
    }
}

fn safe_rel(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return None;
    }
    Some(p.to_path_buf())
}

/// Extract the package into `staging`, returning the relative file list.
fn extract(package: &Path, staging: &Path) -> io::Result<Vec<PathBuf>> {
    let f = fs::File::open(package)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let names: Vec<String> = (0..z.len()).filter_map(|i| z.by_index(i).ok().map(|e| e.name().to_string())).filter(|n| !n.ends_with('/')).collect();
    let prefix = common_prefix(&names);
    let mut files = Vec::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if e.is_dir() {
            continue;
        }
        let name = e.name().to_string();
        let name = prefix.as_ref().and_then(|p| name.strip_prefix(p.as_str())).unwrap_or(&name).to_string();
        let Some(rel) = safe_rel(&name) else { continue };
        let out = staging.join(&rel);
        if let Some(d) = out.parent() {
            fs::create_dir_all(d)?;
        }
        let mut o = fs::File::create(&out)?;
        io::copy(&mut e, &mut o)?;
        files.push(rel);
    }
    Ok(files)
}

/// Move files from staging into `dir`, backing up replaced files. On error,
/// everything moved so far is rolled back.
fn apply(dir: &Path, staging: &Path, backup: &Path, files: &[PathBuf], log: &mut Log) -> io::Result<()> {
    let mut done: Vec<(PathBuf, bool)> = Vec::new();
    let result = (|| -> io::Result<()> {
        for rel in files {
            let dst = dir.join(rel);
            let had = dst.exists();
            if had {
                let b = backup.join(rel);
                if let Some(d) = b.parent() {
                    fs::create_dir_all(d)?;
                }
                // Retry: the old process may still release file handles briefly.
                let mut tries = 0;
                loop {
                    match fs::rename(&dst, &b) {
                        Ok(()) => break,
                        Err(e) if tries < 50 => {
                            tries += 1;
                            log.line(&format!("retry backup {}: {e}", rel.display()));
                            std::thread::sleep(Duration::from_millis(200));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            if let Some(d) = dst.parent() {
                fs::create_dir_all(d)?;
            }
            fs::rename(staging.join(rel), &dst).or_else(|_| fs::copy(staging.join(rel), &dst).map(|_| ()))?;
            done.push((rel.clone(), had));
        }
        Ok(())
    })();
    if let Err(e) = &result {
        log.line(&format!("apply failed: {e}; rolling back {} files", done.len()));
        for (rel, had) in done.iter().rev() {
            let dst = dir.join(rel);
            let _ = fs::remove_file(&dst);
            if *had {
                let _ = fs::rename(backup.join(rel), &dst);
            }
        }
    }
    result
}

fn run(a: &Args, log: &mut Log) -> Result<(), String> {
    log.line(&format!("waiting for pid {}", a.pid));
    if !wait_for_exit(a.pid, Duration::from_secs(60)) {
        return Err("CloudBoard 未能在 60 秒内退出".into());
    }
    let staging = a.dir.join(".update-staging");
    let backup = a.dir.join(".update-backup");
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&backup);
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    let files = extract(&a.package, &staging).map_err(|e| format!("解压失败：{e}"))?;
    if !files.iter().any(|f| f.to_string_lossy().eq_ignore_ascii_case(&a.exe)) {
        return Err(format!("更新包中缺少 {}", a.exe));
    }
    log.line(&format!("extracted {} files", files.len()));
    apply(&a.dir, &staging, &backup, &files, log).map_err(|e| format!("替换文件失败：{e}"))?;
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir_all(&backup);
    let _ = fs::remove_file(&a.package);
    log.line("update applied");
    Ok(())
}

#[cfg(windows)]
fn message(text: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("CloudBoard 更新"), MB_OK | MB_ICONWARNING);
    }
}

#[cfg(not(windows))]
fn message(text: &str) {
    eprintln!("{text}");
}

fn main() {
    let a = match parse() {
        Ok(a) => a,
        Err(e) => {
            message(&format!("参数错误：{e}"));
            std::process::exit(2);
        }
    };
    let log_path = std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("CloudBoard").join("update.log")).unwrap_or_else(|| std::env::temp_dir().join("cloudboard-update.log"));
    let _ = fs::create_dir_all(log_path.parent().unwrap());
    let mut log = Log(fs::File::create(&log_path).ok());
    let t0 = Instant::now();
    let r = run(&a, &mut log);
    log.line(&format!("finished in {:?}: {:?}", t0.elapsed(), r));
    if let Err(e) = &r {
        message(&format!("更新未完成，已保留原有版本。\n{e}"));
    }
    // Restart CloudBoard either way (new or rolled-back version).
    let _ = std::process::Command::new(a.dir.join(&a.exe)).current_dir(&a.dir).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let f = fs::File::create(path).unwrap();
        let mut z = zip::ZipWriter::new(f);
        for (n, d) in files {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(d).unwrap();
        }
        z.finish().unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cbupd-{}-{}-{}", name, std::process::id(), Instant::now().elapsed().as_nanos()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn applies_package_with_wrapper_folder() {
        let dir = tmp("app");
        fs::write(dir.join("cloudboard.exe"), b"old").unwrap();
        fs::write(dir.join("keep.txt"), b"keep").unwrap();
        let pkg = dir.join("../pkg-a.zip");
        make_zip(&pkg, &[("cloudboard-27.1.0/cloudboard.exe", b"new"), ("cloudboard-27.1.0/pdfium.dll", b"dll"), ("cloudboard-27.1.0/../evil.txt", b"x")]);
        let a = Args { pid: 999_999, package: pkg.clone(), dir: dir.clone(), exe: "cloudboard.exe".into() };
        run(&a, &mut Log(None)).unwrap();
        assert_eq!(fs::read(dir.join("cloudboard.exe")).unwrap(), b"new");
        assert_eq!(fs::read(dir.join("pdfium.dll")).unwrap(), b"dll");
        assert_eq!(fs::read(dir.join("keep.txt")).unwrap(), b"keep");
        assert!(!dir.join("../evil.txt").exists());
        assert!(!dir.join(".update-backup").exists());
    }

    #[test]
    fn rejects_package_without_exe() {
        let dir = tmp("app2");
        fs::write(dir.join("cloudboard.exe"), b"old").unwrap();
        let pkg = dir.join("../pkg-b.zip");
        make_zip(&pkg, &[("readme.txt", b"hi")]);
        let a = Args { pid: 999_999, package: pkg, dir: dir.clone(), exe: "cloudboard.exe".into() };
        assert!(run(&a, &mut Log(None)).is_err());
        assert_eq!(fs::read(dir.join("cloudboard.exe")).unwrap(), b"old");
    }

    #[test]
    fn rollback_restores_files() {
        let dir = tmp("app3");
        let staging = dir.join("st");
        let backup = dir.join("bk");
        fs::create_dir_all(&staging).unwrap();
        fs::write(dir.join("a.txt"), b"old-a").unwrap();
        fs::write(staging.join("a.txt"), b"new-a").unwrap();
        // b.txt missing from staging -> apply fails after a.txt was replaced.
        let files = vec![PathBuf::from("a.txt"), PathBuf::from("b.txt")];
        assert!(apply(&dir, &staging, &backup, &files, &mut Log(None)).is_err());
        assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"old-a");
    }
}
