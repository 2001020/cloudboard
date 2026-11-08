//! Audio feeds: capture callbacks push samples at their native rate and
//! channel count; they are resampled to 48 kHz stereo on arrival and mixed
//! by the encoder thread.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;

struct State {
    /// Interleaved stereo samples at 48 kHz waiting to be mixed.
    queue: VecDeque<f32>,
    prev: [f32; 2],
    /// Position between `prev` and the next input frame, in input frames.
    t: f64,
    primed: bool,
}

/// A capture source's sample queue. Cheap to clone; `push` may be called
/// from any thread (audio callbacks).
#[derive(Clone)]
pub struct AudioFeed {
    inner: Arc<Mutex<State>>,
}

impl Default for AudioFeed {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioFeed {
    pub fn new() -> AudioFeed {
        AudioFeed { inner: Arc::new(Mutex::new(State { queue: VecDeque::new(), prev: [0.0; 2], t: 0.0, primed: false })) }
    }

    /// Append interleaved samples (`channels` per frame) recorded at `rate`.
    pub fn push(&self, data: &[f32], rate: u32, channels: u16) {
        if rate == 0 || channels == 0 {
            return;
        }
        let ch = channels as usize;
        let ratio = rate as f64 / AUDIO_RATE as f64;
        let mut st = self.inner.lock().unwrap();
        for f in data.chunks_exact(ch) {
            let cur = if ch == 1 { [f[0], f[0]] } else { [f[0], f[1]] };
            if !st.primed {
                st.prev = cur;
                st.primed = true;
                continue;
            }
            while st.t < 1.0 {
                let t = st.t as f32;
                let (a, b) = (st.prev[0] + (cur[0] - st.prev[0]) * t, st.prev[1] + (cur[1] - st.prev[1]) * t);
                st.queue.push_back(a);
                st.queue.push_back(b);
                st.t += ratio;
            }
            st.t -= 1.0;
            st.prev = cur;
        }
        // Never buffer more than 2 s (a stalled consumer or clock drift).
        let max = AUDIO_RATE as usize * 2 * 2;
        if st.queue.len() > max {
            let drop = st.queue.len() - AUDIO_RATE as usize * 2;
            st.queue.drain(..drop);
        }
    }

    /// Samples (frames) currently buffered.
    pub fn buffered(&self) -> usize {
        self.inner.lock().unwrap().queue.len() / 2
    }
}

/// Mix `frames` stereo frames from all feeds into 16-bit PCM. Feeds that
/// run short contribute silence for the missing part.
pub fn mix(feeds: &[AudioFeed], frames: usize, out: &mut Vec<i16>) {
    let mut acc = vec![0f32; frames * 2];
    for f in feeds {
        let mut st = f.inner.lock().unwrap();
        let n = (frames * 2).min(st.queue.len());
        for (a, s) in acc.iter_mut().zip(st.queue.drain(..n)) {
            *a += s;
        }
    }
    out.clear();
    out.extend(acc.into_iter().map(|v| (v.clamp(-1.0, 1.0) * 32767.0) as i16));
}

#[cfg(all(feature = "capture", any(windows, target_os = "macos")))]
pub mod capture {
    use super::AudioFeed;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::any::Any;

    /// Open the default input device (or, with `loopback`, the default
    /// output device in WASAPI loopback mode) and feed its samples.
    pub fn input(feed: AudioFeed, loopback: bool) -> Result<Box<dyn Any>, String> {
        let host = cpal::default_host();
        let device = if loopback { host.default_output_device() } else { host.default_input_device() }.ok_or_else(|| if loopback { "没有找到播放设备".to_string() } else { "没有找到麦克风".to_string() })?;
        let cfg = if loopback { device.default_output_config() } else { device.default_input_config() }.map_err(|e| e.to_string())?;
        let rate = cfg.sample_rate().0;
        let ch = cfg.channels();
        let sc: cpal::StreamConfig = cfg.clone().into();
        let err = |e: cpal::StreamError| log::warn!("audio capture: {e}");
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => {
                let f = feed.clone();
                device.build_input_stream(&sc, move |d: &[f32], _: &_| f.push(d, rate, ch), err, None)
            }
            cpal::SampleFormat::I16 => {
                let f = feed.clone();
                device.build_input_stream(&sc, move |d: &[i16], _: &_| f.push(&d.iter().map(|v| *v as f32 / 32768.0).collect::<Vec<_>>(), rate, ch), err, None)
            }
            cpal::SampleFormat::U16 => {
                let f = feed.clone();
                device.build_input_stream(&sc, move |d: &[u16], _: &_| f.push(&d.iter().map(|v| (*v as f32 - 32768.0) / 32768.0).collect::<Vec<_>>(), rate, ch), err, None)
            }
            cpal::SampleFormat::I32 => {
                let f = feed.clone();
                device.build_input_stream(&sc, move |d: &[i32], _: &_| f.push(&d.iter().map(|v| *v as f32 / 2147483648.0).collect::<Vec<_>>(), rate, ch), err, None)
            }
            other => return Err(format!("不支持的音频格式 {other:?}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Box::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resamples_to_48k_stereo() {
        let f = AudioFeed::new();
        // 1 s of 44.1 kHz mono.
        for _ in 0..100 {
            f.push(&vec![0.5; 441], 44_100, 1);
        }
        let n = f.buffered();
        assert!((47_900..=48_000).contains(&n), "{n}");
        let mut out = Vec::new();
        mix(&[f.clone()], 1000, &mut out);
        assert_eq!(out.len(), 2000);
        assert!(out.iter().all(|v| (*v - 16383).abs() <= 1));
    }

    #[test]
    fn mixing_sums_and_pads() {
        let a = AudioFeed::new();
        let b = AudioFeed::new();
        a.push(&vec![0.25; 4802], 48_000, 2);
        b.push(&vec![0.5; 1000], 48_000, 2);
        let mut out = Vec::new();
        mix(&[a, b], 2000, &mut out);
        assert_eq!(out.len(), 4000);
        // First part has both feeds, the rest only `a` (b ran short).
        assert!((out[0] as i32 - 24575).abs() <= 2);
        assert!((out[3999] as i32 - 8191).abs() <= 2);
    }

    #[test]
    fn drops_backlog_beyond_two_seconds() {
        let f = AudioFeed::new();
        for _ in 0..300 {
            f.push(&vec![0.1; 960], 48_000, 2);
        }
        assert!(f.buffered() <= 96_000);
    }
}
