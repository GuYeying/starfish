//! 背景音乐播放器
//!
//! 占用引擎内部一条不可见的声道，开发者通过 `AudioEngine::music_*` 控制。
//! 支持：播放/暂停/停止、循环、淡出、跳转、排队切歌。

use std::{collections::VecDeque, sync::Arc};

use crate::base::subsystem::audio::common::StereoFrame;

use super::sfx::{AudioEffect, ChannelState, FadeState};
use super::SoundData;

/// 背景音乐播放器
pub struct MusicPlayer {
    pub data: Option<Arc<SoundData>>,
    pub state: ChannelState,
    pub cursor: usize,
    pub loops: i32,
    pub fade: Option<FadeState>,
    pub queue: VecDeque<Arc<SoundData>>,
    /// 效果器链（与 SfxChannel 共用 AudioEffect trait）
    pub effects: Vec<Box<dyn AudioEffect>>,
}

impl MusicPlayer {
    pub fn new() -> Self {
        Self {
            data: None,
            state: ChannelState::Stopped,
            cursor: 0,
            loops: 0,
            fade: None,
            queue: VecDeque::new(),
            effects: Vec::new(),
        }
    }

    /// 加载音频（替换当前数据，不自动播放）
    pub fn load(&mut self, data: Arc<SoundData>) {
        self.data = Some(data);
        self.cursor = 0;
        self.state = ChannelState::Stopped;
        self.fade = None;
    }

    /// 开始播放
    pub fn play(&mut self, loops: i32) {
        if self.data.is_some() {
            self.cursor = 0;
            self.loops = loops;
            self.state = ChannelState::Playing;
        }
    }

    /// 停止
    pub fn stop(&mut self) {
        self.state = ChannelState::Stopped;
        self.cursor = 0;
        self.fade = None;
    }

    /// 暂停
    pub fn pause(&mut self) {
        if self.state == ChannelState::Playing {
            self.state = ChannelState::Paused;
        }
    }

    /// 恢复
    pub fn resume(&mut self) {
        if self.state == ChannelState::Paused {
            self.state = ChannelState::Playing;
        }
    }

    /// 淡出停止
    pub fn fade_out(&mut self, ms: u32) {
        let sample_rate = self.data.as_ref().map_or(44100, |d| d.sample_rate);
        self.fade = Some(FadeState::new_fade_out(ms, sample_rate));
    }

    /// 淡入（从当前音量开始淡入到完整音量）
    ///
    /// `ms`: 淡入时长（毫秒）
    pub fn fade_in(&mut self, ms: u32) {
        let sample_rate = self.data.as_ref().map_or(44100, |d| d.sample_rate);
        self.fade = Some(FadeState::new_fade_in(ms, sample_rate));
    }

    /// 跳转到指定秒数
    pub fn seek(&mut self, seconds: f32) {
        if let Some(ref data) = self.data {
            let frame = (seconds * data.sample_rate as f32) as usize;
            self.cursor = frame.min(data.frames.len());
        }
    }

    /// 当前播放位置（秒）
    pub fn position(&self) -> f32 {
        self.data
            .as_ref()
            .map(|d| self.cursor as f32 / d.sample_rate as f32)
            .unwrap_or(0.0)
    }

    /// 总时长（秒）
    pub fn duration(&self) -> f32 {
        self.data
            .as_ref()
            .map(|d| d.frames.len() as f32 / d.sample_rate as f32)
            .unwrap_or(0.0)
    }

    /// 排队下一首
    pub fn queue(&mut self, data: Arc<SoundData>) {
        if self.data.is_none() {
            // 没有当前歌曲，直接加载
            self.load(data);
        } else {
            self.queue.push_back(data);
        }
    }

    /// 从头开始（不改变播放状态）
    pub fn rewind(&mut self) {
        self.cursor = 0;
    }

    /// 淡变增益
    pub fn fade_gain(&self) -> f32 {
        self.fade.as_ref().map_or(1.0, |f| f.gain())
    }

    /// 添加效果器
    pub fn add_effect(&mut self, effect: Box<dyn AudioEffect>) {
        self.effects.push(effect);
    }

    /// 读一段 PCM 帧
    ///
    /// 返回实际读取的帧数
    pub fn read_frames(&mut self, output: &mut [StereoFrame]) -> usize {
        let Some(ref data) = self.data else {
            return 0;
        };

        if self.cursor >= data.frames.len() {
            // 当前曲目已播完
            if let Some(next) = self.queue.pop_front() {
                self.data = Some(next);
                self.cursor = 0;
                self.loops = 0;
                // 递归读取新曲目
                return self.read_frames(output);
            } else if self.loops == -1 {
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

        // ── 效果器链（在淡变之前应用） ──
        for effect in &mut self.effects {
            effect.process(&mut output[..to_read]);
        }

        // ── 淡变 ──
        if let Some(fade) = &mut self.fade {
            let done = fade.advance(to_read);
            if done && fade.fade_type == crate::base::audio::sfx::FadeType::Out {
                self.state = ChannelState::Stopped;
                // 尝试播下一首
                if let Some(next) = self.queue.pop_front() {
                    self.load(next);
                    self.play(-1);
                }
                self.fade = None;
            } else if done {
                self.fade = None;
            }
        }

        to_read
    }
}
