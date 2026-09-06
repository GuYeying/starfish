//! 流式解码线程
//!
//! 生产者：symphonia 逐包解码 →（按需）线性重采样到混音输出率 → 写入环形缓冲。
//!
//! 节奏模型是"尽力提前灌满，灌满了就睡"：缓冲有充足空位时连续解码；
//! 空位不足时在命令槽的 condvar 上小睡（控制命令即时唤醒，无命令则
//! 超时自醒后重查空位）。解码到文件尾后清空余量、置 EOF、线程退出。

use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::base::subsystem::audio::common::StereoFrame;

use crate::base::audio::ring::SharedRing;

/// 控制面 → 解码线程的命令
pub(crate) enum Cmd {
    /// 跳转（秒）。worker 收到后：symphonia seek → 换代 → 从新位置继续
    Seek(f32),
    /// 退出线程
    Stop,
}

/// 命令槽：控制面投递、worker 消费，兼作 worker 的睡眠/唤醒点
#[derive(Default)]
pub(crate) struct CmdSlot {
    cmd: Mutex<Option<Cmd>>,
    signal: Condvar,
}

impl CmdSlot {
    /// 投递命令并唤醒 worker（控制线程调用，绝不阻塞于解码进度）
    pub(crate) fn send(&self, cmd: Cmd) {
        *self.cmd.lock().unwrap() = Some(cmd);
        self.signal.notify_all();
    }

    /// worker：取命令；无命令时等待至多 `timeout`（兜底轮询缓冲空位）
    fn take(&self, timeout: Duration) -> Option<Cmd> {
        let mut guard = self.cmd.lock().unwrap();
        if guard.is_none() {
            // 无命令：睡眠（控制面 notify 即醒，否则超时自醒）
            guard = self.signal.wait_timeout(guard, timeout).unwrap().0;
        }
        guard.take()
    }
}

/// 跨包线性重采样器（与 `SoundData::resample` 同款线性插值，带状态）
struct Resampler {
    step: f64,
    /// 下一个输出点相对 `last` 的位置 ∈ [0, 1)
    pos: f64,
    last: StereoFrame,
    init: bool,
}

impl Resampler {
    fn new(src_rate: u32, dst_rate: u32) -> Self {
        Self {
            step: src_rate as f64 / dst_rate as f64,
            pos: 1.0,
            last: StereoFrame::SILENT,
            init: false,
        }
    }

    fn needs_resample(&self) -> bool {
        self.step != 1.0
    }

    /// 把一个包的帧追加到 `out`（输出采样率）
    fn process(&mut self, input: &[StereoFrame], out: &mut Vec<StereoFrame>) {
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

/// 启动解码线程
///
/// `dst_rate`：混音输出采样率。源采样率不同时由 worker 侧重采样，
/// 环形缓冲内永远是输出采样率，音频线程读取即纯 memcpy。
pub(crate) fn spawn_worker(
    mut reader: crate::base::audio::decoder::SymphoniaReader,
    ring: Arc<SharedRing>,
    cmd: Arc<CmdSlot>,
    dst_rate: u32,
) -> JoinHandle<()> {
    let mut resampler = Resampler::new(reader.src_rate(), dst_rate);
    std::thread::Builder::new()
        .name("starfish-audio-decoder".into())
        .spawn(move || {
            let mut pending: Vec<StereoFrame> = Vec::new();
            let mut resampled: Vec<StereoFrame> = Vec::new();

            loop {
                // ── 1. 处理控制命令（顺带充当空位轮询的睡眠点） ──
                match cmd.take(Duration::from_millis(5)) {
                    Some(Cmd::Stop) => return,
                    Some(Cmd::Seek(sec)) => {
                        pending.clear();
                        match reader.seek(sec) {
                            // 先换代再写新数据，读者据此丢弃旧代残留
                            Ok(()) => ring.begin_generation(),
                            Err(e) => eprintln!("[starfish audio] seek 失败: {e}"),
                        }
                    }
                    None => {}
                }

                // ── 2. 空位不足则继续睡（约 4096 帧 ≈ 85ms 粒度批量写入） ──
                if ring.free() < 4096 {
                    continue;
                }

                // ── 3. 上一包余量优先写入 ──
                if !pending.is_empty() {
                    let n = ring.write(&pending);
                    pending.drain(..n);
                    continue;
                }

                // ── 4. 解码下一包 ──
                match reader.next_interleaved() {
                    Ok(Some(chunk)) if chunk.is_empty() => continue,
                    Ok(Some(chunk)) => {
                        let mut frames: Vec<StereoFrame> =
                            Vec::with_capacity(chunk.len() / 2);
                        if reader.src_channels() == 1 {
                            // 单声道流：环形缓冲保持统一立体声（容量以秒计有界，翻倍代价可控），
                            // 与 SoundData 的单声道省内存策略不同
                            for &s in &chunk {
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
                        resampled.clear();
                        resampler.process(&frames, &mut resampled);

                        let n = ring.write(&resampled);
                        pending.extend_from_slice(&resampled[n..]);
                    }
                    Ok(None) => {
                        // 流结束：把余量灌完再置 EOF，保证尾部不截断
                        while !pending.is_empty() {
                            if matches!(cmd.take(Duration::from_millis(5)), Some(Cmd::Stop)) {
                                return;
                            }
                            let n = ring.write(&pending);
                            pending.drain(..n);
                        }
                        ring.set_eof();
                        return;
                    }
                    Err(e) => {
                        eprintln!("[starfish audio] 解码错误，流终止: {e}");
                        ring.set_eof();
                        return;
                    }
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
    fn resampler_upsample_2x_length_and_values() {
        // 22050 → 44100：step = 0.5
        let mut r = Resampler::new(22050, 44100);
        let src: Vec<StereoFrame> = (0..100).map(|i| frame(i as f32)).collect();
        let mut out = Vec::new();
        r.process(&src, &mut out);

        // 输出 t = 0, 0.5, ..., 98.5（t=99 的边界样本要等下一个包补上 → 198）
        assert_eq!(out.len(), 198);
        assert!((out[0].left - 0.0).abs() < 1e-6);
        assert!((out[197].left - 98.5).abs() < 1e-6);
        // 中间插值：t=0.5 处应为 0.5
        assert!((out[1].left - 0.5).abs() < 1e-6);
    }

    #[test]
    fn resampler_same_rate_passthrough() {
        let mut r = Resampler::new(44100, 44100);
        let src: Vec<StereoFrame> = (0..10).map(|i| frame(i as f32)).collect();
        let mut out = Vec::new();
        r.process(&src, &mut out);
        assert_eq!(out.len(), 10);
    }

    #[test]
    fn resampler_downsample_length() {
        // 44100 → 22050：step = 2.0，输出约一半
        let mut r = Resampler::new(44100, 22050);
        let src: Vec<StereoFrame> = (0..100).map(|i| frame(i as f32)).collect();
        let mut out = Vec::new();
        r.process(&src, &mut out);
        assert_eq!(out.len(), 50);
        // 抽取点保持原值
        assert!((out[0].left - 0.0).abs() < 1e-6);
        assert!((out[1].left - 2.0).abs() < 1e-6);
    }

    #[test]
    fn resampler_cross_packet_continuity() {
        // 分两个包送入，结果应与一次性送入一致
        let src: Vec<StereoFrame> = (0..64).map(|i| frame(i as f32 * 0.5)).collect();
        let mut whole = Vec::new();
        Resampler::new(22050, 44100).process(&src, &mut whole);

        let mut split = Vec::new();
        let mut r = Resampler::new(22050, 44100);
        r.process(&src[..13], &mut split);
        r.process(&src[13..], &mut split);

        assert_eq!(whole.len(), split.len());
        for (a, b) in whole.iter().zip(split.iter()) {
            assert!((a.left - b.left).abs() < 1e-6);
        }
    }
}
