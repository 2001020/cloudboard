//! Update client for GitHub Releases of `2001020/cloudboard`.
//!
//! Each release carries:
//! * `latest.json` – manifest (version, notes, per-platform package URL, SHA-256, size)
//! * `latest.json.sig` – base64 Ed25519 signature of `latest.json`
//! * the packages themselves (`cloudboard-<ver>-windows-x64.zip`, …)
//!
//! The client verifies the manifest signature (when a release key is
//! embedded), then the package hash after download. Downloads resume.

pub mod manifest;

use base64::Engine;
pub use manifest::{AssetInfo, Manifest};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub const DEFAULT_REPO: &str = "2001020/cloudboard";
pub const DEFAULT_API: &str = "https://api.github.com";

/// Release-signing public key (base64, 32 bytes). Generate with
/// `cargo run -p cb-update --bin cb-release -- keygen` and paste the public
/// half here; keep the private half in the `UPDATE_SIGNING_KEY` secret.
/// Empty = signatures not enforced (HTTPS + SHA-256 only).
pub const RELEASE_PUBLIC_KEY: &str = include_str!("../release-public-key.txt");

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("网络错误：{0}")]
    Network(String),
    #[error("更新信息格式错误：{0}")]
    Manifest(String),
    #[error("更新签名校验失败")]
    Signature,
    #[error("安装包校验失败（SHA-256 不匹配）")]
    Checksum,
    #[error("此版本没有适用于本平台的安装包")]
    NoAsset,
    #[error("已取消")]
    Cancelled,
    #[error("文件错误：{0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub repo: String,
    pub api_base: String,
    pub current_version: String,
    /// Key into `Manifest::assets`, e.g. "windows-x64" / "macos-universal".
    pub asset_key: String,
    /// Optional URL prefix for downloads (mirror), e.g. "https://ghproxy.example/".
    pub mirror: String,
    pub public_key: String,
    pub download_dir: PathBuf,
    pub user_agent: String,
    /// Honour HTTPS_PROXY/ALL_PROXY environment variables.
    pub env_proxy: bool,
}

impl Config {
    pub fn new(asset_key: &str, download_dir: PathBuf) -> Config {
        Config {
            repo: DEFAULT_REPO.into(),
            api_base: DEFAULT_API.into(),
            current_version: cb_core::version::APP_VERSION.into(),
            asset_key: asset_key.into(),
            mirror: String::new(),
            public_key: RELEASE_PUBLIC_KEY.trim().into(),
            download_dir,
            user_agent: format!("CloudBoard/{}", cb_core::version::APP_VERSION),
            env_proxy: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: Vec<String>,
    pub mandatory: bool,
    pub asset: AssetInfo,
    pub html_url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateEvent {
    Checking,
    UpToDate,
    Available(Release),
    Progress { done: u64, total: u64 },
    /// Verified package ready to install.
    Ready { package: PathBuf, version: String },
    Failed(String),
}

fn agent(cfg: &Config) -> ureq::Agent {
    let b = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .user_agent(&cfg.user_agent)
        .try_proxy_from_env(cfg.env_proxy);
    #[cfg(any(windows, target_os = "macos"))]
    let b = match native_tls::TlsConnector::new() {
        Ok(t) => b.tls_connector(Arc::new(t)),
        Err(e) => {
            log::warn!("TLS init failed: {e}");
            b
        }
    };
    b.build()
}

fn get_bytes(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let r = agent.get(url).set("Accept", "application/octet-stream").call().map_err(|e| UpdateError::Network(e.to_string()))?;
    let mut v = Vec::new();
    r.into_reader().take(limit).read_to_end(&mut v).map_err(|e| UpdateError::Network(e.to_string()))?;
    Ok(v)
}

pub fn verify_signature(public_key_b64: &str, data: &[u8], sig_b64: &str) -> bool {
    let b64 = base64::engine::general_purpose::STANDARD;
    let (Ok(pk), Ok(sig)) = (b64.decode(public_key_b64.trim()), b64.decode(sig_b64.trim())) else { return false };
    let (Ok(pk), Ok(sig)) = (<[u8; 32]>::try_from(pk.as_slice()), <[u8; 64]>::try_from(sig.as_slice())) else { return false };
    let Ok(vk) = ed25519_dalek::VerifyingKey::from_bytes(&pk) else { return false };
    vk.verify_strict(data, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
}

/// Query GitHub for the latest release and return it if newer.
pub fn check(cfg: &Config) -> Result<Option<Release>, UpdateError> {
    let agent = agent(cfg);
    let url = format!("{}/repos/{}/releases/latest", cfg.api_base.trim_end_matches('/'), cfg.repo);
    let resp = agent.get(&url).set("Accept", "application/vnd.github+json").call();
    let rel: serde_json::Value = match resp {
        Ok(r) => r.into_json().map_err(|e| UpdateError::Manifest(e.to_string()))?,
        Err(ureq::Error::Status(404, _)) => return Ok(None), // no releases yet
        Err(e) => return Err(UpdateError::Network(e.to_string())),
    };
    let assets = rel.get("assets").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let find = |name: &str| {
        assets.iter().find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name)).and_then(|a| a.get("browser_download_url")).and_then(|u| u.as_str()).map(|s| s.to_string())
    };
    let Some(manifest_url) = find("latest.json") else {
        // A release without a manifest: compare by tag only and send the user to the page.
        let tag = rel.get("tag_name").and_then(|t| t.as_str()).unwrap_or("");
        if cb_core::version::is_newer(tag, &cfg.current_version) {
            return Err(UpdateError::Manifest(format!("版本 {tag} 缺少 latest.json")));
        }
        return Ok(None);
    };
    let data = get_bytes(&agent, &manifest_url, 1 << 20)?;
    if !cfg.public_key.is_empty() {
        let sig_url = find("latest.json.sig").ok_or(UpdateError::Signature)?;
        let sig = get_bytes(&agent, &sig_url, 4096)?;
        if !verify_signature(&cfg.public_key, &data, &String::from_utf8_lossy(&sig)) {
            return Err(UpdateError::Signature);
        }
    }
    let m: Manifest = serde_json::from_slice(&data).map_err(|e| UpdateError::Manifest(e.to_string()))?;
    if !cb_core::version::is_newer(&m.version, &cfg.current_version) {
        return Ok(None);
    }
    let asset = m.assets.get(&cfg.asset_key).cloned().ok_or(UpdateError::NoAsset)?;
    let mandatory = m.mandatory || (!m.min_supported.is_empty() && cb_core::version::is_newer(&m.min_supported, &cfg.current_version));
    Ok(Some(Release {
        version: m.version,
        notes: m.notes,
        mandatory,
        asset,
        html_url: rel.get("html_url").and_then(|u| u.as_str()).unwrap_or("").to_string(),
    }))
}

/// Download (resuming a partial file) and verify the package.
pub fn download(cfg: &Config, rel: &Release, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64)) -> Result<PathBuf, UpdateError> {
    std::fs::create_dir_all(&cfg.download_dir)?;
    let name = sanitize(&rel.asset.name);
    let final_path = cfg.download_dir.join(&name);
    if final_path.exists() && sha256_file(&final_path)? == rel.asset.sha256.to_lowercase() {
        return Ok(final_path);
    }
    let part = cfg.download_dir.join(format!("{name}.part"));
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > rel.asset.size && rel.asset.size > 0 {
        std::fs::remove_file(&part)?;
        have = 0;
    }
    let url = format!("{}{}", cfg.mirror, rel.asset.url);
    let agent = agent(cfg);
    let mut req = agent.get(&url);
    if have > 0 {
        req = req.set("Range", &format!("bytes={have}-"));
    }
    let resp = req.call().map_err(|e| UpdateError::Network(e.to_string()))?;
    let resumed = resp.status() == 206;
    if !resumed {
        have = 0;
    }
    let total = if rel.asset.size > 0 { rel.asset.size } else { resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).map(|n| n + have).unwrap_or(0) };
    let mut file = std::fs::OpenOptions::new().create(true).write(true).append(resumed).truncate(!resumed).open(&part)?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = have;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(UpdateError::Cancelled);
        }
        let n = reader.read(&mut buf).map_err(|e| UpdateError::Network(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    file.flush()?;
    drop(file);
    if sha256_file(&part)? != rel.asset.sha256.to_lowercase() {
        let _ = std::fs::remove_file(&part);
        return Err(UpdateError::Checksum);
    }
    std::fs::rename(&part, &final_path)?;
    Ok(final_path)
}

fn sanitize(name: &str) -> String {
    let n: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    if n.is_empty() || n.starts_with('.') {
        "update.zip".into()
    } else {
        n
    }
}

pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Background updater: runs checks/downloads on worker threads and reports
/// through `poll()`.
pub struct Updater {
    cfg: Arc<Mutex<Config>>,
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    waker: Arc<dyn Fn() + Send + Sync>,
}

impl Updater {
    pub fn new(cfg: Config, waker: Arc<dyn Fn() + Send + Sync>) -> Updater {
        let (tx, rx) = channel();
        Updater { cfg: Arc::new(Mutex::new(cfg)), tx, rx, cancel: Arc::new(AtomicBool::new(false)), busy: Arc::new(AtomicBool::new(false)), waker }
    }

    pub fn set_mirror(&self, m: &str) {
        self.cfg.lock().unwrap().mirror = m.to_string();
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }

    pub fn check(&self) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let (cfg, tx, busy, w) = (self.cfg.lock().unwrap().clone(), self.tx.clone(), self.busy.clone(), self.waker.clone());
        let _ = tx.send(UpdateEvent::Checking);
        std::thread::spawn(move || {
            let ev = match check(&cfg) {
                Ok(Some(r)) => UpdateEvent::Available(r),
                Ok(None) => UpdateEvent::UpToDate,
                Err(e) => UpdateEvent::Failed(e.to_string()),
            };
            busy.store(false, Ordering::SeqCst);
            let _ = tx.send(ev);
            w();
        });
    }

    pub fn download(&self, rel: Release) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.cancel.store(false, Ordering::SeqCst);
        let (cfg, tx, busy, w, cancel) = (self.cfg.lock().unwrap().clone(), self.tx.clone(), self.busy.clone(), self.waker.clone(), self.cancel.clone());
        std::thread::spawn(move || {
            let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
            let r = download(&cfg, &rel, &cancel, |d, t| {
                if last.elapsed().as_millis() > 100 {
                    last = std::time::Instant::now();
                    let _ = tx.send(UpdateEvent::Progress { done: d, total: t });
                    w();
                }
            });
            busy.store(false, Ordering::SeqCst);
            let _ = tx.send(match r {
                Ok(p) => UpdateEvent::Ready { package: p, version: rel.version.clone() },
                Err(e) => UpdateEvent::Failed(e.to_string()),
            });
            w();
        });
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn poll(&self) -> Option<UpdateEvent> {
        self.rx.try_recv().ok()
    }
}
