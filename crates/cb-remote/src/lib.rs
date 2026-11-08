//! LAN companion server: phones/tablets open a web page, draw on it, and
//! strokes appear on the board in real time.
//!
//! * `GET /`            – embedded web client (`web/remote/index.html`)
//! * `GET /ws?t=TOKEN`  – WebSocket; JSON text frames from the client,
//!                        tagged JPEG binary frames to the client (whole
//!                        viewport preview, or a sharp detail of the zoomed part).
//! * `POST /upload?t=TOKEN&batch=ID&i=0&n=3` – a photo from the phone.
//!
//! Only the newest connected client may write; older ones are disconnected.

pub mod proto;

pub use proto::{ClientMsg, RemoteMsg, ServerState, FRAME_DETAIL, FRAME_PREVIEW, MAX_UPLOAD};
use rand::Rng;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

pub static INDEX_HTML: &str = include_str!("../../../web/remote/index.html");

pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub port: u16,
    pub token: String,
    /// Reachable addresses, best first.
    pub ips: Vec<Ipv4Addr>,
}

impl ServerInfo {
    pub fn url_for(&self, ip: Ipv4Addr) -> String {
        format!("http://{}:{}/?t={}", ip, self.port, self.token)
    }
    pub fn primary_url(&self) -> Option<String> {
        self.ips.first().map(|ip| self.url_for(*ip))
    }
}

enum Out {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

struct Shared {
    token: String,
    events: Sender<RemoteMsg>,
    waker: Waker,
    /// Generation of the current writer; older connections exit.
    current: AtomicU64,
    outbox: Mutex<Option<(u64, Sender<Out>)>>,
    last_state: Mutex<Option<String>>,
    last_preview: Mutex<Option<Vec<u8>>>,
    stop: AtomicBool,
}

pub struct RemoteServer {
    info: ServerInfo,
    shared: Arc<Shared>,
    events: Receiver<RemoteMsg>,
    port: u16,
}

/// Local IPv4 addresses ordered by how likely a phone can reach them
/// (Windows hotspot 192.168.137.x first, then private LAN ranges).
pub fn local_ipv4() -> Vec<Ipv4Addr> {
    let mut v: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(a) if !a.is_loopback() && !a.is_link_local() && !a.is_unspecified() => Some(a),
            _ => None,
        })
        .collect();
    let rank = |a: &Ipv4Addr| {
        let o = a.octets();
        match o {
            [192, 168, 137, _] => 0,
            [192, 168, ..] => 1,
            [10, ..] => 2,
            [172, b, ..] if (16..32).contains(&b) => 3,
            _ => 4,
        }
    };
    v.sort_by_key(rank);
    v.dedup();
    v
}

fn random_token() -> String {
    const CH: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut r = rand::thread_rng();
    (0..8).map(|_| CH[r.gen_range(0..CH.len())] as char).collect()
}

impl RemoteServer {
    /// Bind on all interfaces. `port` 0 tries 8765..8775 then any free port.
    pub fn start(port: u16, waker: Waker) -> std::io::Result<RemoteServer> {
        let listener = if port != 0 {
            TcpListener::bind((Ipv4Addr::UNSPECIFIED, port))?
        } else {
            (8765..8776).find_map(|p| TcpListener::bind((Ipv4Addr::UNSPECIFIED, p)).ok()).map(Ok).unwrap_or_else(|| TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)))?
        };
        let port = listener.local_addr()?.port();
        let (tx, rx) = channel();
        let shared = Arc::new(Shared {
            token: random_token(),
            events: tx,
            waker,
            current: AtomicU64::new(0),
            outbox: Mutex::new(None),
            last_state: Mutex::new(None),
            last_preview: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
        let info = ServerInfo { port, token: shared.token.clone(), ips: local_ipv4() };
        let sh = shared.clone();
        std::thread::Builder::new().name("cb-remote".into()).spawn(move || {
            for conn in listener.incoming() {
                if sh.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = conn else { continue };
                let sh = sh.clone();
                let _ = std::thread::Builder::new().name("cb-remote-conn".into()).spawn(move || handle(stream, sh));
            }
        })?;
        Ok(RemoteServer { info, shared, events: rx, port })
    }

    pub fn info(&self) -> &ServerInfo {
        &self.info
    }

    pub fn refresh_ips(&mut self) {
        self.info.ips = local_ipv4();
    }

    pub fn poll(&self) -> Vec<RemoteMsg> {
        self.events.try_iter().collect()
    }

    pub fn connected(&self) -> bool {
        self.shared.outbox.lock().unwrap().is_some()
    }

    pub fn send_state(&self, st: &ServerState) {
        let s = serde_json::to_string(st).unwrap_or_default();
        *self.shared.last_state.lock().unwrap() = Some(s.clone());
        if let Some((_, tx)) = self.shared.outbox.lock().unwrap().as_ref() {
            let _ = tx.send(Out::Text(s));
        }
    }

    /// Send an encoded preview image (JPEG/PNG) of the whole viewport.
    pub fn send_preview(&self, img: Vec<u8>) {
        let mut f = Vec::with_capacity(img.len() + 1);
        f.push(FRAME_PREVIEW);
        f.extend_from_slice(&img);
        *self.shared.last_preview.lock().unwrap() = Some(f.clone());
        if let Some((_, tx)) = self.shared.outbox.lock().unwrap().as_ref() {
            let _ = tx.send(Out::Binary(f));
        }
    }

    /// Send a sharp image of the normalised region `rect` (x, y, w, h) the
    /// phone is zoomed into.
    pub fn send_detail(&self, rect: [f32; 4], img: Vec<u8>) {
        let mut f = Vec::with_capacity(img.len() + 17);
        f.push(FRAME_DETAIL);
        for v in rect {
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&img);
        if let Some((_, tx)) = self.shared.outbox.lock().unwrap().as_ref() {
            let _ = tx.send(Out::Binary(f));
        }
    }

    pub fn stop(self) {
        drop(self)
    }
}

impl Drop for RemoteServer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some((_, tx)) = self.shared.outbox.lock().unwrap().take() {
            let _ = tx.send(Out::Close);
        }
        self.shared.current.fetch_add(1, Ordering::SeqCst);
        // Unblock accept().
        let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)), Duration::from_millis(200));
    }
}

fn respond(s: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let _ = write!(s, "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
    let _ = s.write_all(body);
}

fn query_param<'a>(path: &'a str, key: &str) -> Option<&'a str> {
    let q = path.split_once('?')?.1;
    q.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == key).map(|(_, v)| v))
}

fn handle(mut stream: TcpStream, sh: Arc<Shared>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/").to_string());
    let mut ws_key = None;
    let mut upgrade = false;
    let mut content_length: Option<usize> = None;
    let mut total = 0;
    loop {
        let mut h = String::new();
        match reader.read_line(&mut h) {
            Ok(0) | Err(_) => return,
            Ok(n) => total += n,
        }
        if total > 16 * 1024 {
            return;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            if k == "sec-websocket-key" {
                ws_key = Some(v.trim().to_string());
            } else if k == "upgrade" && v.trim().eq_ignore_ascii_case("websocket") {
                upgrade = true;
            } else if k == "content-length" {
                content_length = v.trim().parse().ok();
            }
        }
    }
    let route = path.split('?').next().unwrap_or("/");
    if method == "POST" && route == "/upload" {
        if query_param(&path, "t") != Some(sh.token.as_str()) {
            respond(&mut stream, "403 Forbidden", "application/json", br#"{"ok":false,"error":"token"}"#);
            return;
        }
        let Some(len) = content_length else {
            respond(&mut stream, "411 Length Required", "application/json", br#"{"ok":false}"#);
            return;
        };
        if len == 0 || len > MAX_UPLOAD {
            respond(&mut stream, "413 Payload Too Large", "application/json", br#"{"ok":false,"error":"size"}"#);
            return;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let num = |k: &str| query_param(&path, k).and_then(|v| v.parse::<u32>().ok());
        let batch = query_param(&path, "batch").unwrap_or("").chars().filter(|c| c.is_ascii_alphanumeric()).take(32).collect::<String>();
        let total = num("n").unwrap_or(1).clamp(1, 50);
        let index = num("i").unwrap_or(0).min(total - 1);
        let _ = sh.events.send(RemoteMsg::Image { batch, index, total, data: body });
        (sh.waker)();
        respond(&mut stream, "200 OK", "application/json", br#"{"ok":true}"#);
        return;
    }
    if method != "GET" {
        respond(&mut stream, "405 Method Not Allowed", "text/plain", b"");
        return;
    }
    match route {
        "/" | "/index.html" => respond(&mut stream, "200 OK", "text/html; charset=utf-8", INDEX_HTML.as_bytes()),
        "/ws" if upgrade => {
            if query_param(&path, "t") != Some(sh.token.as_str()) {
                respond(&mut stream, "403 Forbidden", "text/plain; charset=utf-8", "连接码无效，请重新扫码".as_bytes());
                return;
            }
            let Some(key) = ws_key else { return };
            let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
            let _ = write!(stream, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n");
            drop(reader);
            run_ws(stream, sh);
        }
        "/health" => respond(&mut stream, "200 OK", "text/plain", b"ok"),
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn run_ws(stream: TcpStream, sh: Arc<Shared>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(15)));
    let _ = stream.set_nodelay(true);
    let peer = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let mut ws = WebSocket::from_raw_socket(stream, Role::Server, None);
    let gen = sh.current.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = channel::<Out>();
    {
        let mut ob = sh.outbox.lock().unwrap();
        if let Some((_, old)) = ob.take() {
            let _ = old.send(Out::Close);
        }
        *ob = Some((gen, tx));
    }
    let emit = |m: RemoteMsg| {
        let _ = sh.events.send(m);
        (sh.waker)();
    };
    emit(RemoteMsg::Connected { peer: peer.clone() });
    if let Some(s) = sh.last_state.lock().unwrap().clone() {
        let _ = ws.send(Message::Text(s));
    }
    if let Some(p) = sh.last_preview.lock().unwrap().clone() {
        let _ = ws.send(Message::Binary(p));
    }
    'outer: loop {
        if sh.current.load(Ordering::SeqCst) != gen || sh.stop.load(Ordering::SeqCst) {
            let _ = ws.close(None);
            let _ = ws.flush();
            break;
        }
        match ws.read() {
            Ok(Message::Text(t)) => match serde_json::from_str::<ClientMsg>(&t) {
                Ok(m) => emit(RemoteMsg::Client(m)),
                Err(e) => log::debug!("bad remote message: {e}"),
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break,
        }
        // Only the newest frame of each kind matters; drain and coalesce.
        let mut latest_bin: Option<Vec<u8>> = None;
        let mut latest_detail: Option<Vec<u8>> = None;
        while let Ok(o) = rx.try_recv() {
            match o {
                Out::Text(t) => {
                    if ws.send(Message::Text(t)).is_err() {
                        break 'outer;
                    }
                }
                Out::Binary(b) if b.first() == Some(&FRAME_DETAIL) => latest_detail = Some(b),
                Out::Binary(b) => latest_bin = Some(b),
                Out::Close => {
                    let _ = ws.close(None);
                    let _ = ws.flush();
                    break 'outer;
                }
            }
        }
        for b in [latest_bin, latest_detail].into_iter().flatten() {
            if ws.send(Message::Binary(b)).is_err() {
                break 'outer;
            }
        }
    }
    let mut ob = sh.outbox.lock().unwrap();
    if ob.as_ref().map(|(g, _)| *g) == Some(gen) {
        *ob = None;
        drop(ob);
        emit(RemoteMsg::Disconnected);
    }
}

/// QR code modules for `text`: (side length, row-major dark flags).
pub fn qr_matrix(text: &str) -> Option<(usize, Vec<bool>)> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M).ok()?;
    let n = code.width();
    let v = code.to_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
    Some((n, v))
}
