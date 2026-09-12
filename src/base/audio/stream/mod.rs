//! 流式音频播放核心
//!
//! [`MusicStream`] 是长音频（BGM / 环境音 / 语音）的流式容器：内部拥有
//! SPSC 环形缓冲与解码驱动，边解码边供 MusicPlayer 读取。
//! 与全量解码的 [`SoundData`](crate::base::audio::SoundData) 相对，
//! 二者构成短/长音频的对称容器：
//!
//! | | [`SoundData`](crate::base::audio::SoundData) | [`MusicStream`] |
//! |---|---|---|
//! | 本质 | 纯数据（死的） | 活资源（拥有解码驱动） |
//! | Clone/共享 | 可以（`Arc` 分发） | 不可，所有权唯一 |
//! | 适用 | 短音效 | 长音频 |
//! | 对位 | `pygame.mixer.Sound` | `pygame.mixer.music` |
//!
//! 驱动规则：
//! 1. native：解码后台线程自动驱动（灌满即让位睡眠），join 只发生在控制线程
//! 2. Web（无线程）：游戏循环每帧调用 [`AudioMixer::pump_streams`](crate::base::audio::AudioMixer::pump_streams)
//!    预算式推进解码
//! 3. 音频回调线程只见环上的两个原子指针，从不接触线程

mod worker;

use std::sync::Arc;

use crate::base::audio::common::AudioError;
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
    /// 解码线程句柄（native；join 只发生在控制线程；Web 上恒为 None）
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) worker: Option<std::thread::JoinHandle<()>>,
    /// 预算式解码泵（仅 Web 编译目标存在；native 由线程自动驱动）
    #[cfg(target_arch = "wasm32")]
    pub(crate) pump: Option<worker::DecoderPump>,
    /// 输出采样率（解码驱动已重采样到该值）
    pub(crate) sample_rate: u32,
    /// 容器声明的总时长（秒）
    pub(crate) duration: Option<f32>,
}

impl MusicStream {
    /// 打开音频文件并启动解码
    ///
    /// `output_sample_rate`：目标混音输出采样率
    /// （见 [`AudioMixer::output_sample_rate`](crate::base::audio::AudioMixer)）。
    /// 源采样率不同会在解码侧完成重采样。
    pub fn from_file(path: &str, output_sample_rate: u32) -> Result<Self, AudioError> {
        let sym_reader = crate::base::audio::decoder::SymphoniaReader::open(path)?;
        Ok(Self::from_reader(sym_reader, output_sample_rate))
    }

    /// 从内存字节打开（Web 友好：字节由消费端获取，如 fetch 后喂入）
    pub fn from_bytes(data: Vec<u8>, output_sample_rate: u32) -> Result<Self, AudioError> {
        let sym_reader = crate::base::audio::decoder::SymphoniaReader::from_bytes(data)?;
        Ok(Self::from_reader(sym_reader, output_sample_rate))
    }

    fn from_reader(
        sym_reader: crate::base::audio::decoder::SymphoniaReader,
        output_sample_rate: u32,
    ) -> Self {
        let duration = sym_reader.duration();
        let ring = Arc::new(SharedRing::with_capacity(output_sample_rate as usize * 2));
        let cmd = Arc::new(CmdSlot::default());

        #[cfg(not(target_arch = "wasm32"))]
        let worker = Some(worker::spawn_decoder_thread(
            worker::DecoderPump::new(sym_reader, ring.clone(), cmd.clone(), output_sample_rate),
        ));
        #[cfg(target_arch = "wasm32")]
        let pump = Some(worker::DecoderPump::new(
            sym_reader,
            ring.clone(),
            cmd.clone(),
            output_sample_rate,
        ));

        Self {
            reader: StreamReader { ring },
            cmd,
            #[cfg(not(target_arch = "wasm32"))]
            worker,
            #[cfg(target_arch = "wasm32")]
            pump,
            sample_rate: output_sample_rate,
            duration,
        }
    }

    /// 流的总时长（秒）；容器未提供元数据时为 None
    pub fn duration(&self) -> Option<f32> {
        self.duration
    }

    /// 流的采样率（即混音输出采样率，解码已完成重采样）
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// 通知解码退出（不 join；join 由控制面稍后完成）
    pub(crate) fn request_stop(&self) {
        self.cmd.send(Cmd::Stop);
    }

    /// 每帧推进解码（预算：帧数），返回实际解码帧数
    ///
    /// native（有线程）：无操作——后台线程自动维持缓冲；
    /// Web（无线程）：由游戏循环每帧调用，预算式推进解码。
    pub(crate) fn pump(&mut self, budget_frames: usize) -> usize {
        #[cfg(target_arch = "wasm32")]
        if let Some(p) = &mut self.pump {
            return p.pump_budget(budget_frames);
        }
        0
    }

    /// 控制面：收走 JoinHandle，调用方须在**控制线程、无锁状态**下 join
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn take_worker(&mut self) -> Option<std::thread::JoinHandle<()>> {
        self.worker.take()
    }
}

impl Drop for MusicStream {
    fn drop(&mut self) {
        // 只发停止信号，不在此 join：drop 可能发生在持有 Inner 锁的路径上。
        // 解码驱动收到 Stop 后数毫秒内自行退出；此后即使句柄被丢弃（detach）
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
    pub(crate) fn read_frames(
        &self,
        out: &mut [crate::base::audio::common::StereoFrame],
    ) -> usize {
        self.ring.read(out)
    }

    /// 整条流是否已播完（EOF 且缓冲已空）
    pub(crate) fn finished(&self) -> bool {
        self.ring.finished()
    }
}
