//! 音频数据载体
//!
//! 解码后的 PCM 数据，统一为 f32 采样。
//! 立体声源**原样保留两路**；单声道源**只存一份**（混音时展开为 L=R）——
//! 游戏 SFX 大多为单声道，省一半内存。
//! 适用于 SFX 和 Music 两个子系统。

use crate::base::subsystem::audio::common::{AudioError, StereoFrame};

/// 声道形态
#[derive(Debug, Clone)]
pub enum AudioChannels {
    /// 单声道：每帧一个样本，混音读取时展开为 L=R
    Mono(Vec<f32>),
    /// 立体声：左右帧原样保留
    Stereo(Vec<StereoFrame>),
}

/// 解码后的音频数据
///
/// 无论原始文件格式（WAV/OGG/MP3/FLAC），
/// 最终都解码为统一格式再封装进 SoundData：
///   - 采样格式：f32 [-1.0, 1.0]
///   - 声道形态：单声道（存一份） / 立体声（两路）
///   - 支持重采样为任意输出采样率
#[derive(Debug, Clone)]
pub struct SoundData {
    /// 采样率（Hz），如 44100、48000
    pub sample_rate: u32,
    /// 声道数据（单声道/立体声）
    pub channels: AudioChannels,
}

impl SoundData {
    /// 从文件加载（自动探测 OGG/MP3/FLAC/WAV，整段解码进内存）
    ///
    /// 适合短音效。长音频请改用
    /// [`MusicStream::from_file`](crate::base::audio::MusicStream::from_file)
    /// 流式加载，避免整曲占用内存。
    pub fn from_file(path: &str) -> Result<Self, AudioError> {
        crate::base::audio::decoder::SymphoniaDecoder::from_file(path)
    }

    /// 从原始交错 f32 立体声采样创建（L/R 交错）
    pub fn from_interleaved_f32(samples: &[f32], sample_rate: u32) -> Self {
        let frame_count = samples.len() / 2;
        let mut frames = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            frames.push(StereoFrame {
                left: samples[i * 2],
                right: samples[i * 2 + 1],
            });
        }
        Self {
            sample_rate,
            channels: AudioChannels::Stereo(frames),
        }
    }

    /// 从单声道 f32 采样创建
    ///
    /// **只存储一份**（不复制成双声道），混音读取时展开为 L=R。
    pub fn from_mono_f32(samples: &[f32], sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: AudioChannels::Mono(samples.to_vec()),
        }
    }

    /// 从立体声帧创建
    pub fn from_stereo_frames(frames: Vec<StereoFrame>, sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: AudioChannels::Stereo(frames),
        }
    }

    /// 总时长（秒）
    pub fn duration(&self) -> f32 {
        self.frame_count() as f32 / self.sample_rate as f32
    }

    /// 帧数
    pub fn frame_count(&self) -> usize {
        match &self.channels {
            AudioChannels::Mono(m) => m.len(),
            AudioChannels::Stereo(s) => s.len(),
        }
    }

    /// 原始采样数
    pub fn sample_count(&self) -> usize {
        match &self.channels {
            AudioChannels::Mono(m) => m.len(),
            AudioChannels::Stereo(s) => s.len() * 2,
        }
    }

    /// 重采样为目标采样率（线性插值，与流式路径同款算法）
    ///
    /// 采样率相同或数据为空时零开销返回克隆。
    pub fn resample(&self, target_sample_rate: u32) -> Self {
        if target_sample_rate == self.sample_rate || self.frame_count() == 0 {
            return self.clone();
        }
        let ratio = self.sample_rate as f64 / target_sample_rate as f64;
        let target_len = (self.frame_count() as f64 / ratio).ceil() as usize;

        match &self.channels {
            AudioChannels::Mono(m) => {
                let mut out = Vec::with_capacity(target_len);
                for i in 0..target_len {
                    let src_pos = i as f64 * ratio;
                    let idx = src_pos as usize;
                    let frac = (src_pos - idx as f64) as f32;
                    let v = if idx + 1 < m.len() {
                        m[idx] + (m[idx + 1] - m[idx]) * frac
                    } else {
                        m[m.len() - 1]
                    };
                    out.push(v);
                }
                Self {
                    sample_rate: target_sample_rate,
                    channels: AudioChannels::Mono(out),
                }
            }
            AudioChannels::Stereo(frames) => {
                let last = frames.len() - 1;
                let mut out = Vec::with_capacity(target_len);
                for i in 0..target_len {
                    let src_pos = i as f64 * ratio;
                    let idx = src_pos as usize;
                    let frac = (src_pos - idx as f64) as f32;
                    let frame = if idx + 1 <= last {
                        let a = &frames[idx];
                        let b = &frames[idx + 1];
                        StereoFrame {
                            left: a.left + (b.left - a.left) * frac,
                            right: a.right + (b.right - a.right) * frac,
                        }
                    } else {
                        frames[last]
                    };
                    out.push(frame);
                }
                Self {
                    sample_rate: target_sample_rate,
                    channels: AudioChannels::Stereo(out),
                }
            }
        }
    }

    /// 读取：从 `cursor` 帧起填充至多 `out.len()` 帧，返回实际帧数
    ///
    /// 单声道源在此展开为 L=R——这是唯一的形态分支点，
    /// 混音循环/流式/效果器链全部只面对 StereoFrame。
    pub(crate) fn read_into(&self, cursor: usize, out: &mut [StereoFrame]) -> usize {
        let total = self.frame_count();
        if cursor >= total {
            return 0;
        }
        let n = out.len().min(total - cursor);
        match &self.channels {
            AudioChannels::Mono(m) => {
                for (i, dst) in out[..n].iter_mut().enumerate() {
                    let s = m[cursor + i];
                    *dst = StereoFrame { left: s, right: s };
                }
            }
            AudioChannels::Stereo(frames) => {
                out[..n].copy_from_slice(&frames[cursor..cursor + n]);
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_stores_single_copy_and_expands_on_read() {
        let data = SoundData::from_mono_f32(&[0.5, 0.25, 0.75], 8000);
        assert_eq!(data.frame_count(), 3);

        let mut out = [StereoFrame::SILENT; 8];
        let n = data.read_into(0, &mut out);
        assert_eq!(n, 3);
        // 展开 L=R
        assert_eq!(out[0].left, 0.5);
        assert_eq!(out[0].right, 0.5);
        assert_eq!(out[2].right, 0.75);
        // 越界读为 0
        assert_eq!(data.read_into(3, &mut out), 0);
    }

    #[test]
    fn stereo_keeps_channels_distinct() {
        let data = SoundData::from_interleaved_f32(&[0.1, 0.9, 0.2, 0.8], 8000);
        assert_eq!(data.frame_count(), 2);
        assert_eq!(data.sample_count(), 4);

        let mut out = [StereoFrame::SILENT; 8];
        assert_eq!(data.read_into(0, &mut out), 2);
        assert_eq!(out[0].left, 0.1);
        assert_eq!(out[0].right, 0.9, "立体声两路必须各自保留");
    }

    #[test]
    fn resample_keeps_channel_shape() {
        // 单声道：重采样后仍是 Mono
        let mono = SoundData::from_mono_f32(&[0.0, 1.0], 8000).resample(16000);
        assert!(matches!(mono.channels, AudioChannels::Mono(_)));
        assert_eq!(mono.frame_count(), 4);

        // 立体声：重采样后仍是 Stereo
        let stereo = SoundData::from_interleaved_f32(&[0.1, 0.9, 0.2, 0.8], 8000).resample(16000);
        assert!(matches!(stereo.channels, AudioChannels::Stereo(_)));
        assert_eq!(stereo.frame_count(), 4);
    }
}
