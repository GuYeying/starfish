//! 流式音频播放核心
//!
//! [`MusicStream`] 是长音频（BGM / 环境音 / 语音）的流式容器：内部拥有一个
//! 解码线程与一个 SPSC 环形缓冲，边解码边供 MusicPlayer 在音频线程上读取。
//! 与全量解码的 [`SoundData`](crate::base::audio::SoundData) 相对，
//! 二者构成短/长音频的对称容器：
//!
//! | | [`SoundData`](crate::base::audio::SoundData) | [`MusicStream`] |
//! |---|---|---|
//! | 本质 | 纯数据（死的） | 活资源（拥有解码线程） |
//! | Clone/共享 | 可以（`Arc` 分发） | 不可，所有权唯一 |
//! | 适用 | 短音效 | 长音频 |
//! | 对位 | `pygame.mixer.Sound` | `pygame.mixer.music` |
//!
//! 线程规则：
//! 1. spawn 只发生在 [`MusicStream::from_file`]（控制面）
//! 2. `JoinHandle` 随 `MusicStream` 走，join 只发生在控制线程
//!    （MusicPlayer 把退役句柄收集到 `retired`，AudioMixer 锁外统一 join）
//! 3. 音频回调线程只见环上的两个原子指针，从不接触线程

mod worker;

use std::sync::Arc;
use std::thread::JoinHandle;

use crate::base::subsystem::audio::common::AudioError;
pub(crate) use worker::{Cmd, CmdSlot};

use super::ring::SharedRing;

/// 流式音频容器（长音频，对位 `pygame.mixer.music`）
///
/// # 示例
///
/// ```ignore
/// let stream = MusicStream::from_file("bgm.ogg", mixer.output_sample_rate)?;
/// mixer.music_load(stream);            // 所有权交给混音器
/// // 或一步到位：mixer.music_load_file("bgm.ogg")?;
/// ```
pub struct MusicStream {
    /// 音频线程侧的读取端
    pub(crate) reader: StreamReader,
    /// 控制命令槽（seek / stop）
    pub(crate) cmd: Arc<CmdSlot>,
    /// 解码线程句柄（join 只发生在控制线程）
    pub(crate) worker: Option<JoinHandle<()>>,
    /// 输出采样率（解码线程已重采样到该值）
    pub(crate) sample_rate: u32,
    /// 容器声明的总时长（秒）
    pub(crate) duration: Option<f32>,
}

impl MusicStream {
    /// 打开音频文件并启动解码线程
    ///
    /// `output_sample_rate`：目标混音输出采样率
    /// （见 [`AudioMixer::output_sample_rate`](crate::base::audio::AudioMixer)）。
    /// 源采样率不同会在解码线程侧完成重采样。
    pub fn from_file(path: &str, output_sample_rate: u32) -> Result<Self, AudioError> {
        let sym_reader = crate::base::audio::decoder::SymphoniaReader::open(path)?;
        let duration = sym_reader.duration();
        let ring = Arc::new(SharedRing::with_capacity(output_sample_rate as usize * 2));
        let cmd = Arc::new(CmdSlot::default());
        let worker = worker::spawn_worker(sym_reader, ring.clone(), cmd.clone(), output_sample_rate);
        Ok(Self {
            reader: StreamReader { ring },
            cmd,
            worker: Some(worker),
            sample_rate: output_sample_rate,
            duration,
        })
    }

    /// 流的总时长（秒）；容器未提供元数据时为 None
    pub fn duration(&self) -> Option<f32> {
        self.duration
    }

    /// 流的采样率（即混音输出采样率，解码线程已完成重采样）
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 通知解码线程退出（不 join；join 由控制面稍后完成）
    pub(crate) fn request_stop(&self) {
        self.cmd.send(Cmd::Stop);
    }

    /// 控制面：收走 JoinHandle，调用方须在**控制线程、无锁状态**下 join
    pub(crate) fn take_worker(&mut self) -> Option<JoinHandle<()>> {
        self.worker.take()
    }
}

impl Drop for MusicStream {
    fn drop(&mut self) {
        // 只发停止信号，不在此 join：drop 可能发生在持有 Inner 锁的路径上。
        // worker 收到 Stop 后数毫秒内自行退出；此后即使句柄被丢弃（detach）
        // 也只是回收一个即将结束的线程，无泄漏。
        self.request_stop();
    }
}

/// 音频线程侧的流读取端（仅持环句柄，无锁）
pub(crate) struct StreamReader {
    pub(crate) ring: Arc<SharedRing>,
}

impl StreamReader {
    /// 读至多 `out.len()` 帧（不阻塞、不分配）
    ///
    /// 返回 0 且 [`finished`](Self::finished) 为假 = 暂时欠载（解码未跟上）。
    pub(crate) fn read_frames(&self, out: &mut [crate::base::subsystem::audio::common::StereoFrame]) -> usize {
        self.ring.read(out)
    }

    /// 整条流是否已播完（EOF 且缓冲已空）
    pub(crate) fn finished(&self) -> bool {
        self.ring.finished()
    }
}
