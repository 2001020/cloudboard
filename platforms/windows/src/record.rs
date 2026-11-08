//! MP4 (H.264 + AAC) encoding through Media Foundation's sink writer.
//! Built into Windows 10/11; "N" editions need the Media Feature Pack.

use cb_record::{SinkConfig, VideoSink, AUDIO_CHANNELS, AUDIO_RATE};
use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

pub struct MfSink {
    writer: IMFSinkWriter,
    video: u32,
    audio: Option<u32>,
    width: u32,
    height: u32,
    started_com: bool,
}

// The sink writer is free-threaded (MTA); it is created and used on the
// encoder thread only.
unsafe impl Send for MfSink {}

fn hr<T>(r: windows::core::Result<T>, what: &str) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {} (0x{:08X})", e.message(), e.code().0))
}

fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

impl MfSink {
    pub fn open(cfg: &SinkConfig) -> Result<MfSink, String> {
        unsafe {
            let started_com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
                return Err(format!("无法启动 Windows 媒体组件（Windows N 版需安装“媒体功能包”）：{}", e.message()));
            }
            let mut attrs: Option<IMFAttributes> = None;
            hr(MFCreateAttributes(&mut attrs, 3), "MFCreateAttributes")?;
            let attrs = attrs.ok_or("MFCreateAttributes")?;
            let _ = attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1);
            let _ = attrs.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4);
            let _ = attrs.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1);
            let url = HSTRING::from(cfg.path.as_os_str());
            let writer = hr(MFCreateSinkWriterFromURL(&url, None, &attrs), "无法创建视频文件")?;

            // Video: H.264 out, RGB32 in (the writer inserts the colour converter).
            let out = hr(MFCreateMediaType(), "MFCreateMediaType")?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok();
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264).ok();
            out.SetUINT32(&MF_MT_AVG_BITRATE, cfg.video_bitrate).ok();
            out.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).ok();
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack(cfg.width, cfg.height)).ok();
            out.SetUINT64(&MF_MT_FRAME_RATE, pack(cfg.fps, 1)).ok();
            out.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).ok();
            let video = hr(writer.AddStream(&out), "视频编码器不可用")?;
            let inp = hr(MFCreateMediaType(), "MFCreateMediaType")?;
            inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok();
            inp.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok();
            inp.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).ok();
            inp.SetUINT64(&MF_MT_FRAME_SIZE, pack(cfg.width, cfg.height)).ok();
            inp.SetUINT64(&MF_MT_FRAME_RATE, pack(cfg.fps, 1)).ok();
            inp.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)).ok();
            hr(writer.SetInputMediaType(video, &inp, None), "视频格式不受支持")?;

            // Audio: AAC 192 kbit/s out, 16-bit PCM in.
            let audio = if cfg.audio {
                let ch = AUDIO_CHANNELS as u32;
                let out = hr(MFCreateMediaType(), "MFCreateMediaType")?;
                out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).ok();
                out.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).ok();
                out.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).ok();
                out.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).ok();
                out.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch).ok();
                out.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 24_000).ok();
                let idx = hr(writer.AddStream(&out), "音频编码器不可用")?;
                let inp = hr(MFCreateMediaType(), "MFCreateMediaType")?;
                inp.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).ok();
                inp.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).ok();
                inp.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).ok();
                inp.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).ok();
                inp.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, ch).ok();
                inp.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, ch * 2).ok();
                inp.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_RATE * ch * 2).ok();
                hr(writer.SetInputMediaType(idx, &inp, None), "音频格式不受支持")?;
                Some(idx)
            } else {
                None
            };
            hr(writer.BeginWriting(), "无法开始写入视频")?;
            Ok(MfSink { writer, video, audio, width: cfg.width, height: cfg.height, started_com })
        }
    }

    fn write(&mut self, stream: u32, len: usize, pts: i64, dur: i64, fill: impl FnOnce(&mut [u8])) -> Result<(), String> {
        unsafe {
            let buf = hr(MFCreateMemoryBuffer(len as u32), "MFCreateMemoryBuffer")?;
            let mut p: *mut u8 = std::ptr::null_mut();
            hr(buf.Lock(&mut p, None, None), "Lock")?;
            fill(std::slice::from_raw_parts_mut(p, len));
            let _ = buf.Unlock();
            hr(buf.SetCurrentLength(len as u32), "SetCurrentLength")?;
            let sample = hr(MFCreateSample(), "MFCreateSample")?;
            hr(sample.AddBuffer(&buf), "AddBuffer")?;
            hr(sample.SetSampleTime(pts), "SetSampleTime")?;
            hr(sample.SetSampleDuration(dur), "SetSampleDuration")?;
            hr(self.writer.WriteSample(stream, &sample), "写入视频失败")
        }
    }
}

impl VideoSink for MfSink {
    fn video(&mut self, bgra: &[u8], pts: i64, dur: i64) -> Result<(), String> {
        let stride = self.width as usize * 4;
        let h = self.height as usize;
        // RGB32 media samples are bottom-up.
        self.write(self.video, stride * h, pts, dur, |dst| {
            for y in 0..h {
                dst[y * stride..(y + 1) * stride].copy_from_slice(&bgra[(h - 1 - y) * stride..(h - y) * stride]);
            }
        })
    }

    fn audio(&mut self, pcm: &[i16], pts: i64, dur: i64) -> Result<(), String> {
        let Some(idx) = self.audio else { return Ok(()) };
        if pcm.is_empty() {
            return Ok(());
        }
        self.write(idx, pcm.len() * 2, pts, dur, |dst| {
            for (d, s) in dst.chunks_exact_mut(2).zip(pcm) {
                d.copy_from_slice(&s.to_le_bytes());
            }
        })
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        let r = unsafe { hr(self.writer.Finalize(), "无法完成视频文件") };
        let com = self.started_com;
        drop(self);
        unsafe {
            let _ = MFShutdown();
            if com {
                CoUninitialize();
            }
        }
        r
    }
}

/// The recording backend for Windows.
pub fn backend() -> cb_record::Backend {
    cb_record::Backend {
        sink: std::sync::Arc::new(|c: &SinkConfig| MfSink::open(c).map(|s| Box::new(s) as Box<dyn VideoSink>)),
        microphone: Some(cb_record::cpal_microphone()),
        system_audio: Some(cb_record::cpal_loopback()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Encodes two seconds of synthetic video + a tone and checks the file.
    /// Skipped where Media Foundation is unavailable (e.g. Server images
    /// without the Media Foundation feature).
    #[test]
    fn encodes_mp4() {
        let path = std::env::temp_dir().join(format!("cb-record-test-{}.mp4", std::process::id()));
        let cfg = SinkConfig { path: path.clone(), width: 640, height: 360, fps: 30, audio: true, video_bitrate: 2_000_000 };
        let mut sink = match MfSink::open(&cfg) {
            Ok(s) => Box::new(s),
            Err(e) => {
                eprintln!("SKIP: Media Foundation unavailable: {e}");
                return;
            }
        };
        let t0 = Instant::now();
        let mut frame = vec![0u8; 640 * 360 * 4];
        for n in 0..60i64 {
            for (i, px) in frame.chunks_exact_mut(4).enumerate() {
                let x = (i % 640) as i64;
                px.copy_from_slice(&[((x + n * 8) % 256) as u8, 80, 200, 255]);
            }
            let pts = n * cb_record::HNS / 30;
            sink.video(&frame, pts, (n + 1) * cb_record::HNS / 30 - pts).unwrap();
            let pcm: Vec<i16> = (0..1600).flat_map(|i| {
                let v = (((n * 1600 + i) as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 8000.0) as i16;
                [v, v]
            }).collect();
            sink.audio(&pcm, n * 1600 * cb_record::HNS / 48_000, 1600 * cb_record::HNS / 48_000).unwrap();
        }
        sink.finish().unwrap();
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        eprintln!("encoded {} bytes in {:?}", len, t0.elapsed());
        let head = std::fs::read(&path).unwrap();
        assert!(len > 10_000, "file too small: {len}");
        assert_eq!(&head[4..8], b"ftyp", "not an MP4 file");
        assert!(head.windows(4).any(|w| w == b"avc1"), "no H.264 track");
        assert!(head.windows(4).any(|w| w == b"mp4a"), "no AAC track");
        let _ = std::fs::remove_file(&path);
    }
}
