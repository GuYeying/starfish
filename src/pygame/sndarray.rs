//! pygame.sndarray 对应物（ADR-10 通用数据桥的**音频端点**）
//!
//! pygame 的 sndarray 靠 numpy 互转 Sound ↔ 采样数组；本层等价物 =
//! [`SoundArray`]（交错 f32 + 格式描述）↔ `base::audio::SoundData`
//! 双向桥——热路径格式与引擎混音器同源（f32 [-1.0, 1.0]），零转码。
//!
//! 对齐面：`get_array`（Sound → 数组）/ `make_sound`（数组 → Sound）。
//! 差异：pygame 默认 int16，本层恒 f32（绑定位按需换算）。

use crate::base::audio::{SoundData, StereoFrame};

/// 交错采样数组 + 格式描述
#[derive(Debug, Clone, PartialEq)]
pub struct SoundArray {
    /// 采样率（Hz）
    pub sample_rate: u32,
    /// 声道数（1 = 单声道，2 = 立体声）
    pub channels: u8,
    /// 交错采样 [-1.0, 1.0]（单声道 = 逐样本；立体声 = L,R 交错）
    pub samples: Vec<f32>,
}

impl SoundArray {
    /// 单声道数组构造（长度 = 帧数）
    pub fn mono(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self { sample_rate, channels: 1, samples }
    }

    /// 立体声交错数组构造（长度 = 帧数 × 2）
    pub fn stereo(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self { sample_rate, channels: 2, samples }
    }

    /// 帧数（一帧 = 一次采样的全部声道）
    pub fn frame_count(&self) -> usize {
        match self.channels {
            1 => self.samples.len(),
            2 => self.samples.len() / 2,
            _ => 0,
        }
    }
}

/// SoundData → 采样数组（对位 `sndarray.array`）
pub fn get_array(data: &SoundData) -> SoundArray {
    match &data.channels {
        crate::base::audio::AudioChannels::Mono(m) => SoundArray {
            sample_rate: data.sample_rate,
            channels: 1,
            samples: m.clone(),
        },
        crate::base::audio::AudioChannels::Stereo(frames) => {
            let mut samples = Vec::with_capacity(frames.len() * 2);
            for f in frames {
                samples.push(f.left);
                samples.push(f.right);
            }
            SoundArray {
                sample_rate: data.sample_rate,
                channels: 2,
                samples,
            }
        }
    }
}

/// 采样数组 → SoundData（对位 `sndarray.make_sound`）
///
/// 声道数非 1/2、或采样长度与声道不整除 → Err。
pub fn make_sound(arr: &SoundArray) -> Result<SoundData, String> {
    match arr.channels {
        1 => Ok(SoundData::from_mono_f32(&arr.samples, arr.sample_rate)),
        2 => {
            if arr.samples.len() % 2 != 0 {
                return Err("make_sound: 立体声采样长度须为偶数（L,R 交错）".into());
            }
            let frames: Vec<StereoFrame> = arr
                .samples
                .chunks_exact(2)
                .map(|p| StereoFrame { left: p[0], right: p[1] })
                .collect();
            Ok(SoundData::from_stereo_frames(frames, arr.sample_rate))
        }
        n => Err(format!("make_sound: 不支持的声道数 {n}（仅 1/2）")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_roundtrip() {
        let data = SoundData::from_mono_f32(&[0.1, -0.2, 0.3], 44100);
        let arr = get_array(&data);
        assert_eq!(arr.channels, 1);
        assert_eq!(arr.frame_count(), 3);
        let back = make_sound(&arr).unwrap();
        assert_eq!(back.sample_rate, 44100);
        match back.channels {
            crate::base::audio::AudioChannels::Mono(m) => {
                assert_eq!(m, vec![0.1, -0.2, 0.3])
            }
            _ => panic!("应为单声道"),
        }
    }

    #[test]
    fn stereo_roundtrip_and_validation() {
        let data = SoundData::from_interleaved_f32(&[0.5, -0.5, 0.25, -0.25], 48000);
        let arr = get_array(&data);
        assert_eq!(arr.channels, 2);
        assert_eq!(arr.frame_count(), 2);

        let back = make_sound(&arr).unwrap();
        assert_eq!(back.sample_rate, 48000);

        // 奇数长度立体声 → Err
        let bad = SoundArray::stereo(vec![0.1, 0.2, 0.3], 48000);
        assert!(make_sound(&bad).is_err());
        // 声道数 3 → Err
        let tri = SoundArray { sample_rate: 8000, channels: 3, samples: vec![0.0; 3] };
        assert!(make_sound(&tri).is_err());
    }
}
