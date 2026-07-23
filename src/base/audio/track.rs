//! 音频数据载体
//!
//! 解码后的 PCM 数据，统一为浮点立体声帧格式。
//! 适用于 SFX 和 Music 两个子系统。

use crate::base::subsystem::audio::common::StereoFrame;

/// 解码后的音频数据
///
/// 无论原始文件格式（WAV/OGG/MP3/FLAC），
/// 最终都解码为统一格式再封装进 SoundData：
///   - 采样格式：f32 [-1.0, 1.0]
///   - 声道数：立体声（双声道交错）
///   - 存储形式：Vec<StereoFrame>
///   - 支持重采样为任意输出采样率
#[derive(Debug, Clone)]
pub struct SoundData {
    /// PCM 帧数据（解码后的立体声浮点采样）
    pub frames: Vec<StereoFrame>,
    /// 采样率（Hz），如 44100、48000
    pub sample_rate: u32,
}

impl SoundData {
    /// 从原始交错 f32 采样创建 SoundData
    ///
    /// `samples` 必须是偶数长度（L/R 交错），会被转换为 StereoFrame 数组。
    pub fn from_interleaved_f32(samples: &[f32], sample_rate: u32) -> Self {
        let frame_count = samples.len() / 2;
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            frames.push(StereoFrame {
                left: samples[i * 2],
                right: samples[i * 2 + 1],
            });
        }
        Self { frames, sample_rate }
    }

    /// 从单声道 f32 采样创建 SoundData（自动复制到双声道）
    pub fn from_mono_f32(samples: &[f32], sample_rate: u32) -> Self {
        let frames: Vec<StereoFrame> = samples
            .iter()
            .map(|&s| StereoFrame { left: s, right: s })
            .collect();
        Self { frames, sample_rate }
    }

    /// 总时长（秒）
    pub fn duration(&self) -> f32 {
        self.frames.len() as f32 / self.sample_rate as f32
    }

    /// 原始 PCM 帧数
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 原始 PCM 采样数（帧数 × 2）
    pub fn sample_count(&self) -> usize {
        self.frames.len() * 2
    }

    /// 重采样为目标采样率
    ///
    /// 使用线性插值，质量足以满足游戏音频需求。
    /// 如果 `target_sample_rate` 与当前相同，直接返回 clone（零开销）。
    pub fn resample(&self, target_sample_rate: u32) -> Self {
        if target_sample_rate == self.sample_rate || self.frames.is_empty() {
            return self.clone();
        }

        let ratio = self.sample_rate as f64 / target_sample_rate as f64;
        let target_len = (self.frames.len() as f64 / ratio).ceil() as usize;
        let mut out = Vec::with_capacity(target_len);
        let last = self.frames.len() - 1;

        for i in 0..target_len {
            let src_pos = i as f64 * ratio;
            let src_idx = src_pos as usize;
            let frac = src_pos.fract() as f32;

            let frame = if src_idx + 1 <= last {
                let a = &self.frames[src_idx];
                let b = &self.frames[src_idx + 1];
                StereoFrame {
                    left: a.left + (b.left - a.left) * frac,
                    right: a.right + (b.right - a.right) * frac,
                }
            } else {
                self.frames[last]
            };
            out.push(frame);
        }

        Self {
            frames: out,
            sample_rate: target_sample_rate,
        }
    }
}
