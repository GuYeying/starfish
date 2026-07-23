use std::fmt;



// common.rs
pub const AUDIO_STACK_FRAME_BUF: usize = 1024;
pub const AUDIO_SAMPLE_PER_FRAME: usize = 2;
pub const STACK_SAMPLE_CAP: usize = AUDIO_STACK_FRAME_BUF * AUDIO_SAMPLE_PER_FRAME;

/// 引擎统一立体声帧（全平台/播放/录音标准格式 f32 [-1.0, 1.0]）
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



/// 音频通用错误
/// 音频通用错误（全模块唯一错误类型，统一替换两套旧Error）
#[derive(Debug)]
pub enum AudioError {
    Sdl(sdl3::Error),
    BufferMismatch,
    UnsupportedFormat,
    NullSubsystem,
    Custom(String), // 新增：自定义文本错误，替代旧 Error 结构体
}

// 构造快捷方法
impl AudioError {
    pub fn custom(msg: impl Into<String>) -> Self {
        Self::Custom(msg.into())
    }
}

impl From<sdl3::Error> for AudioError {
    fn from(e: sdl3::Error) -> Self {
        Self::Sdl(e)
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Sdl(e) => write!(f, "SDL audio error: {e}"),
            AudioError::BufferMismatch => write!(f, "Audio buffer size mismatch"),
            AudioError::UnsupportedFormat => write!(f, "Unsupported audio format"),
            AudioError::NullSubsystem => write!(f, "AudioSubsystem pointer is null"),
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
        dst[i].left = src[i*2].clamp(-1.0, 1.0);
        dst[i].right = src[i*2+1].clamp(-1.0, 1.0);
    }
    Ok(())
}


#[inline(always)]
pub fn frames_to_samples(
    src: &[StereoFrame],
    dst: &mut [f32],
) -> Result<(), AudioError> {
    if dst.len() < src.len() * 2 {
        return Err(AudioError::BufferMismatch);
    }

    for (i, frame) in src.iter().enumerate() {
        dst[i * 2] = frame.left;
        dst[i * 2 + 1] = frame.right;
    }

    Ok(())
}