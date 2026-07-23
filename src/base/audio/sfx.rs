//! 短音效（SFX）声道
//!
//! 每个 SfxChannel 对应一条可独立控制的播放轨道。
//! 承载已解码的 PCM 数据、播放状态、音量、声像、淡变、效果器链。

use std::sync::Arc;

use crate::base::audio::GroupHandle;
use crate::base::subsystem::audio::common::StereoFrame;

use super::SoundData;

// ============================================================================
// 状态枚举
// ============================================================================

/// 声道播放状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelState {
    /// 正在播放
    Playing,
    /// 暂停
    Paused,
    /// 已停止 / 空闲
    Stopped,
}

// ============================================================================
// 淡变状态
// ============================================================================

/// 淡变类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeType {
    /// 淡入（音量 0→1）
    In,
    /// 淡出（音量 1→0）
    Out,
}

/// 淡变状态机
///
/// 基于帧数计算，不依赖时间，保证在音频线程中无锁无分配。
#[derive(Debug, Clone)]
pub struct FadeState {
    /// 淡变类型
    pub fade_type: FadeType,
    /// 已消耗的帧数
    pub elapsed: usize,
    /// 总淡变帧数
    pub total: usize,
}

impl FadeState {
    pub fn new_fade_in(ms: u32, sample_rate: u32) -> Self {
        let total = (ms as u64 * sample_rate as u64 / 1000) as usize;
        Self {
            fade_type: FadeType::In,
            elapsed: 0,
            total: total.max(1),
        }
    }

    pub fn new_fade_out(ms: u32, sample_rate: u32) -> Self {
        let total = (ms as u64 * sample_rate as u64 / 1000) as usize;
        Self {
            fade_type: FadeType::Out,
            elapsed: 0,
            total: total.max(1),
        }
    }

    /// 当前增益系数（0.0 ~ 1.0）
    pub fn gain(&self) -> f32 {
        let t = (self.elapsed as f32 / self.total as f32).clamp(0.0, 1.0);
        match self.fade_type {
            FadeType::In => t,
            FadeType::Out => 1.0 - t,
        }
    }

    /// 推进 N 帧，返回是否已完成
    pub fn advance(&mut self, n: usize) -> bool {
        self.elapsed += n;
        self.elapsed >= self.total
    }
}

// ============================================================================
// 效果器 trait
// ============================================================================

/// 通用音频效果器 trait
///
/// 实现此 trait 即可自定义任意 DSP 效果（失真、延时、混响、滤波……）。
/// 效果器会被串联在声道的数据通路上：`read_frames → effects → mix`
///
/// # 示例
///
/// ```ignore
/// use starfish::base::audio::sfx::AudioEffect;
///
/// struct Distortion { drive: f32 }
///
/// impl AudioEffect for Distortion {
///     fn name(&self) -> &str { "distortion" }
///     fn process(&mut self, frames: &mut [StereoFrame]) {
///         for f in frames {
///             f.left  = (f.left * self.drive).tanh();
///             f.right = (f.right * self.drive).tanh();
///         }
///     }
/// }
///
/// channel.effects.push(Box::new(Distortion { drive: 2.0 }));
/// ```
pub trait AudioEffect: Send + 'static {
    /// 效果器名称（调试用）
    fn name(&self) -> &str;

    /// 处理一帧 PCM 数据
    fn process(&mut self, frames: &mut [StereoFrame]);

    /// 声道停止时调用，用于清理效果器内部状态
    fn on_channel_stop(&mut self) {}
}

// ============================================================================
// SfxChannel
// ============================================================================

/// 单个 SFX 播放声道
pub struct SfxChannel {
    /// 播放状态
    pub state: ChannelState,
    /// 音效数据（None 表示空闲）
    pub data: Option<Arc<SoundData>>,
    /// 当前读取位置（帧索引）
    pub cursor: usize,
    /// 循环次数（-1 = 无限，0 = 一次，N = N+1 次）
    pub loops: i32,
    /// 音量（0.0 ~ 1.0）
    pub volume: f32,
    /// 声像（-1.0 左 ~ 0.0 中 ~ 1.0 右）
    pub pan: f32,
    /// 淡变状态（None = 无淡变）
    pub fade: Option<FadeState>,
    /// 效果器链（按添加顺序依次处理）
    pub effects: Vec<Box<dyn AudioEffect>>,
    /// 所属分组（None = 未分组）
    pub group: Option<GroupHandle>,
}

impl SfxChannel {
    /// 创建空闲声道
    pub fn new() -> Self {
        Self {
            state: ChannelState::Stopped,
            data: None,
            cursor: 0,
            loops: 0,
            volume: 1.0,
            pan: 0.0,
            fade: None,
            effects: Vec::new(),
            group: None,
        }
    }

    /// 创建带音效数据的声道（播放就绪）
    pub fn with_sound(
        sound: Arc<SoundData>,
        loops: i32,
        fade_in_ms: f32,
    ) -> Self {
        let sample_rate = sound.sample_rate;
        let fade = if fade_in_ms > 0.0 {
            Some(FadeState::new_fade_in(fade_in_ms as u32, sample_rate))
        } else {
            None
        };

        Self {
            state: ChannelState::Playing,
            data: Some(sound),
            cursor: 0,
            loops,
            volume: 1.0,
            pan: 0.0,
            fade,
            effects: Vec::new(),
            group: None,
        }
    }

    /// 获取当前播放的 SoundData（None = 空闲）
    pub fn get_sound(&self) -> Option<Arc<SoundData>> {
        self.data.clone()
    }

    /// 从 PCM 数据中读取一段帧（推进 cursor）
    ///
    /// 返回实际读取的帧数（到达末尾时可能少于 output.len()）
    pub fn read_frames(&mut self, output: &mut [StereoFrame]) -> usize {
        let Some(ref data) = self.data else {
            self.state = ChannelState::Stopped;
            return 0;
        };

        if self.cursor >= data.frames.len() {
            if self.loops == -1 {
                self.cursor = 0;
            } else if self.loops > 0 {
                self.cursor = 0;
                self.loops -= 1;
            } else {
                self.state = ChannelState::Stopped;
                return 0;
            }
        }

        let available = data.frames.len() - self.cursor;
        let to_read = output.len().min(available);

        output[..to_read]
            .copy_from_slice(&data.frames[self.cursor..self.cursor + to_read]);

        self.cursor += to_read;

        // ── 效果器链（在淡变之前应用，让淡变控制最终输出） ──
        for effect in &mut self.effects {
            effect.process(&mut output[..to_read]);
        }

        // ── 淡变处理 ──
        if let Some(fade) = &mut self.fade {
            let done = fade.advance(to_read);
            if done && fade.fade_type == FadeType::Out {
                self.state = ChannelState::Stopped;
                self.fade = None;
            } else if done {
                self.fade = None;
            }
        }

        to_read
    }

    /// 停止播放
    pub fn stop(&mut self) {
        self.state = ChannelState::Stopped;
        self.cursor = 0;
        self.fade = None;
        for effect in &mut self.effects {
            effect.on_channel_stop();
        }
    }

    /// 当前增益系数（受淡变影响）
    pub fn fade_gain(&self) -> f32 {
        self.fade.as_ref().map_or(1.0, |f| f.gain())
    }

    /// 是否处于活跃（播放或暂停）状态
    pub fn is_active(&self) -> bool {
        self.state == ChannelState::Playing || self.state == ChannelState::Paused
    }
}
