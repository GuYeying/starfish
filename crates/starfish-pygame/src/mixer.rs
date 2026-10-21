//! pygame.mixer 对应物（thread-local 混音器生命周期 facade）
//!
//! 混音器是进程级单例语义：构造即开音频设备、Drop 即停——thread-local
//! 保活（Send 非 Sync，与渲染会话槽同格局）。init/quit/get_init 对齐
//! pygame 生命周期词汇；`with` 惰性建 8 通道（比 pygame 的 init 门槛
//! 友好）。Web 无解码线程，宿主帧循环每帧 [`pump`]（桌面 no-op——
//! 解码线程自驱）。
//!
//! **Web 自动播放策略 = 部署约定**（2026-10-18 定约，原运行时手势门
//! 已撤）：浏览器要求 AudioContext 的创建/恢复落在用户手势（点击/按键）
//! 调用栈内，否则 suspended 无声。本层**不做任何手势检测**——由宿主
//! 页面保证首次手势先行（嵌入页必然先点击；独立页用 html+js+css 的
//! "点击启动"覆盖层把 wasm 启动挂到 click 回调，pygame-rs 的
//! build_web.py 产物即此形态）。三平台因此行为一致：音频随时可播，
//! 无钩子、无跳过路径、无额外错误处理。

use std::cell::RefCell;
use std::sync::Arc;

use starfish_audio::common::AudioError;
use starfish_audio::{AudioMixer, SoundData};
use crate::effects::Effect;

/// 内嵌标准测试音（三平台零 IO；无参 Sound 构造的 Font(None) 惯例延续）
const BEEP: &[u8] = include_bytes!("../resources/audio/bleep.wav");

thread_local! {
    /// 混音器单例（Drop 即停设备 → 必须保活）
    static MIXER: RefCell<Option<AudioMixer>> = const { RefCell::new(None) };
}

// ── 生命周期 ─────────────────────────────────────────────────────

/// 内嵌标准提示音（无参 Sound 构造用；纯解码不开设备）
pub fn beep() -> Arc<SoundData> {
    // 已知合法内嵌字节，解码失败 = 构建期资源损坏
    Arc::new(SoundData::from_bytes(BEEP.to_vec()).expect("内嵌提示音解码失败"))
}

/// 显式（重建）初始化：`pygame.mixer.init(channels)`
pub fn init(channels: u32) -> Result<(), AudioError> {
    MIXER.with(|m| -> Result<(), AudioError> {
        *m.borrow_mut() = Some(AudioMixer::new(channels.max(1))?);
        Ok(())
    })
}

/// 退出：Drop 即停设备（`pygame.mixer.quit()`）
pub fn quit() {
    MIXER.with(|m| *m.borrow_mut() = None);
}

/// 是否已初始化（`pygame.mixer.get_init()` 的 bool 形态）
pub fn get_init() -> bool {
    MIXER.with(|m| m.borrow().is_some())
}

/// 访问混音器（未 init 时以默认 8 通道惰性创建）
pub fn with<R>(f: impl FnOnce(&mut AudioMixer) -> R) -> Result<R, AudioError> {
    MIXER.with(|m| {
        let mut slot = m.borrow_mut();
        if slot.is_none() {
            *slot = Some(AudioMixer::new(8)?);
        }
        Ok(f(slot.as_mut().expect("刚初始化")))
    })
}

/// Web 每帧驱动流式解码（桌面 no-op——解码线程自驱）；宿主帧循环调用
pub fn pump() {
    #[cfg(target_arch = "wasm32")]
    MIXER.with(|m| {
        if let Some(mixer) = m.borrow_mut().as_mut() {
            mixer.pump_streams(2048);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beep_decodes() {
        let snd = beep();
        assert!(snd.duration() > 0.1, "内嵌提示音时长为正");
    }

    #[test]
    fn sound_from_bytes_length() {
        let s = Sound::from_bytes(BEEP.to_vec()).unwrap();
        assert!(s.get_length() > 0.1);
    }




}
// ── Sound：pygame.mixer.Sound 对应物 ─────────────────────────────

/// 可播放的音频数据（`Arc<SoundData>` 共享；Clone 便宜）。
/// sndarray 桥的落点（`from_data`）。
#[derive(Clone)]
pub struct Sound {
    pub(crate) data: Arc<SoundData>,
}

impl Sound {
    /// Sound()：内嵌提示音（Font(None) 惯例延续）
    pub fn new() -> Self {
        Self { data: beep() }
    }

    /// 内存解码（WAV/OGG/MP3/FLAC 按内容探测）
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, AudioError> {
        Ok(Self {
            data: Arc::new(SoundData::from_bytes(bytes)?),
        })
    }

    /// sndarray 桥：采样数据 → Sound
    pub(crate) fn from_data(data: Arc<SoundData>) -> Self {
        Self { data }
    }

    /// 数据句柄（sndarray get_array 用）
    pub(crate) fn data(&self) -> &Arc<SoundData> {
        &self.data
    }

    /// get_length() -> float：时长（秒）
    pub fn get_length(&self) -> f32 {
        self.data.duration()
    }

    /// play(loops=0) -> Channel：找空闲通道播放（无空闲 = Err）
    pub fn play(&self, loops: i32) -> Result<Channel, AudioError> {
        with(|m| {
            let idx = m
                .play_with(self.data.clone(), loops)?
                .ok_or_else(|| AudioError::custom("没有空闲通道"))?;
            Ok(Channel { idx })
        })
        .and_then(|r| r)
    }

    /// play_on(channel, loops=0)：指定通道播放
    pub fn play_on(&self, channel: usize, loops: i32) -> Result<(), AudioError> {
        with(|m| m.play_on(self.data.clone(), channel)).and_then(|r| r)
    }
}

// ── Channel：pygame.mixer.Channel 对应物 ─────────────────────────

/// 通道句柄（idx 薄包装；Clone/Copy——通道数 init 时定格）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Channel {
    pub(crate) idx: usize,
}

impl Channel {
    /// 按索引取通道句柄
    pub fn new(idx: usize) -> Self {
        Self { idx }
    }

    /// 通道索引
    pub fn idx(&self) -> usize {
        self.idx
    }

    /// play(sound, loops=0)：在通道上播放
    pub fn play(&self, sound: &Sound, loops: i32) -> Result<(), AudioError> {
        with(|m| m.play_on(sound.data.clone(), self.idx)).and_then(|r| r)
    }

    /// stop()：停止本通道（含清特效通知）
    pub fn stop(&self) {
        with(|m| m.stop(self.idx)).ok();
    }

    /// pause()：暂停
    pub fn pause(&self) {
        with(|m| m.pause(self.idx)).ok();
    }

    /// resume()：恢复
    pub fn resume(&self) {
        with(|m| m.resume(self.idx)).ok();
    }

    /// set_volume(v)：通道音量（0.0..1.0）
    pub fn set_volume(&self, v: f32) {
        with(|m| m.set_channel_volume(self.idx, v)).ok();
    }

    /// fade_in(sound, ms)：淡入播放
    pub fn fade_in(&self, sound: &Sound, ms: u32) -> Result<(), AudioError> {
        with(|m| {
            m.play_on(sound.data.clone(), self.idx)?;
            m.fade_in(self.idx, ms);
            Ok(())
        })
        .and_then(|r| r)
    }

    /// fade_out(ms)：淡出停止
    pub fn fade_out(&self, ms: u32) {
        with(|m| m.fade_out(self.idx, ms)).ok();
    }

    /// get_busy() -> bool：是否在播
    pub fn get_busy(&self) -> bool {
        with(|m| m.is_channel_busy(self.idx)).unwrap_or(false)
    }

    /// get_sound() -> Option<Sound>：通道当前音源
    pub fn get_sound(&self) -> Option<Sound> {
        with(|m| m.get_channel_sound(self.idx))
            .ok()
            .flatten()
            .map(Sound::from_data)
    }

    /// add_effect(effect)：挂特效（串联在数据通路：read_frames → effects → mix）
    pub fn add_effect(&self, effect: Effect) {
        with(|m| m.sfx_add_effect(self.idx, effect.boxed())).ok();
    }
}

// ── 混音器主控面 ────────────────────────────────────────────────

/// 通道数（init(channels) 时定格）
pub fn num_channels() -> usize {
    with(|m| m.num_channels()).unwrap_or(0)
}

/// 保留通道（0..n 不参与 Sound.play 的自动分配）
pub fn set_reserved(n: usize) {
    with(|m| m.set_reserved(n)).ok();
}

/// 全局是否在播（任一通道活跃）
pub fn get_busy() -> bool {
    with(|m| m.get_busy()).unwrap_or(false)
}

/// 主音量
pub fn set_master_volume(v: f32) {
    with(|m| m.set_master_volume(v)).ok();
}

/// 设备采样率（特效构造参考；未 init = 48000 垫底）
pub fn output_sample_rate() -> u32 {
    with(|m| m.output_sample_rate()).unwrap_or(48000)
}

