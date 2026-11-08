//! Release helper used by CI and maintainers.
//!
//!   cb-release keygen
//!       Print a new Ed25519 key pair (base64). Put the public key into
//!       crates/cb-update/release-public-key.txt and the private key into the
//!       GitHub secret UPDATE_SIGNING_KEY.
//!   cb-release manifest --version 27.0.1 --base-url URL [--notes FILE]
//!                       [--min-supported VER] [--mandatory] --asset KEY=PATH ... > latest.json
//!   cb-release sign FILE        (key from env UPDATE_SIGNING_KEY) > FILE.sig
//!   cb-release verify FILE SIG PUBLIC_KEY

use base64::Engine;
use cb_update::{AssetInfo, Manifest};
use ed25519_dalek::{Signer, SigningKey};
use std::collections::BTreeMap;
use std::process::exit;

fn die(msg: &str) -> ! {
    eprintln!("cb-release: {msg}");
    exit(2)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let b64 = base64::engine::general_purpose::STANDARD;
    match args.first().map(|s| s.as_str()) {
        Some("keygen") => {
            let sk = SigningKey::generate(&mut rand::rngs::OsRng);
            println!("public:  {}", b64.encode(sk.verifying_key().to_bytes()));
            println!("private: {}", b64.encode(sk.to_bytes()));
        }
        Some("sign") => {
            let file = args.get(1).unwrap_or_else(|| die("sign FILE"));
            let key = std::env::var("UPDATE_SIGNING_KEY").unwrap_or_else(|_| die("UPDATE_SIGNING_KEY not set"));
            let kb = b64.decode(key.trim()).unwrap_or_else(|_| die("bad key"));
            let kb: [u8; 32] = kb.try_into().unwrap_or_else(|_| die("key must be 32 bytes"));
            let sk = SigningKey::from_bytes(&kb);
            let data = std::fs::read(file).unwrap_or_else(|e| die(&e.to_string()));
            println!("{}", b64.encode(sk.sign(&data).to_bytes()));
        }
        Some("verify") => {
            let (f, s, k) = (args.get(1), args.get(2), args.get(3));
            let (Some(f), Some(s), Some(k)) = (f, s, k) else { die("verify FILE SIG PUBLIC_KEY") };
            let data = std::fs::read(f).unwrap_or_else(|e| die(&e.to_string()));
            let sig = std::fs::read_to_string(s).unwrap_or_else(|e| die(&e.to_string()));
            if cb_update::verify_signature(k, &data, &sig) {
                println!("OK");
            } else {
                die("signature INVALID")
            }
        }
        Some("manifest") => {
            let mut version = String::new();
            let mut base = String::new();
            let mut notes = Vec::new();
            let mut min_supported = String::new();
            let mut mandatory = false;
            let mut assets = BTreeMap::new();
            let mut i = 1;
            while i < args.len() {
                let next = |i: &mut usize| -> String {
                    *i += 1;
                    args.get(*i).cloned().unwrap_or_else(|| die("missing value"))
                };
                match args[i].as_str() {
                    "--version" => version = next(&mut i).trim_start_matches('v').to_string(),
                    "--base-url" => base = next(&mut i),
                    "--min-supported" => min_supported = next(&mut i),
                    "--mandatory" => mandatory = true,
                    "--notes" => {
                        let t = std::fs::read_to_string(next(&mut i)).unwrap_or_default();
                        notes = t.lines().map(|l| l.trim().trim_start_matches(['-', '*']).trim().to_string()).filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
                    }
                    "--asset" => {
                        let v = next(&mut i);
                        let (k, p) = v.split_once('=').unwrap_or_else(|| die("--asset KEY=PATH"));
                        let name = std::path::Path::new(p).file_name().unwrap().to_string_lossy().to_string();
                        let size = std::fs::metadata(p).unwrap_or_else(|e| die(&e.to_string())).len();
                        let sha256 = cb_update::sha256_file(std::path::Path::new(p)).unwrap_or_else(|e| die(&e.to_string()));
                        let url = format!("{}/{}", base.trim_end_matches('/'), name);
                        assets.insert(k.to_string(), AssetInfo { name, url, sha256, size });
                    }
                    o => die(&format!("unknown option {o}")),
                }
                i += 1;
            }
            if version.is_empty() {
                die("--version required");
            }
            let m = Manifest { version, notes, min_supported, mandatory, published: String::new(), assets };
            println!("{}", serde_json::to_string_pretty(&m).unwrap());
        }
        _ => die("usage: keygen | manifest ... | sign FILE | verify FILE SIG KEY"),
    }
}
