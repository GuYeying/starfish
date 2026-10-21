//! 音频公共件
//!
//! 两部分：
//! - **数据面**：[`StereoFrame`]、[`AudioError`]、[`AudioUserCallback`]——
//!   播放/录音/混音/解码全模块统一面对的数据契约
//! - **voice 面**：声道状态、淡变状态机、效果器 trait——与数据来源
//!   （内存缓冲 / 流式解码）无关的发声体属性
//!
//! 本文件自包含（原居 subsystem/audio，SDL 设备层退役后迁入），
//! 全部类型零平台依赖——平台差异只允许出现在 [`super::device`]（设备层）。

use std::fmt;

// ============================================================================
// 数据面
// ============================================================================

/// 引擎统一立体声帧（全平台/播放/录音标准格式 f32 [-1.0, 1.0]）
///
/// `#[repr(C)] { f32, f32 }` 与交错立体声 f32 内存布局一致，
/// 设备层经 bytemuck 零拷贝 reinterpret。
#[derive(Debug, Clone, Copy, Default, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct StereoFrame {
    pub left: f32,
    pub right: f32,
}

impl StereoFrame {
    /// 静音帧常量
    pub const SILENT: Self = Self { left: 0.0, right: 0.0 };
}

/// 音频通用错误（全模块唯一错误类型）
#[derive(Debug)]
pub enum AudioError {
    /// 设备层错误（打开/枚举/流操作失败，文本来自后端）
    Device(String),
    BufferMismatch,
    UnsupportedFormat,
    Custom(String),
}

// 构造快捷方法
impl AudioError {
    pub fn custom(msg: impl Into<String>) -> Self {
        Self::Custom(msg.into())
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Device(s) => write!(f, "audio device: {s}"),
            AudioError::BufferMismatch => write!(f, "Audio buffer size mismatch"),
            AudioError::UnsupportedFormat => write!(f, "Unsupported audio format"),
            AudioError::Custom(s) => write!(f, "Audio: {s}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// 上层业务统一回调 Trait
/// 播放：填充帧数据 | 录音：处理采集帧数据
pub trait AudioUserCallback: Send + 'static {
    fn on_frames(&mut self, frames: &mut [StereoFrame]);
}

/// 为普通闭包自动实现 Trait，使用更便捷
impl<F: FnMut(&mut [StereoFrame]) + Send + 'static> AudioUserCallback for F {
    fn on_frames(&mut self, frames: &mut [StereoFrame]) {
        self(frames);
    }
}

/// 交错 f32 采样 → 立体声帧（限幅 [-1, 1]）
#[inline(always)]
pub fn samples_to_frames(src: &[f32], dst: &mut [StereoFrame]) -> Result<(), AudioError> {
    if src.len() % 2 != 0 {
        return Err(AudioError::custom("stereo samples length must be even"));
    }
    let frame_count = src.len() / 2;
    if frame_count > dst.len() {
        return Err(AudioError::BufferMismatch);
    }
    for i in 0..frame_count {
        dst[i].left = src[i * 2].clamp(-1.0, 1.0);
        dst[i].right = src[i * 2 + 1].clamp(-1.0, 1.0);
    }
    Ok(())
}

/// 立体声帧 → 交错 f32 采样
#[inline(always)]
pub fn frames_to_samples(src: &[StereoFrame], dst: &mut [f32]) -> Result<(), AudioError> {
    if dst.len() < src.len() * 2 {
        return Err(AudioError::BufferMismatch);
    }

    for (i, frame) in src.iter().enumerate() {
        dst[i * 2] = frame.left;
        dst[i * 2 + 1] = frame.right;
    }

    Ok(())
}

// ============================================================================
// voice 面
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
