//! 引擎层录音（AudioRecorder）
//!
//! 与 [`AudioMixer`](super::AudioMixer)（输出）对称的输入端：cpal 录音回调是
//! 生产者，主线程是消费者，中间复用与流式播放同一份 SPSC 环
//! （[`super::ring::SharedRing`]），音频线程全程无锁、无分配。
//!
//! 数据流：
//!
//! ```text
//! 麦克风 → cpal 输入流 → RingSink（数据面回调）→ SharedRing → 主线程 read/save_wav
//! ```
//!
//! 行为约定：
//! - **构造即采集**：打开流即 play，无显式 start（与 AudioMixer 风格一致）
//! - **溢出丢弃新数据并计数**：用户不及时 `read` 时，缓冲写满后丢弃新采集帧，
//!   [`dropped`](AudioRecorder::dropped) 可查——音频线程永不阻塞
//! - 长录音请周期性 `read`，或用
//!   [`new_with_capacity`](AudioRecorder::new_with_capacity) 一次性给足容量
//! - **采样率 = 设备真实采样率**（cpal 无设备边界转换；设备层见
//!   [`super::device`]），WAV 头据此写出

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::base::audio::common::{AudioError, AudioUserCallback, StereoFrame};
use crate::base::audio::device;
use crate::base::audio::ring::SharedRing;

/// 环形写入端（跑在录音回调线程上）：写满即丢新数据并计数
struct RingSink {
    ring: Arc<SharedRing>,
    dropped: Arc<AtomicU64>,
}

impl AudioUserCallback for RingSink {
    fn on_frames(&mut self, frames: &mut [StereoFrame]) {
        let written = self.ring.write(frames);
        if written < frames.len() {
            let lost = (frames.len() - written) as u64;
            self.dropped.fetch_add(lost, Ordering::Relaxed);
        }
    }
}

/// 引擎层录音器（AudioMixer 的输入端对位物）
///
/// # 示例
///
/// ```ignore
/// let mut rec = AudioRecorder::new()?;                  // 默认约 4 秒容量
/// println!("设备：{:?}", AudioRecorder::device_names()?);
/// std::thread::sleep(Duration::from_secs(5));           // 录 5 秒
/// let frames = rec.save_wav("recording.wav")?;          // 拉空并保存
/// ```
pub struct AudioRecorder {
    /// 持有 cpal 录音流（Drop 即停止采集并释放设备）
    _stream: device::DeviceStream,
    ring: Arc<SharedRing>,
    dropped: Arc<AtomicU64>,
    sample_rate: u32,
}

/// `AudioRecorder::new()` 的默认容量：≈4 秒 @48kHz（环形容量只需量级正确，
/// 它只决定溢出前的缓冲深度，WAV 头用的是设备真实采样率）
const DEFAULT_CAPACITY_FRAMES: usize = 192_000;

impl AudioRecorder {
    /// 打开默认录音设备并开始采集（f32 立体声，设备真实采样率）
    pub fn new() -> Result<Self, AudioError> {
        Self::new_with_capacity(DEFAULT_CAPACITY_FRAMES)
    }

    /// 打开默认录音设备，指定环形缓冲容量（帧，向上取 2 的幂）
    pub fn new_with_capacity(capacity_frames: usize) -> Result<Self, AudioError> {
        let ring = Arc::new(SharedRing::with_capacity(capacity_frames));
        let dropped = Arc::new(AtomicU64::new(0));

        let sink = RingSink {
            ring: ring.clone(),
            dropped: dropped.clone(),
        };

        let stream = device::open_input_stream(sink)?;
        let sample_rate = stream.spec.sample_rate;

        Ok(Self {
            _stream: stream,
            ring,
            dropped,
            sample_rate,
        })
    }

    /// 枚举系统录音设备名（诊断用；v1 打开的始终是默认设备）
    pub fn device_names() -> Result<Vec<String>, AudioError> {
        device::input_device_names()
    }

    // ───────────────────────── 数据面（全部非阻塞，主线程调用） ─────────────────────────

    /// 拉走至多 `out.len()` 帧已采集数据，返回实际帧数
    pub fn read(&mut self, out: &mut [StereoFrame]) -> usize {
        self.ring.read(out)
    }

    /// 当前缓冲内可读帧数
    pub fn available(&self) -> usize {
        self.ring.available()
    }

    /// 因缓冲写满而丢弃的帧数（累计值）
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// 丢弃缓冲内全部已采集数据
    pub fn clear(&mut self) {
        self.ring.clear();
    }

    /// 暂停采集（流级暂停，设备保持打开）
    pub fn pause(&mut self) -> Result<(), AudioError> {
        self._stream.pause()
    }

    /// 恢复采集
    pub fn resume(&mut self) -> Result<(), AudioError> {
        self._stream.resume()
    }

    /// 拉空缓冲并构建 16-bit PCM 立体声 WAV 字节
    ///
    /// 供 [`dialog::save_bytes`](crate::base::dialog::save_bytes) 上传保存
    /// （Android SAF 保存到用户可见位置），调用后缓冲被清空。
    pub fn wav_bytes(&mut self) -> Result<Vec<u8>, AudioError> {
        let mut frames = Vec::with_capacity(self.ring.available());
        let mut buf = vec![StereoFrame::SILENT; 4096];
        loop {
            let n = self.ring.read(&mut buf);
            if n == 0 {
                break;
            }
            frames.extend_from_slice(&buf[..n]);
        }
        Ok(build_wav_16(&frames, self.sample_rate))
    }

    /// 把缓冲内全部已采集数据写出为 16-bit PCM 立体声 WAV，返回帧数
    ///
    /// 调用后缓冲被清空。
    pub fn save_wav(&mut self, path: &str) -> Result<u64, AudioError> {
        let bytes = self.wav_bytes()?;
        let frames = bytes.len() as u64 / 4; // 16-bit × 2ch = 4 B/帧
        std::fs::write(path, bytes)
            .map_err(|e| AudioError::custom(format!("写入 WAV 失败 {path}: {e}")))?;
        Ok(frames)
    }

    /// 采样率（Hz）——设备真实采样率
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// 构建 16-bit PCM 立体声 WAV 字节（纯函数，可脱离设备测试）
pub(crate) fn build_wav_16(frames: &[StereoFrame], sample_rate: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity(frames.len() * 4);
    for f in frames {
        let l = (f.left.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        let r = (f.right.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        data.extend_from_slice(&l.to_le_bytes());
        data.extend_from_slice(&r.to_le_bytes());
    }

    let mut wav = Vec::with_capacity(data.len() + 44);
    let data_len = data.len() as u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&2u16.to_le_bytes()); // 立体声
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 4).to_le_bytes()); // byte rate
    wav.extend_from_slice(&4u16.to_le_bytes()); // 块对齐 = 2ch × 2B
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

/// 将帧数据写为 16-bit PCM 立体声 WAV 文件（[`build_wav_16`] + 落盘）
pub(crate) fn write_wav_16(
    path: &str,
    frames: &[StereoFrame],
    sample_rate: u32,
) -> Result<(), AudioError> {
    let wav = build_wav_16(frames, sample_rate);
    std::fs::write(path, wav)
        .map_err(|e| AudioError::custom(format!("写入 WAV 失败 {path}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::audio::decoder::SymphoniaDecoder;

    #[test]
    fn wav_round_trip() {
        // 生成 0.5s 440Hz 正弦（44100Hz 立体声）
        let rate = 44100u32;
        let n = (rate as f32 * 0.5) as usize;
        let frames: Vec<StereoFrame> = (0..n)
            .map(|i| {
                let s = (i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.5;
                StereoFrame { left: s, right: s }
            })
            .collect();

        let path = std::env::temp_dir().join("starfish_rec_test.wav");
        let path = path.to_str().unwrap();
        write_wav_16(path, &frames, rate).unwrap();

        // 用自家解码器读回，验证可解析且元数据一致
        let decoded = SymphoniaDecoder::from_file(path).unwrap();
        assert_eq!(decoded.sample_rate, rate);
        assert!(
            (decoded.duration() - 0.5).abs() < 0.01,
            "duration = {}",
            decoded.duration()
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn overflow_drops_and_counts() {
        let ring = Arc::new(SharedRing::with_capacity(16));
        let dropped = Arc::new(AtomicU64::new(0));
        let mut sink = RingSink {
            ring: ring.clone(),
            dropped: dropped.clone(),
        };

        // 一次喂 100 帧：容量 16 → 写 16 丢 84
        let mut frames: Vec<StereoFrame> =
            (0..100).map(|i| StereoFrame { left: i as f32, right: 0.0 }).collect();
        sink.on_frames(&mut frames);
        assert_eq!(ring.available(), 16);
        assert_eq!(dropped.load(Ordering::Relaxed), 84);

        // 读空后再喂：不再丢弃
        let mut out = vec![StereoFrame::SILENT; 16];
        assert_eq!(ring.read(&mut out), 16);
        let mut more: Vec<StereoFrame> =
            (0..10).map(|i| StereoFrame { left: i as f32, right: 0.0 }).collect();
        sink.on_frames(&mut more);
        assert_eq!(dropped.load(Ordering::Relaxed), 84);
        assert_eq!(ring.available(), 10);
    }

    #[test]
    fn clear_discards_pending() {
        let ring = Arc::new(SharedRing::with_capacity(64));
        let mut sink = RingSink {
            ring: ring.clone(),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let mut frames: Vec<StereoFrame> =
            (0..30).map(|i| StereoFrame { left: i as f32, right: 0.0 }).collect();
        sink.on_frames(&mut frames);
        assert_eq!(ring.available(), 30);

        ring.clear();
        assert_eq!(ring.available(), 0);

        // clear 后继续采集/读取正常，数据从 clear 点起连续
        let mut more: Vec<StereoFrame> =
            (0..5).map(|i| StereoFrame { left: i as f32, right: 0.0 }).collect();
        sink.on_frames(&mut more);
        let mut out = vec![StereoFrame::SILENT; 16];
        assert_eq!(ring.read(&mut out), 5);
        assert_eq!(out[0].left, 0.0);
    }
}
