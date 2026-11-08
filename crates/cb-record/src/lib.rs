//! Screen recording core.
//!
//! The app pushes rendered frames (the whole window) whenever it repaints;
//! an encoder thread turns them into a constant-frame-rate video by repeating
//! the latest frame on every tick, scales them to the output size
//! (letterboxed), mixes microphone and system audio resampled to 48 kHz
//! stereo, and hands everything to a platform encoder ([`VideoSink`]):
//! Media Foundation on Windows, AVAssetWriter on macOS.

pub mod audio;
pub mod scale;

pub use audio::{AudioFeed, AUDIO_CHANNELS, AUDIO_RATE};

use std::any::Any;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 100-nanosecond units per second (Media Foundation / CMTime timescale).
pub const HNS: i64 = 10_000_000;

/// What the platform encoder is asked to produce.
#[derive(Debug, Clone, PartialEq)]
pub struct SinkConfig {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// Include an AAC track (48 kHz stereo).
    pub audio: bool,
    pub video_bitrate: u32,
}

/// Platform video encoder. Times are in 100 ns units from the start.
pub trait VideoSink: Send {
    /// `bgra` is top-down, tightly packed (stride = width * 4).
    fn video(&mut self, bgra: &[u8], pts: i64, dur: i64) -> Result<(), String>;
    /// Interleaved stereo 16-bit PCM at 48 kHz.
    fn audio(&mut self, pcm: &[i16], pts: i64, dur: i64) -> Result<(), String>;
    fn finish(self: Box<Self>) -> Result<(), String>;
}

pub type SinkFactory = Arc<dyn Fn(&SinkConfig) -> Result<Box<dyn VideoSink>, String> + Send + Sync>;
/// Starts a capture that feeds `AudioFeed`; the returned guard stops it on drop.
pub type CaptureFn = Arc<dyn Fn(AudioFeed) -> Result<Box<dyn Any>, String>>;

/// What a platform provides for recording.
#[derive(Clone)]
pub struct Backend {
    pub sink: SinkFactory,
    pub microphone: Option<CaptureFn>,
    pub system_audio: Option<CaptureFn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordConfig {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub microphone: bool,
    pub system_audio: bool,
}

/// Output size for a window of `win` pixels with an optional target height
/// (keeps the aspect ratio; even dimensions as H.264 requires).
pub fn output_size(win: (u32, u32), target_h: Option<u32>) -> (u32, u32) {
    let (w, h) = (win.0.max(2), win.1.max(2));
    let (ow, oh) = match target_h {
        Some(th) => {
            let th = th.min(2160);
            ((w as f64 * th as f64 / h as f64).round() as u32, th)
        }
        None => {
            // Cap at 4K-ish so encoders accept it.
            let k = (3840.0 / w as f64).min(2160.0 / h as f64).min(1.0);
            ((w as f64 * k).round() as u32, (h as f64 * k).round() as u32)
        }
    };
    (even(ow.max(2)), even(oh.max(2)))
}

pub fn even(v: u32) -> u32 {
    v & !1
}

struct Frame {
    data: Vec<u8>,
    w: u32,
    h: u32,
}

/// Latest-frame mailbox with buffer reuse.
#[derive(Default)]
struct Mailbox {
    latest: Option<Frame>,
    spare: Vec<Vec<u8>>,
}

enum Msg {
    Stop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub path: PathBuf,
    pub duration: Duration,
    pub frames: u64,
}

pub struct Recorder {
    tx: Sender<Msg>,
    thread: Option<JoinHandle<Result<Finished, String>>>,
    mailbox: Arc<Mutex<Mailbox>>,
    guards: Vec<Box<dyn Any>>,
    started: Instant,
    last_push: Option<Instant>,
    frame_gap: Duration,
    /// Audio sources that could not be started (shown to the user).
    pub warnings: Vec<String>,
}

impl Recorder {
    /// Open the encoder and start capturing audio.
    pub fn start(cfg: RecordConfig, backend: &Backend) -> Result<Recorder, String> {
        let fps = cfg.fps.clamp(5, 60);
        let mut warnings = Vec::new();
        let mut feeds = Vec::new();
        let mut guards: Vec<Box<dyn Any>> = Vec::new();
        for (wanted, cap, what) in [(cfg.microphone, &backend.microphone, "麦克风"), (cfg.system_audio, &backend.system_audio, "电脑声音")] {
            if !wanted {
                continue;
            }
            match cap {
                Some(f) => {
                    let feed = AudioFeed::new();
                    match f(feed.clone()) {
                        Ok(g) => {
                            guards.push(g);
                            feeds.push(feed);
                        }
                        Err(e) => warnings.push(format!("无法录制{what}：{e}")),
                    }
                }
                None => warnings.push(format!("此系统不支持录制{what}")),
            }
        }
        let sink_cfg = SinkConfig {
            path: cfg.path.clone(),
            width: even(cfg.width.max(2)),
            height: even(cfg.height.max(2)),
            fps,
            audio: !feeds.is_empty(),
            video_bitrate: bitrate_for(cfg.width, cfg.height, fps),
        };
        let sink = (backend.sink)(&sink_cfg)?;
        let mailbox = Arc::new(Mutex::new(Mailbox::default()));
        let (tx, rx) = channel();
        let started = Instant::now();
        let mb = mailbox.clone();
        let thread = std::thread::Builder::new()
            .name("cb-record".into())
            .spawn(move || encode_loop(sink, sink_cfg, rx, mb, feeds, started))
            .map_err(|e| e.to_string())?;
        Ok(Recorder { tx, thread: Some(thread), mailbox, guards, started, last_push: None, frame_gap: Duration::from_secs_f64(1.0 / fps as f64 * 0.9), warnings })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Offer a rendered frame (premultiplied, opaque RGBA). Cheap to call on
    /// every repaint: frames arriving faster than the frame rate are skipped.
    pub fn push_frame(&mut self, rgba: &[u8], w: u32, h: u32) {
        let now = Instant::now();
        if self.last_push.is_some_and(|t| now.duration_since(t) < self.frame_gap) {
            return;
        }
        if rgba.len() < (w * h * 4) as usize {
            return;
        }
        self.last_push = Some(now);
        let mut mb = self.mailbox.lock().unwrap();
        let mut buf = mb.spare.pop().unwrap_or_default();
        buf.clear();
        buf.extend_from_slice(&rgba[..(w * h * 4) as usize]);
        if let Some(old) = mb.latest.replace(Frame { data: buf, w, h }) {
            mb.spare.push(old.data);
        }
    }

    /// Whether a frame push is due (lets the app skip work between frames).
    pub fn wants_frame(&self) -> bool {
        self.last_push.map_or(true, |t| t.elapsed() >= self.frame_gap)
    }

    /// Stop capturing, flush the encoder and close the file.
    pub fn stop(mut self) -> Result<Finished, String> {
        self.guards.clear();
        let _ = self.tx.send(Msg::Stop);
        match self.thread.take().map(|t| t.join()) {
            Some(Ok(r)) => r,
            Some(Err(_)) => Err("录制线程异常退出".into()),
            None => Err("录制未在进行".into()),
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.guards.clear();
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn bitrate_for(w: u32, h: u32, fps: u32) -> u32 {
    // ~0.1 bit per pixel per frame: 1080p30 ≈ 6 Mbit/s (board content compresses well).
    ((w as u64 * h as u64 * fps as u64) / 10).clamp(1_500_000, 20_000_000) as u32
}

fn encode_loop(mut sink: Box<dyn VideoSink>, cfg: SinkConfig, rx: Receiver<Msg>, mailbox: Arc<Mutex<Mailbox>>, feeds: Vec<AudioFeed>, started: Instant) -> Result<Finished, String> {
    let fps = cfg.fps as i64;
    let (ow, oh) = (cfg.width, cfg.height);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    // Start black so a recording that starts before the first frame is valid.
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&[0, 0, 0, 255]);
    }
    let mut n: i64 = 0;
    let mut written: u64 = 0;
    let mut audio_written: u64 = 0;
    let mut pcm: Vec<i16> = Vec::new();
    let mut stop = false;
    let mut err: Option<String> = None;
    // Audio is consumed this far behind real time so capture jitter never underruns.
    let audio_lag = 0.25f64;
    while !stop {
        let due = started + Duration::from_nanos((n as u64) * 1_000_000_000 / fps as u64);
        let now = Instant::now();
        if due > now {
            match rx.recv_timeout(due - now) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => stop = true,
                Err(RecvTimeoutError::Timeout) => {}
            }
        } else if let Ok(Msg::Stop) = rx.try_recv() {
            stop = true;
        }
        // Newest frame, if any arrived since the last tick.
        let frame = mailbox.lock().unwrap().latest.take();
        if let Some(f) = frame {
            scale::fit_rgba_to_bgra(&f.data, f.w, f.h, &mut out, ow, oh);
            mailbox.lock().unwrap().spare.push(f.data);
        }
        let pts = n * HNS / fps;
        if let Err(e) = sink.video(&out, pts, (n + 1) * HNS / fps - pts) {
            err = Some(e);
            break;
        }
        written += 1;
        // Mix the audio captured up to (now - lag), or everything when stopping.
        if cfg.audio {
            let t = started.elapsed().as_secs_f64();
            let target = if stop { ((n + 1) as f64 / fps as f64 * AUDIO_RATE as f64) as u64 } else { ((t - audio_lag).max(0.0) * AUDIO_RATE as f64) as u64 };
            if target > audio_written {
                let frames = (target - audio_written) as usize;
                audio::mix(&feeds, frames, &mut pcm);
                let pts = audio_written as i64 * HNS / AUDIO_RATE as i64;
                let d = frames as i64 * HNS / AUDIO_RATE as i64;
                if let Err(e) = sink.audio(&pcm, pts, d) {
                    err = Some(e);
                    break;
                }
                audio_written = target;
            }
        }
        n += 1;
        // Running late (slow encoder): skip ticks instead of drifting.
        let behind = started.elapsed().as_nanos() as i64 * fps / 1_000_000_000;
        if behind > n + 2 {
            n = behind;
        }
    }
    let fin = sink.finish();
    if let Some(e) = err {
        return Err(e);
    }
    fin?;
    Ok(Finished { path: cfg.path, duration: Duration::from_nanos((n as u64) * 1_000_000_000 / fps as u64), frames: written })
}

/// Microphone capture through the default input device (cpal).
#[cfg(all(feature = "capture", any(windows, target_os = "macos")))]
pub fn cpal_microphone() -> CaptureFn {
    Arc::new(|feed| audio::capture::input(feed, false))
}

/// System audio through WASAPI loopback on the default output device.
#[cfg(all(feature = "capture", windows))]
pub fn cpal_loopback() -> CaptureFn {
    Arc::new(|feed| audio::capture::input(feed, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Log {
        video: Vec<(i64, i64, u8)>,
        audio_frames: u64,
        audio_pts: Vec<i64>,
        finished: bool,
    }
    struct Mock(Arc<Mutex<Log>>, u32, u32);
    impl VideoSink for Mock {
        fn video(&mut self, bgra: &[u8], pts: i64, dur: i64) -> Result<(), String> {
            // Record the red channel of the centre pixel.
            let i = (((self.2 / 2) * self.1 + self.1 / 2) * 4) as usize;
            self.0.lock().unwrap().video.push((pts, dur, bgra[i + 2]));
            Ok(())
        }
        fn audio(&mut self, pcm: &[i16], pts: i64, _dur: i64) -> Result<(), String> {
            let mut l = self.0.lock().unwrap();
            l.audio_frames += (pcm.len() / 2) as u64;
            l.audio_pts.push(pts);
            Ok(())
        }
        fn finish(self: Box<Self>) -> Result<(), String> {
            self.0.lock().unwrap().finished = true;
            Ok(())
        }
    }

    #[test]
    fn output_sizes_are_even_and_keep_aspect() {
        assert_eq!(output_size((2560, 1440), Some(1080)), (1920, 1080));
        assert_eq!(output_size((1366, 768), Some(720)), (1280, 720));
        assert_eq!(output_size((1367, 769), None), (1366, 768));
        assert_eq!(output_size((7680, 4320), None), (3840, 2160));
    }

    #[test]
    fn constant_frame_rate_with_audio() {
        let log = Arc::new(Mutex::new(Log::default()));
        let l2 = log.clone();
        let feed_slot: Arc<Mutex<Option<AudioFeed>>> = Arc::new(Mutex::new(None));
        let fs = feed_slot.clone();
        let backend = Backend {
            sink: Arc::new(move |c: &SinkConfig| {
                assert!(c.audio);
                Ok(Box::new(Mock(l2.clone(), c.width, c.height)) as Box<dyn VideoSink>)
            }),
            microphone: Some(Arc::new(move |f: AudioFeed| {
                *fs.lock().unwrap() = Some(f);
                Ok(Box::new(()) as Box<dyn Any>)
            })),
            system_audio: None,
        };
        let cfg = RecordConfig { path: "x.mp4".into(), width: 64, height: 36, fps: 30, microphone: true, system_audio: true };
        let mut rec = Recorder::start(cfg, &backend).unwrap();
        assert_eq!(rec.warnings.len(), 1, "system audio unsupported warning");
        let feed = feed_slot.lock().unwrap().clone().unwrap();
        // Only two frames are pushed (red then green); the encoder repeats them.
        let red: Vec<u8> = [255u8, 0, 0, 255].repeat(32 * 18);
        rec.push_frame(&red, 32, 18);
        let t0 = Instant::now();
        let mut pushed_green = false;
        while t0.elapsed() < Duration::from_millis(1000) {
            // 44.1 kHz mono microphone in 10 ms chunks.
            feed.push(&vec![0.25f32; 441], 44_100, 1);
            if t0.elapsed() > Duration::from_millis(500) && !pushed_green {
                let green: Vec<u8> = [0u8, 255, 0, 255].repeat(32 * 18);
                rec.last_push = None;
                rec.push_frame(&green, 32, 18);
                pushed_green = true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let fin = rec.stop().unwrap();
        let l = log.lock().unwrap();
        assert!(l.finished);
        let n = l.video.len() as i64;
        // A loaded machine may skip ticks to stay in sync, never add them.
        assert!((20..=33).contains(&n), "frames {n}");
        assert_eq!(fin.frames as i64, n);
        for w in l.video.windows(2) {
            assert!(w[1].0 > w[0].0, "timestamps increase");
        }
        for (pts, dur, _) in &l.video {
            assert!((*dur - HNS / 30).abs() <= 1);
            assert!(*pts <= fin.duration.as_nanos() as i64 / 100);
        }
        assert_eq!(l.video[1].2, 255, "first frames red");
        assert_eq!(l.video.last().unwrap().2, 0, "last frames green");
        // Audio covers the video duration at 48 kHz.
        let want = fin.duration.as_millis() as u64 * 48;
        assert!(l.audio_frames.abs_diff(want) < 48_000 / 30 + 2, "audio {} vs {want}", l.audio_frames);
        assert!(l.audio_pts.windows(2).all(|w| w[1] > w[0]));
    }
}
