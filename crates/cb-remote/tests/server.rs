use cb_remote::{qr_matrix, ClientMsg, RemoteMsg, RemoteServer, ServerState};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tungstenite::Message;

fn wait_for(srv: &RemoteServer, n: usize) -> Vec<RemoteMsg> {
    let t0 = Instant::now();
    let mut v = Vec::new();
    while v.len() < n && t0.elapsed() < Duration::from_secs(5) {
        v.extend(srv.poll());
        std::thread::sleep(Duration::from_millis(5));
    }
    v
}

#[test]
fn serves_page_and_relays_strokes() {
    let srv = RemoteServer::start(0, Arc::new(|| {})).unwrap();
    let port = srv.info().port;
    // HTML page
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut body = String::new();
    s.read_to_string(&mut body).unwrap();
    assert!(body.starts_with("HTTP/1.1 200"));
    assert!(body.contains("Cloudboard"));

    // Wrong token rejected
    let bad = tungstenite::connect(format!("ws://127.0.0.1:{port}/ws?t=nope"));
    assert!(bad.is_err());

    let url = format!("ws://127.0.0.1:{port}/ws?t={}", srv.info().token);
    srv.send_state(&ServerState::new(0, 3, 0.5625, false));
    srv.send_preview(vec![1, 2, 3]);
    let (mut ws, _) = tungstenite::connect(&url).unwrap();
    // Receives cached state + preview on connect.
    let first = ws.read().unwrap();
    assert!(matches!(first, Message::Text(ref t) if t.contains("\"pages\":3")));
    assert!(matches!(ws.read().unwrap(), Message::Binary(ref b) if b == &[cb_remote::FRAME_PREVIEW, 1, 2, 3]));
    // Detail frames carry their region.
    srv.send_detail([0.25, 0.5, 0.5, 0.25], vec![9, 9]);
    match ws.read().unwrap() {
        Message::Binary(b) => {
            assert_eq!(b[0], cb_remote::FRAME_DETAIL);
            assert_eq!(f32::from_le_bytes(b[1..5].try_into().unwrap()), 0.25);
            assert_eq!(f32::from_le_bytes(b[13..17].try_into().unwrap()), 0.25);
            assert_eq!(&b[17..], &[9, 9]);
        }
        m => panic!("unexpected {m:?}"),
    }
    ws.send(Message::Text(r##"{"t":"begin","id":1,"x":0.1,"y":0.2,"color":"#ff0000","w":0.003}"##.into())).unwrap();
    ws.send(Message::Text(r#"{"t":"move","id":1,"pts":[[0.2,0.3,1],[0.3,0.4,0.5]]}"#.into())).unwrap();
    ws.send(Message::Text(r#"{"t":"end","id":1}"#.into())).unwrap();
    let ev = wait_for(&srv, 4);
    assert!(matches!(ev[0], RemoteMsg::Connected { .. }));
    assert!(matches!(ev[1], RemoteMsg::Client(ClientMsg::Begin { id: 1, .. })));
    assert!(matches!(ev[2], RemoteMsg::Client(ClientMsg::Move { ref pts, .. }) if pts.len() == 2));
    assert_eq!(ev[3], RemoteMsg::Client(ClientMsg::End { id: 1 }));
    assert!(srv.connected());

    // A second client takes over; the first is closed.
    let (mut ws2, _) = tungstenite::connect(&url).unwrap();
    let _ = ws2.read();
    let t0 = Instant::now();
    let mut closed = false;
    while t0.elapsed() < Duration::from_secs(3) {
        match ws.read() {
            Ok(Message::Close(_)) | Err(_) => {
                closed = true;
                break;
            }
            _ => {}
        }
    }
    assert!(closed, "old client should be disconnected");
    drop(srv);
}

#[test]
fn qr_code() {
    let (n, m) = qr_matrix("http://192.168.137.1:8765/?t=abcdefgh").unwrap();
    assert!(n >= 21);
    assert_eq!(m.len(), n * n);
}

fn post(port: u16, path: &str, body: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n", body.len()).unwrap();
    s.write_all(body).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn photo_upload() {
    let srv = RemoteServer::start(0, Arc::new(|| {})).unwrap();
    let port = srv.info().port;
    let tok = srv.info().token.clone();
    // Wrong token.
    assert!(post(port, "/upload?t=bad&batch=a&i=0&n=1", b"xx").starts_with("HTTP/1.1 403"));
    // A batch of two photos.
    let big = vec![7u8; 300_000];
    assert!(post(port, &format!("/upload?t={tok}&batch=b1&i=0&n=2"), &big).starts_with("HTTP/1.1 200"));
    assert!(post(port, &format!("/upload?t={tok}&batch=b1&i=1&n=2"), b"second").starts_with("HTTP/1.1 200"));
    let ev = wait_for(&srv, 2);
    match (&ev[0], &ev[1]) {
        (RemoteMsg::Image { batch, index: 0, total: 2, data }, RemoteMsg::Image { index: 1, data: d2, .. }) => {
            assert_eq!(batch, "b1");
            assert_eq!(data.len(), 300_000);
            assert_eq!(d2.as_slice(), b"second");
        }
        e => panic!("unexpected {e:?}"),
    }
    // Too large is refused before reading the body.
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "POST /upload?t={tok} HTTP/1.1\r\nContent-Length: {}\r\n\r\n", cb_remote::MAX_UPLOAD + 1).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 413"));
}
