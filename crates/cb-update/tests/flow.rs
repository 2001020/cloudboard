//! End-to-end update flow against a local mock of the GitHub API.

use base64::Engine;
use cb_update::{check, download, AssetInfo, Config, Manifest, UpdateError};
use ed25519_dalek::{Signer, SigningKey};
use sha2::Digest;
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

type Routes = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// Minimal HTTP/1.1 server supporting Range requests.
fn serve(routes: Routes) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let routes = routes.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                let mut range: Option<u64> = None;
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("range: bytes=") {
                        range = v.trim().trim_end_matches('-').parse().ok();
                    }
                }
                let body = routes.lock().unwrap().get(&path).cloned();
                match body {
                    None => {
                        let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    }
                    Some(b) => {
                        let (status, slice) = match range {
                            Some(off) if (off as usize) < b.len() => ("206 Partial Content", &b[off as usize..]),
                            _ => ("200 OK", &b[..]),
                        };
                        let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", slice.len());
                        let _ = s.write_all(slice);
                    }
                }
                let _ = r.read(&mut [0u8; 0]);
            });
        }
    });
    addr
}

struct Fixture {
    cfg: Config,
    routes: Routes,
    sk: SigningKey,
    package: Vec<u8>,
}

fn fixture(version: &str) -> Fixture {
    let routes: Routes = Arc::new(Mutex::new(HashMap::new()));
    let base = serve(routes.clone());
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let package: Vec<u8> = (0..300_000u32).map(|i| (i * 31 % 251) as u8).collect();
    let sha = hex::encode(sha2::Sha256::digest(&package));
    let mut assets = BTreeMap::new();
    assets.insert("windows-x64".to_string(), AssetInfo { name: "cloudboard-27.1.0-windows-x64.zip".into(), url: format!("{base}/dl/pkg.zip"), sha256: sha, size: package.len() as u64 });
    let m = Manifest { version: version.into(), notes: vec!["改进".into()], min_supported: String::new(), mandatory: false, published: String::new(), assets };
    let mj = serde_json::to_vec(&m).unwrap();
    let sig = base64::engine::general_purpose::STANDARD.encode(sk.sign(&mj).to_bytes());
    let release = serde_json::json!({
        "tag_name": format!("v{version}"),
        "html_url": format!("{base}/release"),
        "assets": [
            {"name": "latest.json", "browser_download_url": format!("{base}/dl/latest.json")},
            {"name": "latest.json.sig", "browser_download_url": format!("{base}/dl/latest.json.sig")},
        ]
    });
    {
        let mut r = routes.lock().unwrap();
        r.insert("/repos/2001020/cloudboard/releases/latest".into(), serde_json::to_vec(&release).unwrap());
        r.insert("/dl/latest.json".into(), mj);
        r.insert("/dl/latest.json.sig".into(), sig.into_bytes());
        r.insert("/dl/pkg.zip".into(), package.clone());
    }
    let dir = std::env::temp_dir().join(format!("cbupd-{}-{}", std::process::id(), rand_suffix()));
    let mut cfg = Config::new("windows-x64", dir);
    cfg.api_base = base;
    cfg.current_version = "27.0.0".into();
    cfg.env_proxy = false;
    cfg.public_key = base64::engine::general_purpose::STANDARD.encode(sk.verifying_key().to_bytes());
    Fixture { cfg, routes, sk, package }
}

fn rand_suffix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64
}

#[test]
fn full_flow_with_resume() {
    let f = fixture("27.1.0");
    let rel = check(&f.cfg).unwrap().expect("update available");
    assert_eq!(rel.version, "27.1.0");
    // Simulate an interrupted earlier download.
    std::fs::create_dir_all(&f.cfg.download_dir).unwrap();
    std::fs::write(f.cfg.download_dir.join("cloudboard-27.1.0-windows-x64.zip.part"), &f.package[..100_000]).unwrap();
    let mut last = 0;
    let p = download(&f.cfg, &rel, &AtomicBool::new(false), |d, t| {
        assert!(d >= 100_000 && t == 300_000);
        last = d;
    })
    .unwrap();
    assert_eq!(last, 300_000);
    assert_eq!(std::fs::read(&p).unwrap(), f.package);
    let _ = std::fs::remove_dir_all(&f.cfg.download_dir);
    let _ = f.sk;
}

#[test]
fn up_to_date_and_bad_signature() {
    let f = fixture("27.0.0");
    assert!(check(&f.cfg).unwrap().is_none());
    let f = fixture("27.2.0");
    f.routes.lock().unwrap().insert("/dl/latest.json.sig".into(), b"AAAA".to_vec());
    assert!(matches!(check(&f.cfg), Err(UpdateError::Signature)));
}

#[test]
fn corrupted_package_rejected() {
    let f = fixture("27.1.0");
    let rel = check(&f.cfg).unwrap().unwrap();
    f.routes.lock().unwrap().insert("/dl/pkg.zip".into(), vec![0u8; 300_000]);
    assert!(matches!(download(&f.cfg, &rel, &AtomicBool::new(false), |_, _| {}), Err(UpdateError::Checksum)));
    assert!(!f.cfg.download_dir.join("cloudboard-27.1.0-windows-x64.zip").exists());
    let _ = std::fs::remove_dir_all(&f.cfg.download_dir);
}

#[test]
fn cancel_stops_download() {
    let f = fixture("27.1.0");
    let rel = check(&f.cfg).unwrap().unwrap();
    assert!(matches!(download(&f.cfg, &rel, &AtomicBool::new(true), |_, _| {}), Err(UpdateError::Cancelled)));
    let _ = std::fs::remove_dir_all(&f.cfg.download_dir);
}

#[test]
fn no_releases_is_up_to_date() {
    let f = fixture("27.1.0");
    f.routes.lock().unwrap().remove("/repos/2001020/cloudboard/releases/latest");
    assert!(check(&f.cfg).unwrap().is_none());
}
