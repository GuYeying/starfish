//! 流式声部：推式 PCM 声源（mixer 三类声源中的"外部推"模型）
//!
//! 与既有两类的分工：SFX 声部（`play_with`，一次性全量缓冲，隐式回收）、
//! music（文件驱动流，mixer 自带解码线程）、流式声部（本模块，**调用方
//! 推帧**）——典型客户：视频音轨、程序化合成、网络音频流。
//!
//! 三段式生命周期：`push_interleaved` 推帧（环形缓冲满 = 背压少收，推方
//! 节奏被消费端拽住——这正是 A/V 同步的天然锚点）→ `set_volume` /
//! `set_muted` / `fade_in` / `fade_out_and_close` → `close` 释放。
//! 推帧按设备采样率（`AudioMixer::output_sample_rate`），v1 不做重采样。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::base::audio::common::{FadeState, FadeType, StereoFrame};
use crate::base::audio::ring::SharedRing;

/// 混音回调侧的槽位（推方持 [`StreamVoice`] 句柄共享同一环；字段对
/// audio 模块可见——混音回调直接读环/控制面）
pub(crate) struct StreamVoiceSlot {
    pub(crate) ring: SharedRing,
    /// 混音域采样率（推帧契约：`push_interleaved` 按此采样率）
    pub(crate) sample_rate: u32,
    pub(crate) volume: Mutex<f32>,
    pub(crate) muted: AtomicBool,
    pub(crate) fade: Mutex<Option<FadeState>>,
    /// 关闭标记：fade_out 走完或显式 `close` 后置位，混音时惰性摘除
    pub(crate) closed: AtomicBool,
    /// 累计已接受的帧数（诊断/测试判读：推泵活性锚点）
    pub(crate) pushed_frames: AtomicU64,
}

impl StreamVoiceSlot {
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            // 16384 帧 ≈ 340ms @48kHz：够吸收推方抖动，延迟仍可控
            ring: SharedRing::with_capacity(16384),
            sample_rate,
            volume: Mutex::new(1.0),
            muted: AtomicBool::new(false),
            fade: Mutex::new(None),
            closed: AtomicBool::new(false),
            pushed_frames: AtomicU64::new(0),
        }
    }
}

/// 流式声部句柄（Clone 共享同一声部）
#[derive(Clone)]
pub struct StreamVoice {
    pub(crate) slot: Arc<StreamVoiceSlot>,
}

impl StreamVoice {
    /// 声部采样率（混音域 = 设备真实采样率）
    ///
    /// 推帧契约：`push_interleaved` 的采样按此采样率解释；源采样率不同时
    /// 由推方先行重采样（见 `audio::resample::StreamResampler`）。
    pub fn sample_rate(&self) -> u32 {
        self.slot.sample_rate
    }

    /// 推入交错立体声帧（left, right, left, ...），返回实际接受的帧数
    ///
    /// 环形缓冲满时少收（**背压**）：推方按返回值保留未收部分稍后再推，
    /// 消费节奏由此反压解码节奏——不要丢弃，丢样会爆音。
    pub fn push_interleaved(&mut self, samples: &[f32]) -> usize {
        let frames = samples.len() / 2;
        let mut buf = Vec::with_capacity(frames);
        for chunk in samples.chunks_exact(2) {
            buf.push(StereoFrame {
                left: chunk[0],
                right: chunk[1],
            });
        }
        let written = self.slot.ring.write(&buf);
        self.slot.pushed_frames.fetch_add(written as u64, Ordering::Relaxed);
        written
    }

    /// 累计已接受的帧数（诊断/测试判读：推泵活性锚点）
    pub fn pushed_frames(&self) -> u64 {
        self.slot.pushed_frames.load(Ordering::Relaxed)
    }

    /// 声部音量（0.0 ~ 1.0；与 SFX/music 的增益语义一致）
    pub fn set_volume(&self, volume: f32) {
        *self.slot.volume.lock().unwrap() = volume.clamp(0.0, 1.0);
    }

    /// 静音开关（与音量独立，混音时直接跳过该声部）
    pub fn set_muted(&self, muted: bool) {
        self.slot.muted.store(muted, Ordering::Relaxed);
    }

    /// 对当前内容淡入（运行时随时可调，不必在打开瞬间）
    pub fn fade_in(&self, ms: u32) {
        *self.slot.fade.lock().unwrap() =
            Some(FadeState::new_fade_in(ms, self.slot.sample_rate));
    }

    /// 淡出，走完后自动关闭声部（视频 ended 收尾的标准打法，防爆音）
    pub fn fade_out_and_close(&self, ms: u32) {
        let mut fade = self.slot.fade.lock().unwrap();
        // 已在淡出中不重置（避免永远淡不完）
        if !matches!(fade.as_ref().map(|f| f.fade_type), Some(FadeType::Out)) {
            *fade = Some(FadeState::new_fade_out(ms, self.slot.sample_rate));
        }
    }

    /// 立即关闭声部（环形缓冲内残留帧丢弃）
    pub fn close(&self) {
        self.slot.closed.store(true, Ordering::Relaxed);
    }
}
