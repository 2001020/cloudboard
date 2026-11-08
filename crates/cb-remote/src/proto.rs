//! Wire protocol (JSON text frames). Coordinates are normalised to the
//! board viewport shown on the PC: (0,0) top-left, (1,1) bottom-right.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        #[serde(default)]
        name: String,
    },
    /// Start a stroke. `w` is the width relative to the viewport width.
    Begin {
        id: u32,
        x: f32,
        y: f32,
        #[serde(default = "one")]
        p: f32,
        color: String,
        w: f32,
        #[serde(default)]
        hl: bool,
        #[serde(default)]
        eraser: bool,
    },
    /// Further points: [[x, y, pressure], ...]
    Move { id: u32, pts: Vec<[f32; 3]> },
    End { id: u32 },
    Undo,
    Redo,
    /// Page change by delta (-1 / +1).
    Page { d: i32 },
    /// The part of the board the phone is zoomed into (normalised; the
    /// whole viewport is 0,0,1,1). The PC answers with sharp detail frames.
    View { x: f32, y: f32, w: f32, h: f32 },
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq)]
pub enum RemoteMsg {
    Connected { peer: String },
    Disconnected,
    Client(ClientMsg),
    /// A photo uploaded from the phone. Photos picked together share `batch`;
    /// `index` / `total` tell when the batch is complete.
    Image { batch: String, index: u32, total: u32, data: Vec<u8> },
}

/// Binary frame kinds (first byte of every binary frame).
pub const FRAME_PREVIEW: u8 = 0;
/// Followed by 4 little-endian f32 (x, y, w, h, normalised) then the image.
pub const FRAME_DETAIL: u8 = 1;

/// Largest accepted upload (bytes).
pub const MAX_UPLOAD: usize = 20 * 1024 * 1024;

/// Board state pushed to the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerState {
    pub t: String,
    pub page: usize,
    pub pages: usize,
    /// Viewport height / width.
    pub aspect: f32,
    pub pdf: bool,
}

impl ServerState {
    pub fn new(page: usize, pages: usize, aspect: f32, pdf: bool) -> ServerState {
        ServerState { t: "state".into(), page, pages, aspect, pdf }
    }
}

/// Parse "#rrggbb" into ARGB.
pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(|v| 0xFF00_0000 | v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_messages() {
        let m: ClientMsg = serde_json::from_str(r##"{"t":"begin","id":1,"x":0.5,"y":0.25,"color":"#ff0000","w":0.004}"##).unwrap();
        assert!(matches!(m, ClientMsg::Begin { id: 1, p, .. } if p == 1.0));
        let m: ClientMsg = serde_json::from_str(r#"{"t":"view","x":0.25,"y":0.25,"w":0.5,"h":0.5}"#).unwrap();
        assert!(matches!(m, ClientMsg::View { w, .. } if w == 0.5));
        let m: ClientMsg = serde_json::from_str(r#"{"t":"move","id":1,"pts":[[0.1,0.2,0.5]]}"#).unwrap();
        assert_eq!(m, ClientMsg::Move { id: 1, pts: vec![[0.1, 0.2, 0.5]] });
        assert_eq!(serde_json::from_str::<ClientMsg>(r#"{"t":"page","d":-1}"#).unwrap(), ClientMsg::Page { d: -1 });
        assert_eq!(parse_color("#1e1e1e"), Some(0xFF1E1E1E));
    }
}
