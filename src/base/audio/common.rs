//! 音频共享叶子类型
//!
//! SFX 与 Music 两个前端共用的最小公共件：声道状态、淡变状态机、效果器 trait。
//! 它们属于"voice（发声体）"的属性，与数据来源（内存缓冲 / 流式解码）无关。

use crate::base::subsystem::audio::common::StereoFrame;

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
/// # 契约（音频线程执行，必须遵守）
///
/// - [`process`](AudioEffect::process) 在**音频回调线程**上按缓冲调用：
///   实现内**不得分配内存、不得阻塞、耗时有上界**（否则会产生可听见的爆音）
/// - 效果器属于"播放实例"而非"文件"：曲目切换 / 淡变不清理效果器状态
///   （例如回声缓冲跨曲目延续），仅在声道 [`stop`](AudioEffect::on_channel_stop) 时通知
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
