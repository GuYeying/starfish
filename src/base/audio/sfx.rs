//! 短音效（SFX）声道
//!
//! 每个 SfxChannel 对应一条可独立控制的播放轨道。
//! 承载已解码的 PCM 数据、播放状态、音量、声像、淡变、效果器链。
//!
//! 声道状态 / 淡变 / 效果器等与"数据来源无关"的叶子类型在 [`super::common`]。

use std::sync::Arc;

use crate::base::audio::GroupHandle;
use crate::base::audio::common::StereoFrame;

use super::SoundData;

// 保持旧路径可用（这些叶子类型现居住在 common.rs）
pub use super::common::{AudioEffect, ChannelState, FadeState, FadeType};

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

        if self.cursor >= data.frame_count() {
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

        // 单声道源在此展开为 L=R（SoundData::read_into 内部分派）
        let to_read = data.read_into(self.cursor, output);
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
