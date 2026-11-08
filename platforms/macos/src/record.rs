//! MP4 recording on macOS: AVAssetWriter (H.264 + AAC) for encoding and
//! ScreenCaptureKit for system audio (macOS 13+, needs the Screen Recording
//! permission). The microphone goes through cpal/CoreAudio.

#![allow(deprecated)]

use cb_record::{AudioFeed, SinkConfig, VideoSink, AUDIO_CHANNELS, AUDIO_RATE, HNS};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_av_foundation::*;
use objc2_core_audio_types::{AudioBuffer, AudioBufferList, AudioStreamBasicDescription};
use objc2_core_foundation::{CFRetained, CFString};
use objc2_core_media::*;
use objc2_core_video::*;
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol, NSString, NSURL};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

const K_AUDIO_FORMAT_LINEAR_PCM: u32 = 0x6c70636d; // 'lpcm'
const K_AUDIO_FORMAT_MPEG4_AAC: u32 = 0x61616320; // 'aac '
const FLAG_FLOAT: u32 = 1 << 0;
const FLAG_SIGNED_INT: u32 = 1 << 2;
const FLAG_PACKED: u32 = 1 << 3;
const FLAG_NON_INTERLEAVED: u32 = 1 << 5;

fn ns_err(e: &NSError) -> String {
    e.localizedDescription().to_string()
}

fn num_i(v: i32) -> Retained<NSNumber> {
    NSNumber::numberWithInt(v)
}

/// CF strings are toll-free bridged to NSString.
fn cf_key(s: &CFString) -> &NSString {
    unsafe { &*(s as *const CFString as *const NSString) }
}

fn cmtime(hns: i64) -> CMTime {
    unsafe { CMTime::new(hns, HNS as i32) }
}

pub struct AvSink {
    writer: Retained<AVAssetWriter>,
    vin: Retained<AVAssetWriterInput>,
    adaptor: Retained<AVAssetWriterInputPixelBufferAdaptor>,
    ain: Option<Retained<AVAssetWriterInput>>,
    afmt: Option<CFRetained<CMAudioFormatDescription>>,
    width: usize,
    height: usize,
}

// AVAssetWriter and its inputs may be used from any (single) thread.
unsafe impl Send for AvSink {}

impl AvSink {
    pub fn open(cfg: &SinkConfig) -> Result<AvSink, String> {
        let _ = std::fs::remove_file(&cfg.path);
        unsafe {
            let url = NSURL::from_file_path(&cfg.path).ok_or("无效的文件路径")?;
            let writer = AVAssetWriter::assetWriterWithURL_fileType_error(&url, AVFileTypeMPEG4.ok_or("AVFileTypeMPEG4")?).map_err(|e| ns_err(&e))?;
            // Video input: H.264 at the requested size and bit rate.
            let comp = NSDictionary::<NSString, AnyObject>::from_slices(&[AVVideoAverageBitRateKey.ok_or("key")?], &[&*num_i(cfg.video_bitrate as i32) as &AnyObject]);
            let codec: &AnyObject = AVVideoCodecTypeH264.ok_or("H264")?;
            let (w, h) = (num_i(cfg.width as i32), num_i(cfg.height as i32));
            let vset = NSDictionary::<NSString, AnyObject>::from_slices(
                &[AVVideoCodecKey.ok_or("key")?, AVVideoWidthKey.ok_or("key")?, AVVideoHeightKey.ok_or("key")?, AVVideoCompressionPropertiesKey.ok_or("key")?],
                &[codec, &*w as &AnyObject, &*h as &AnyObject, &*comp as &AnyObject],
            );
            let vin = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeVideo.ok_or("video")?, Some(&vset));
            vin.setExpectsMediaDataInRealTime(true);
            let fmt = num_i(kCVPixelFormatType_32BGRA as i32);
            let attrs = NSDictionary::<NSString, AnyObject>::from_slices(
                &[cf_key(kCVPixelBufferPixelFormatTypeKey), cf_key(kCVPixelBufferWidthKey), cf_key(kCVPixelBufferHeightKey)],
                &[&*fmt as &AnyObject, &*w as &AnyObject, &*h as &AnyObject],
            );
            let adaptor = AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(&vin, Some(&attrs));
            if !writer.canAddInput(&vin) {
                return Err("无法添加视频轨道".into());
            }
            writer.addInput(&vin);
            // Audio input: AAC 192 kbit/s from 16-bit stereo PCM.
            let (ain, afmt) = if cfg.audio {
                let keys = [objc2_avf_audio_keys::format_id()?, objc2_avf_audio_keys::sample_rate()?, objc2_avf_audio_keys::channels()?, objc2_avf_audio_keys::bit_rate()?];
                let (fid, sr, ch, br) = (num_i(K_AUDIO_FORMAT_MPEG4_AAC as i32), NSNumber::numberWithDouble(AUDIO_RATE as f64), num_i(AUDIO_CHANNELS as i32), num_i(192_000));
                let aset = NSDictionary::<NSString, AnyObject>::from_slices(&keys, &[&*fid as &AnyObject, &*sr as &AnyObject, &*ch as &AnyObject, &*br as &AnyObject]);
                let ain = AVAssetWriterInput::assetWriterInputWithMediaType_outputSettings(AVMediaTypeAudio.ok_or("audio")?, Some(&aset));
                ain.setExpectsMediaDataInRealTime(true);
                if !writer.canAddInput(&ain) {
                    return Err("无法添加音频轨道".into());
                }
                writer.addInput(&ain);
                let asbd = AudioStreamBasicDescription {
                    mSampleRate: AUDIO_RATE as f64,
                    mFormatID: K_AUDIO_FORMAT_LINEAR_PCM,
                    mFormatFlags: FLAG_SIGNED_INT | FLAG_PACKED,
                    mBytesPerPacket: 2 * AUDIO_CHANNELS as u32,
                    mFramesPerPacket: 1,
                    mBytesPerFrame: 2 * AUDIO_CHANNELS as u32,
                    mChannelsPerFrame: AUDIO_CHANNELS as u32,
                    mBitsPerChannel: 16,
                    mReserved: 0,
                };
                let mut out: *const CMAudioFormatDescription = std::ptr::null();
                let st = CMAudioFormatDescriptionCreate(None, NonNull::from(&asbd), 0, std::ptr::null(), 0, std::ptr::null(), None, NonNull::from(&mut out));
                let fmt = NonNull::new(out as *mut CMAudioFormatDescription).filter(|_| st == 0).ok_or(format!("音频格式错误 {st}"))?;
                (Some(ain), Some(CFRetained::from_raw(fmt)))
            } else {
                (None, None)
            };
            if !writer.startWriting() {
                return Err(writer.error().map(|e| ns_err(&e)).unwrap_or_else(|| "无法开始写入视频".into()));
            }
            writer.startSessionAtSourceTime(cmtime(0));
            Ok(AvSink { writer, vin, adaptor, ain, afmt, width: cfg.width as usize, height: cfg.height as usize })
        }
    }

    fn wait_ready(input: &AVAssetWriterInput) -> bool {
        let t0 = Instant::now();
        while !unsafe { input.isReadyForMoreMediaData() } {
            if t0.elapsed() > Duration::from_millis(500) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        true
    }

    fn failure(&self, what: &str) -> String {
        unsafe { self.writer.error() }.map(|e| format!("{what}：{}", ns_err(&e))).unwrap_or_else(|| what.to_string())
    }
}

impl VideoSink for AvSink {
    fn video(&mut self, bgra: &[u8], pts: i64, _dur: i64) -> Result<(), String> {
        if !Self::wait_ready(&self.vin) {
            return Ok(()); // Encoder busy: drop this frame.
        }
        unsafe {
            let mut pb: *mut CVPixelBuffer = std::ptr::null_mut();
            let r = match self.adaptor.pixelBufferPool() {
                Some(pool) => CVPixelBufferPoolCreatePixelBuffer(None, &pool, NonNull::from(&mut pb)),
                None => CVPixelBufferCreate(None, self.width, self.height, kCVPixelFormatType_32BGRA, None, NonNull::from(&mut pb)),
            };
            let pb = NonNull::new(pb).filter(|_| r == 0).ok_or(format!("无法分配视频帧 ({r})"))?;
            let pb: CFRetained<CVPixelBuffer> = CFRetained::from_raw(pb);
            CVPixelBufferLockBaseAddress(&pb, CVPixelBufferLockFlags(0));
            let base = CVPixelBufferGetBaseAddress(&pb) as *mut u8;
            let stride = CVPixelBufferGetBytesPerRow(&pb);
            let row = self.width * 4;
            if !base.is_null() {
                for y in 0..self.height {
                    std::ptr::copy_nonoverlapping(bgra.as_ptr().add(y * row), base.add(y * stride), row);
                }
            }
            CVPixelBufferUnlockBaseAddress(&pb, CVPixelBufferLockFlags(0));
            if !self.adaptor.appendPixelBuffer_withPresentationTime(&pb, cmtime(pts)) {
                return Err(self.failure("写入视频失败"));
            }
        }
        Ok(())
    }

    fn audio(&mut self, pcm: &[i16], pts: i64, _dur: i64) -> Result<(), String> {
        let (Some(ain), Some(fmt)) = (&self.ain, &self.afmt) else { return Ok(()) };
        if pcm.is_empty() || !Self::wait_ready(ain) {
            return Ok(());
        }
        unsafe {
            let len = pcm.len() * 2;
            let mut bb: *mut CMBlockBuffer = std::ptr::null_mut();
            let st = CMBlockBufferCreateWithMemoryBlock(None, std::ptr::null_mut(), len, None, std::ptr::null(), 0, len, 1, NonNull::from(&mut bb));
            let bb = NonNull::new(bb).filter(|_| st == 0).ok_or(format!("音频缓冲区错误 {st}"))?;
            let bb: CFRetained<CMBlockBuffer> = CFRetained::from_raw(bb);
            let st = CMBlockBufferReplaceDataBytes(NonNull::new_unchecked(pcm.as_ptr() as *mut c_void), &bb, 0, len);
            if st != 0 {
                return Err(format!("音频缓冲区错误 {st}"));
            }
            let mut sb: *mut CMSampleBuffer = std::ptr::null_mut();
            let st = CMAudioSampleBufferCreateReadyWithPacketDescriptions(None, &bb, fmt, (pcm.len() / 2) as CMItemCount, cmtime(pts), std::ptr::null(), NonNull::from(&mut sb));
            let sb = NonNull::new(sb).filter(|_| st == 0).ok_or(format!("音频样本错误 {st}"))?;
            let sb: CFRetained<CMSampleBuffer> = CFRetained::from_raw(sb);
            if !ain.appendSampleBuffer(&sb) {
                return Err(self.failure("写入音频失败"));
            }
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<(), String> {
        unsafe {
            self.vin.markAsFinished();
            if let Some(a) = &self.ain {
                a.markAsFinished();
            }
            let (tx, rx) = channel::<()>();
            let block = block2::RcBlock::new(move || {
                let _ = tx.send(());
            });
            self.writer.finishWritingWithCompletionHandler(&block);
            if rx.recv_timeout(Duration::from_secs(20)).is_err() {
                return Err("完成视频文件超时".into());
            }
            if self.writer.status() != AVAssetWriterStatus::Completed {
                return Err(self.failure("无法完成视频文件"));
            }
        }
        Ok(())
    }
}

/// AVFAudio settings keys (declared in AVFAudio, linked through AVFoundation).
mod objc2_avf_audio_keys {
    use objc2_foundation::NSString;
    extern "C" {
        static AVFormatIDKey: &'static NSString;
        static AVSampleRateKey: &'static NSString;
        static AVNumberOfChannelsKey: &'static NSString;
        static AVEncoderBitRateKey: &'static NSString;
    }
    pub fn format_id() -> Result<&'static NSString, String> {
        Ok(unsafe { AVFormatIDKey })
    }
    pub fn sample_rate() -> Result<&'static NSString, String> {
        Ok(unsafe { AVSampleRateKey })
    }
    pub fn channels() -> Result<&'static NSString, String> {
        Ok(unsafe { AVNumberOfChannelsKey })
    }
    pub fn bit_rate() -> Result<&'static NSString, String> {
        Ok(unsafe { AVEncoderBitRateKey })
    }
}

// ------------------------------------------------------------ system audio

mod sck {
    use super::*;
    use objc2_screen_capture_kit::{SCContentFilter, SCShareableContent, SCStream, SCStreamConfiguration, SCStreamOutput, SCStreamOutputType, SCWindow};

    pub struct Ivars {
        pub feed: AudioFeed,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop impl.
        #[unsafe(super(NSObject))]
        #[name = "CloudboardAudioTap"]
        #[ivars = Ivars]
        pub struct AudioTap;

        unsafe impl NSObjectProtocol for AudioTap {}

        unsafe impl SCStreamOutput for AudioTap {
            #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
            unsafe fn on_sample(&self, _stream: &SCStream, sample: &CMSampleBuffer, kind: SCStreamOutputType) {
                if kind == SCStreamOutputType::Audio {
                    push_samples(&self.ivars().feed, sample);
                }
            }
        }
    );

    impl AudioTap {
        pub fn new(feed: AudioFeed) -> Retained<AudioTap> {
            let this = Self::alloc().set_ivars(Ivars { feed });
            unsafe { msg_send![super(this), init] }
        }
    }

    /// Copy (and interleave) float PCM out of a ScreenCaptureKit sample buffer.
    unsafe fn push_samples(feed: &AudioFeed, sample: &CMSampleBuffer) {
        let Some(fd) = CMSampleBufferGetFormatDescription(sample) else { return };
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&fd);
        if asbd.is_null() {
            return;
        }
        let asbd = *asbd;
        if asbd.mFormatFlags & FLAG_FLOAT == 0 || asbd.mBitsPerChannel != 32 {
            return;
        }
        // Room for up to 8 buffers.
        #[repr(C)]
        struct List {
            n: u32,
            bufs: [AudioBuffer; 8],
        }
        let mut list: List = std::mem::zeroed();
        let mut block: *mut CMBlockBuffer = std::ptr::null_mut();
        let st = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(sample, std::ptr::null_mut(), &mut list as *mut List as *mut AudioBufferList, std::mem::size_of::<List>(), None, None, 0, &mut block);
        if st != 0 {
            return;
        }
        let _guard = NonNull::new(block).map(|b| CFRetained::<CMBlockBuffer>::from_raw(b));
        let n = (list.n as usize).min(8);
        let ch = asbd.mChannelsPerFrame.max(1) as usize;
        let rate = asbd.mSampleRate as u32;
        if asbd.mFormatFlags & FLAG_NON_INTERLEAVED != 0 && n >= 1 {
            let frames = list.bufs[0].mDataByteSize as usize / 4;
            let chans: Vec<&[f32]> = (0..n.min(2)).map(|i| std::slice::from_raw_parts(list.bufs[i].mData as *const f32, frames)).collect();
            let mut inter = Vec::with_capacity(frames * 2);
            for f in 0..frames {
                inter.push(chans[0][f]);
                inter.push(chans[chans.len() - 1][f]);
            }
            feed.push(&inter, rate, 2);
        } else if n >= 1 {
            let data = std::slice::from_raw_parts(list.bufs[0].mData as *const f32, list.bufs[0].mDataByteSize as usize / 4);
            feed.push(data, rate, ch as u16);
        }
    }

    /// Running capture; stops when dropped.
    pub struct Capture {
        stream: Retained<SCStream>,
        _tap: Retained<AudioTap>,
    }

    impl Drop for Capture {
        fn drop(&mut self) {
            unsafe { self.stream.stopCaptureWithCompletionHandler(None) };
        }
    }

    pub fn start(feed: AudioFeed) -> Result<Capture, String> {
        unsafe {
            // Shareable content (asks for the Screen Recording permission).
            let (tx, rx) = channel::<Result<Retained<SCShareableContent>, String>>();
            let block = block2::RcBlock::new(move |content: *mut SCShareableContent, err: *mut NSError| {
                let r = match (Retained::retain(content), err.as_ref()) {
                    (Some(c), _) => Ok(c),
                    (None, Some(e)) => Err(ns_err(e)),
                    _ => Err("没有可录制的屏幕".into()),
                };
                let _ = tx.send(r);
            });
            SCShareableContent::getShareableContentWithCompletionHandler(&block);
            let content = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "等待系统授权超时".to_string())??;
            let displays = content.displays();
            let display = displays.firstObject().ok_or("没有可录制的屏幕")?;
            let filter = SCContentFilter::initWithDisplay_excludingWindows(SCContentFilter::alloc(), &display, &NSArray::<SCWindow>::new());
            let cfg = SCStreamConfiguration::new();
            cfg.setCapturesAudio(true);
            cfg.setSampleRate(AUDIO_RATE as isize);
            cfg.setChannelCount(2);
            cfg.setExcludesCurrentProcessAudio(false);
            // We only want audio: keep the video side tiny and slow.
            cfg.setWidth(2);
            cfg.setHeight(2);
            cfg.setMinimumFrameInterval(CMTime::new(1, 2));
            let stream = SCStream::initWithFilter_configuration_delegate(SCStream::alloc(), &filter, &cfg, None);
            let tap = AudioTap::new(feed);
            let queue = dispatch2::DispatchQueue::new("cloudboard.audio", None);
            stream.addStreamOutput_type_sampleHandlerQueue_error(ProtocolObject::from_ref(&*tap), SCStreamOutputType::Audio, Some(&queue)).map_err(|e| ns_err(&e))?;
            let (tx, rx) = channel::<Option<String>>();
            let done = block2::RcBlock::new(move |err: *mut NSError| {
                let _ = tx.send(err.as_ref().map(ns_err));
            });
            stream.startCaptureWithCompletionHandler(Some(&done));
            match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(None) => Ok(Capture { stream, _tap: tap }),
                Ok(Some(e)) => Err(e),
                Err(_) => Err("启动系统声音录制超时".into()),
            }
        }
    }
}

/// macOS 13+ is required for ScreenCaptureKit audio.
fn has_screencapturekit_audio() -> bool {
    let out = std::process::Command::new("sw_vers").arg("-productVersion").output();
    let v = out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    v.split('.').next().and_then(|m| m.parse::<u32>().ok()).is_some_and(|m| m >= 13)
}

/// The recording backend for macOS.
pub fn backend() -> cb_record::Backend {
    #[cfg(feature = "audio")]
    let microphone = Some(cb_record::cpal_microphone());
    #[cfg(not(feature = "audio"))]
    let microphone = None;
    let system_audio: Option<cb_record::CaptureFn> = Some(std::sync::Arc::new(|feed: AudioFeed| {
        if !has_screencapturekit_audio() {
            return Err("需要 macOS 13 或更高版本".into());
        }
        sck::start(feed).map(|c| Box::new(c) as Box<dyn std::any::Any>).map_err(|e| format!("{e}（请在“系统设置 > 隐私与安全性 > 屏幕录制”中允许 Cloudboard）"))
    }));
    cb_record::Backend {
        sink: std::sync::Arc::new(|c: &SinkConfig| AvSink::open(c).map(|s| Box::new(s) as Box<dyn VideoSink>)),
        microphone,
        system_audio,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes two seconds of synthetic video + a tone with AVAssetWriter.
    #[test]
    fn encodes_mp4() {
        let path = std::env::temp_dir().join(format!("cb-record-test-{}.mp4", std::process::id()));
        let cfg = SinkConfig { path: path.clone(), width: 640, height: 360, fps: 30, audio: true, video_bitrate: 2_000_000 };
        let mut sink = Box::new(AvSink::open(&cfg).expect("open AVAssetWriter"));
        let mut frame = vec![0u8; 640 * 360 * 4];
        for n in 0..60i64 {
            for (i, px) in frame.chunks_exact_mut(4).enumerate() {
                let x = (i % 640) as i64;
                px.copy_from_slice(&[((x + n * 8) % 256) as u8, 80, 200, 255]);
            }
            let pts = n * HNS / 30;
            sink.video(&frame, pts, (n + 1) * HNS / 30 - pts).unwrap();
            let pcm: Vec<i16> = (0..1600)
                .flat_map(|i| {
                    let v = (((n * 1600 + i) as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 8000.0) as i16;
                    [v, v]
                })
                .collect();
            sink.audio(&pcm, n * 1600 * HNS / 48_000, 1600 * HNS / 48_000).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        sink.finish().unwrap();
        let data = std::fs::read(&path).unwrap();
        eprintln!("encoded {} bytes", data.len());
        assert!(data.len() > 10_000, "file too small");
        assert_eq!(&data[4..8], b"ftyp", "not an MP4 file");
        assert!(data.windows(4).any(|w| w == b"avc1"), "no H.264 track");
        assert!(data.windows(4).any(|w| w == b"mp4a"), "no AAC track");
        let _ = std::fs::remove_file(&path);
    }
}
