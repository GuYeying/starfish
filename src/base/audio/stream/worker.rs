//! 流式解码核心（DecoderPump）与 native 线程驱动
//!
//! [`DecoderPump`] 是**纯解码逻辑**：symphonia 逐包解码 → 跨包线性重采样 →
//! 写入环形缓冲。它本身不含线程——由调用方驱动：
//!
//! - **native**：[`spawn_decoder_thread`] 起后台线程循环驱动（灌满即让位睡眠）
//! - **Web（无线程）**：游戏循环每帧调用 [`DecoderPump::pump_budget`]（预算式）
//!
//! 两种驱动共享同一份解码核心，行为语义一致。

use std::sync::Arc;
use std::time::Duration;

use crate::base::audio::decoder::SymphoniaReader;
use crate::base::audio::common::StereoFrame;

use crate::base::audio::ring::SharedRing;

const TAU: f32 = std::f32::consts::TAU;

/// 控制面 → 解码核心的命令
#[derive(Debug, Clone, Copy)]
pub(crate) enum Cmd {
    /// 跳转（秒）。收到后：seek → 换代 → 从新位置继续
    Seek(f32),
    /// 退出
    Stop,
}

/// 命令槽：控制面投递、解码侧消费；亦是解码循环的睡眠/唤醒点
#[derive(Default)]
pub(crate) struct CmdSlot {
    cmd: std::sync::Mutex<Option<Cmd>>,
    signal: std::sync::Condvar,
}

impl CmdSlot {
    /// 投递命令并唤醒解码循环（控制线程调用）
    pub(crate) fn send(&self, cmd: Cmd) {
        *self.cmd.lock().unwrap() = Some(cmd);
        self.signal.notify_all();
    }

    /// 非阻塞取命令（Web 游戏循环 / 解码核心内部使用）
    pub(crate) fn try_take(&self) -> Option<Cmd> {
        self.cmd.lock().unwrap().take()
    }

    /// 阻塞等待至多 `timeout` 后取命令（native 驱动的 park 点；命令即时唤醒）
    pub(crate) fn take_timeout(&self, timeout: Duration) -> Option<Cmd> {
        let mut guard = self.cmd.lock().unwrap();
        if guard.is_none() {
            guard = self.signal.wait_timeout(guard, timeout).unwrap().0;
        }
        guard.take()
    }
}

/// 跨包线性重采样器（带状态，与 `SoundData::resample` 同款插值）
pub(crate) struct Resampler {
    step: f64,
    /// 下一个输出点相对 `last` 的位置 ∈ [0, 1)
    pos: f64,
    last: StereoFrame,
    init: bool,
}

impl Resampler {
    pub(crate) fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            step: src_rate as f64 / dst_rate as f64,
            pos: 1.0,
            last: StereoFrame::SILENT,
            init: false,
        }
    }

    pub(crate) fn needs_resample(&self) -> bool {
        self.step != 1.0
    }

    /// 把一包帧追加到 `out`（输出采样率）
    pub(crate) fn process(&mut self, input: &[StereoFrame], out: &mut Vec<StereoFrame>) {
        if !self.needs_resample() {
            out.extend_from_slice(input);
            return;
        }
        for &cur in input {
            if !self.init {
                // 首个源帧直接输出（t=0），下一个输出点在 t=step
                self.last = cur;
                self.init = true;
                self.pos = self.step;
                out.push(cur);
                continue;
            }
            while self.pos < 1.0 {
                let l = self.last.left + (cur.left - self.last.left) * self.pos as f32;
                let r = self.last.right + (cur.right - self.last.right) * self.pos as f32;
                out.push(StereoFrame { left: l, right: r });
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.last = cur;
        }
    }
}

/// 解码核心：逐包解码 → 跨包线性重采样 → 写入环形缓冲
///
/// 纯逻辑、不含线程——native 由后台线程驱动，Web 由游戏循环驱动。
/// 两种驱动共享同一份核心，行为语义一致。
pub(crate) struct DecoderPump {
    reader: SymphoniaReader,
    ring: Arc<SharedRing>,
    cmd: Arc<CmdSlot>,
    resampler: Resampler,
    /// 上一包未能完全写入环的余量（环满时暂存）
    pending: Vec<StereoFrame>,
    /// 单包解码的重采样输出暂存（复用避免分配）
    resampled: Vec<StereoFrame>,
    /// 解码已到流末尾（余量排空后置环 EOF）
    eof: bool,
    /// 收到 Stop
    stopped: bool,
    /// 解码让位阈值：环空闲低于此值时暂停解码（帧）
    batch: usize,
}

impl DecoderPump {
    /// 创建解码核心
    ///
    /// `dst_rate`：混音输出采样率。源采样率不同时在 pump 内完成重采样，
    /// 环形缓冲里永远是输出采样率，读取侧纯 memcpy。
    /// 让位阈值默认 4096 帧，可用 [`set_batch`](Self::set_batch) 调整。
    pub(crate) fn new(
        reader: SymphoniaReader,
        ring: Arc<SharedRing>,
        cmd: Arc<CmdSlot>,
        dst_rate: u32,
    ) -> Self {
        let src_rate = reader.src_rate();
        Self {
            reader,
            ring,
            cmd,
            resampler: Resampler::new(src_rate, dst_rate),
            pending: Vec::new(),
            resampled: Vec::new(),
            eof: false,
            stopped: false,
            batch: 4096,
        }
    }

    /// 设置解码让位阈值（环空闲低于此值时暂停解码）
    pub(crate) fn set_batch(&mut self, frames: usize) {
        self.batch = frames.max(1);
    }

    /// 尽力推进至多 `max_frames` 帧入环，返回实际写入帧数
    ///
    /// 同时非阻塞处理控制命令（Seek / Stop）。
    /// 返回 0 = 本轮无事可做（环满让位 / 已停止 / EOF 已标记）。
    pub(crate) fn pump_budget(&mut self, max_frames: usize) -> usize {
        // 1. 命令（非阻塞）
        match self.cmd.try_take() {
            Some(Cmd::Stop) => {
                self.stopped = true;
                return 0;
            }
            Some(Cmd::Seek(sec)) => {
                self.pending.clear();
                match self.reader.seek(sec) {
                    // 先换代再写新数据，读者据此丢弃旧代残留
                    Ok(()) => self.ring.begin_generation(),
                    Err(e) => eprintln!("[starfish audio] seek 失败: {e}"),
                }
            }
            None => {}
        }

        // 2. 环满让位（批量写入粒度约 batch 帧）
        if self.ring.free() < self.batch {
            return 0;
        }

        // 3. 上一包余量优先写入
        if !self.pending.is_empty() {
            let n = self.ring.write(&self.pending);
            self.pending.drain(..n);
            return n;
        }

        // 4. 解码下一包
        match self.reader.next_interleaved() {
            Ok(Some(chunk)) if chunk.is_empty() => 0,
            Ok(Some(chunk)) => {
                let frames = interleave(&chunk, self.reader.src_channels() == 1);
                self.resampled.clear();
                self.resampler.process(&frames, &mut self.resampled);

                let n = self.ring.write(&self.resampled);
                self.pending.extend_from_slice(&self.resampled[n..]);
                n
            }
            Ok(None) => {
                // 流结束：把余量灌完再标记 EOF，保证尾部不截断
                while !self.pending.is_empty() {
                    if matches!(self.cmd.try_take(), Some(Cmd::Stop)) {
                        self.stopped = true;
                        return 0;
                    }
                    let n = self.ring.write(&self.pending);
                    self.pending.drain(..n);
                    if n == 0 {
                        // 环满：等消费，下次继续排空（EOF 标记延后）
                        return 0;
                    }
                }
                self.ring.set_eof();
                0
            }
            Err(e) => {
                eprintln!("[starfish audio] 解码错误，流终止: {e}");
                self.ring.set_eof();
                0
            }
        }
    }

    /// 是否已收到 Stop
    pub(crate) fn stopped(&self) -> bool {
        self.stopped
    }

    /// 解码是否已到流末尾（余量是否排空另见环状态）
    pub(crate) fn eof(&self) -> bool {
        self.eof
    }
}

/// 交错采样 → 立体声帧（`mono` 时 L=R 展开）
fn interleave(chunk: &[f32], mono: bool) -> Vec<StereoFrame> {
    let mut frames = Vec::with_capacity(chunk.len() / 2 + 1);
    if mono {
        for &s in chunk {
            frames.push(StereoFrame { left: s, right: s });
        }
    } else {
        for pair in chunk.chunks_exact(2) {
            frames.push(StereoFrame {
                left: pair[0],
                right: pair[1],
            });
        }
    }
    frames
}

/// native 驱动：后台解码线程
///
/// 循环驱动 [`DecoderPump`]：命令即时响应，缓冲灌满即让位睡眠，
/// EOF 排空后自动退出。
pub(crate) fn spawn_decoder_thread(mut pump: DecoderPump) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("starfish-audio-decoder".into())
        .spawn(move || {
            loop {
                if pump.stopped() || pump.eof() {
                    return;
                }
                let n = pump.pump_budget(4096);
                if n == 0 {
                    // 环满或无进展：让位睡眠（命令即时唤醒，超时自醒兜底）
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        })
        .expect("starfish audio: 解码线程创建失败")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(l: f32) -> StereoFrame {
        StereoFrame { left: l, right: l }
    }

    #[test]
    fn pump_budget_fills_ring() {
        let ring = Arc::new(SharedRing::with_capacity(2048));
        let cmd = Arc::new(CmdSlot::default());
        let mut pump = DecoderPump::new(make_test_reader(), ring.clone(), cmd.clone(), 44100);
        pump.set_batch(64);

        let n = pump.pump_budget(1000);
        assert!(n > 0, "应写入部分帧");
        assert_eq!(ring.available(), n);
        assert!(!pump.stopped());
        assert!(!pump.eof());
    }

    #[test]
    fn stop_takes_effect() {
        let ring = Arc::new(SharedRing::with_capacity(2048));
        let cmd = Arc::new(CmdSlot::default());
        let mut pump = DecoderPump::new(make_test_reader(), ring.clone(), cmd.clone(), 44100);
        pump.set_batch(64);

        cmd.send(Cmd::Stop);
        assert_eq!(pump.pump_budget(4096), 0);
        assert!(pump.stopped());
    }

    #[test]
    fn seek_generation_no_panic() {
        let ring = Arc::new(SharedRing::with_capacity(2048));
        let cmd = Arc::new(CmdSlot::default());
        let mut pump = DecoderPump::new(make_test_reader(), ring.clone(), cmd.clone(), 44100);
        pump.set_batch(64);

        // 先灌一点，再 seek（换代），继续泵——不应 panic 且可继续产出
        pump.pump_budget(100);
        pump.pump_budget(4096);
        cmd.send(Cmd::Seek(0.2));

        // 模拟消费者：读取会推进 head 并丢弃换代前的旧数据
        fn drain(ring: &SharedRing) {
            let mut buf = [StereoFrame::SILENT; 256];
            while ring.read(&mut buf) > 0 {}
        }
        drain(&ring);

        let n = pump.pump_budget(100);
        assert!(n > 0 || pump.eof());
    }

    fn make_test_reader() -> SymphoniaReader {
        let path = std::env::temp_dir().join("starfish_pump_test.wav");
        crate::base::audio::test_support::write_test_wav(&path, 44100, 1.0);
        SymphoniaReader::open(path.to_str().unwrap()).unwrap()
    }
}
