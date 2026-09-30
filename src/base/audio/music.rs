//! 背景音乐播放器（流式）
//!
//! 占用引擎内部一条不可见的声道，开发者通过 `AudioMixer::music_*` 控制。
//! 数据源二选一（内部枚举 [`MusicSource`]）：
//!   - [`MusicStream`]：流式（边解码边播，适合长音频）
//!   - [`SoundData`]：整段内存数据（兼容旧接口 `music_load`）
//!
//! 支持：播放/暂停/停止、循环、淡变、跳转、排队切歌、效果器。
//!
//! 线程约定：本类型会被 `Inner`（与音频回调共享的 `Mutex`）持有，
//! 因此所有涉及 join 的操作都只把 worker 句柄收集进 [`Self::retired`]，
//! 由 AudioMixer 的控制线程在**锁外**统一 join——绝不在持锁状态下 join。

use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::base::audio::common::{AudioEffect, ChannelState, FadeState, FadeType};
use crate::base::audio::stream::{Cmd, MusicStream};
use crate::base::audio::SoundData;
use crate::base::audio::common::StereoFrame;

/// 音乐数据源（内部枚举：流式 / 内存缓冲）
pub(crate) enum MusicSource {
    Stream(MusicStream),
    Buffer { data: Arc<SoundData>, cursor: usize },
}

impl MusicSource {
    fn rate(&self) -> u32 {
        match self {
            MusicSource::Stream(s) => s.sample_rate,
            MusicSource::Buffer { data, .. } => data.sample_rate,
        }
    }

    fn duration(&self) -> Option<f32> {
        match self {
            MusicSource::Stream(s) => s.duration,
            MusicSource::Buffer { data, .. } => {
                Some(data.frame_count() as f32 / data.sample_rate as f32)
            }
        }
    }
}

/// 背景音乐播放器
pub struct MusicPlayer {
    pub(crate) source: Option<MusicSource>,
    pub(crate) state: ChannelState,
    pub(crate) loops: i32,
    pub(crate) fade: Option<FadeState>,
    pub(crate) queue: VecDeque<MusicSource>,
    /// 效果器链（与 SfxChannel 共用 AudioEffect trait）
    pub(crate) effects: Vec<Box<dyn AudioEffect>>,
    /// 当前播放位置（帧，按当前源采样率）
    pub(crate) position_frames: u64,
    /// 已退役的解码线程句柄，等待控制线程锁外 join
    pub(crate) retired: Vec<JoinHandle<()>>,
}

impl MusicPlayer {
    pub(crate) fn new() -> Self {
        Self {
            source: None,
            state: ChannelState::Stopped,
            loops: 0,
            fade: None,
            queue: VecDeque::new(),
            effects: Vec::new(),
            position_frames: 0,
            retired: Vec::new(),
        }
    }

    // ───────────────────────── 源管理（控制线程） ─────────────────────────

    /// 加载（替换当前源，不自动播放）。旧流式 worker 退役待 join
    pub(crate) fn load(&mut self, source: MusicSource) {
        self.retire_current();
        self.source = Some(source);
        self.position_frames = 0;
        self.state = ChannelState::Stopped;
        self.fade = None;
    }

    /// 排队下一首。流式源立即预起解码线程（灌满环形缓冲即 park，零 CPU）
    pub(crate) fn queue(&mut self, source: MusicSource) {
        if self.source.is_none() {
            self.load(source);
        } else {
            self.queue.push_back(source);
        }
    }

    /// 退役当前源（流式：发 Stop 并收走 JoinHandle，绝不在此 join）
    fn retire_current(&mut self) {
        if let Some(MusicSource::Stream(mut s)) = self.source.take() {
            s.request_stop();
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(h) = s.take_worker() {
                self.retired.push(h);
            }
        }
    }

    /// 控制面：取走待 join 的句柄（调用方必须在锁外 join）
    pub(crate) fn drain_retired(&mut self) -> Vec<JoinHandle<()>> {
        std::mem::take(&mut self.retired)
    }

    /// 控制面：彻底关停（当前源 + 队列中所有流式 worker），Drop 收口用
    pub(crate) fn shutdown(&mut self) {
        self.retire_current();
        for src in self.queue.drain(..) {
            if let MusicSource::Stream(mut s) = src {
                s.request_stop();
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(h) = s.take_worker() {
                    self.retired.push(h);
                }
            }
        }
    }

    /// 无线程环境（Web）：每帧推进流式解码（预算：帧数）
    ///
    /// 桌面（有线程）无需调用——解码线程自动维持缓冲。
    pub(crate) fn pump(&mut self, budget_frames: usize) {
        if let Some(MusicSource::Stream(s)) = &mut self.source {
            s.pump(budget_frames);
        }
    }

    // ───────────────────────── 播放控制（控制线程） ─────────────────────────

    /// 开始播放（从头开始）
    pub(crate) fn play(&mut self, loops: i32) {
        if self.source.is_some() {
            self.restart();
            self.loops = loops;
            self.state = ChannelState::Playing;
        }
    }

    /// 停止（play 可重播；效果器状态保留）
    pub(crate) fn stop(&mut self) {
        self.state = ChannelState::Stopped;
        self.fade = None;
        self.restart();
    }

    /// 暂停
    pub(crate) fn pause(&mut self) {
        if self.state == ChannelState::Playing {
            self.state = ChannelState::Paused;
        }
    }

    /// 恢复
    pub(crate) fn resume(&mut self) {
        if self.state == ChannelState::Paused {
            self.state = ChannelState::Playing;
        }
    }

    /// 淡出停止
    pub(crate) fn fade_out(&mut self, ms: u32) {
        let sample_rate = self.source.as_ref().map_or(44100, |s| s.rate());
        self.fade = Some(FadeState::new_fade_out(ms, sample_rate));
    }

    /// 淡入（从当前音量开始淡入到完整音量）
    ///
    /// `ms`: 淡入时长（毫秒）
    pub(crate) fn fade_in(&mut self, ms: u32) {
        let sample_rate = self.source.as_ref().map_or(44100, |s| s.rate());
        self.fade = Some(FadeState::new_fade_in(ms, sample_rate));
    }

    /// 跳转到指定秒数（流式向解码线程发 Seek 命令，换代丢弃旧数据）
    pub(crate) fn seek(&mut self, seconds: f32) {
        let seconds = seconds.max(0.0);
        match &mut self.source {
            Some(MusicSource::Stream(s)) => s.cmd.send(Cmd::Seek(seconds)),
            Some(MusicSource::Buffer { data, cursor }) => {
                let frame = (seconds * data.sample_rate as f32) as usize;
                *cursor = frame.min(data.frame_count());
            }
            None => {}
        }
        let rate = self.source.as_ref().map_or(44100, |s| s.rate());
        self.position_frames = (seconds * rate as f32) as u64;
    }

    /// 从头开始（不改变播放状态）
    pub(crate) fn rewind(&mut self) {
        self.seek(0.0);
    }

    /// 添加效果器
    pub(crate) fn add_effect(&mut self, effect: Box<dyn AudioEffect>) {
        self.effects.push(effect);
    }

    /// 当前播放位置（秒）
    pub(crate) fn position(&self) -> f32 {
        match &self.source {
            Some(src) => self.position_frames as f32 / src.rate() as f32,
            None => 0.0,
        }
    }

    /// 总时长（秒）
    pub(crate) fn duration(&self) -> f32 {
        self.source
            .as_ref()
            .and_then(MusicSource::duration)
            .unwrap_or(0.0)
    }

    /// 淡变增益
    pub(crate) fn fade_gain(&self) -> f32 {
        self.fade.as_ref().map_or(1.0, |f| f.gain())
    }

    // ───────────────────────── 数据通路（音频线程，持 Inner 锁） ─────────────────────────

    /// 读一段 PCM 帧
    ///
    /// 返回实际读取的帧数。流式源暂时欠载（解码未跟上）时返回 0，
    /// 本回调输出静音、播放状态不变。
    pub(crate) fn read_frames(&mut self, output: &mut [StereoFrame]) -> usize {
        let n = self.fill(output);
        if n == 0 {
            return 0;
        }
        self.position_frames += n as u64;

        // ── 效果器链（在淡变之前应用） ──
        for effect in &mut self.effects {
            effect.process(&mut output[..n]);
        }

        // ── 淡变 ──
        if let Some(fade) = &mut self.fade {
            let done = fade.advance(n);
            if done && fade.fade_type == FadeType::Out {
                self.state = ChannelState::Stopped;
                // 淡出结束：尝试接续队首
                if !self.queue.is_empty() {
                    let next = self.queue.pop_front().unwrap();
                    self.load(next);
                    self.loops = 0;
                    self.state = ChannelState::Playing;
                }
                self.fade = None;
            } else if done {
                self.fade = None;
            }
        }

        n
    }

    /// 从当前源填充 output；源耗尽时按队列/循环语义接续
    ///
    /// 曲目切换只发生在"读不到任何帧"的回调边界，与旧版时序一致。
    fn fill(&mut self, output: &mut [StereoFrame]) -> usize {
        loop {
            let n = match &mut self.source {
                Some(MusicSource::Buffer { data, cursor }) => {
                    if *cursor >= data.frame_count() {
                        0
                    } else {
                        // 单声道源在此展开为 L=R（SoundData::read_into 内部分派）
                        let n = data.read_into(*cursor, output);
                        *cursor += n;
                        n
                    }
                }
                Some(MusicSource::Stream(s)) => {
                    let n = s.reader.read_frames(output);
                    if n == 0 && !s.reader.finished() {
                        // 暂时欠载：解码未跟上，本帧输出静音，保持等待
                        return 0;
                    }
                    n
                }
                None => {
                    self.state = ChannelState::Stopped;
                    return 0;
                }
            };

            if n > 0 {
                return n;
            }

            // 当前源已耗尽 → 队列 / 循环 / 停止
            if !self.queue.is_empty() {
                let next = self.queue.pop_front().unwrap();
                self.load(next);
                self.loops = 0;
                self.state = ChannelState::Playing;
                continue;
            }
            if self.loops == -1 {
                self.restart();
            } else if self.loops > 0 {
                self.loops -= 1;
                self.restart();
            } else {
                self.state = ChannelState::Stopped;
                return 0;
            }
        }
    }

    /// 重头播放当前源（流式发 Seek(0) 换代，缓冲置 0）
    fn restart(&mut self) {
        match &mut self.source {
            Some(MusicSource::Stream(s)) => s.cmd.send(Cmd::Seek(0.0)),
            Some(MusicSource::Buffer { cursor, .. }) => *cursor = 0,
            None => {}
        }
        self.position_frames = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成 16-bit 单声道 PCM WAV 测试文件
    fn write_test_wav(path: &std::path::Path, sample_rate: u32, seconds: f32) {
        let n = (sample_rate as f32 * seconds) as usize;
        let mut data = Vec::with_capacity(n * 2);
        for i in 0..n {
            let t = i as f32 / sample_rate as f32;
            let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
            data.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
        }

        let mut wav = Vec::new();
        let data_len = (n * 2) as u32;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn buffer_queue_sequencing() {
        let mut player = MusicPlayer::new();
        let a = Arc::new(SoundData::from_interleaved_f32(&vec![0.1f32; 200], 8000)); // 100 帧
        let b = Arc::new(SoundData::from_interleaved_f32(&vec![0.2f32; 200], 8000));
        player.load(MusicSource::Buffer { data: a, cursor: 0 });
        player.play(0);
        player.queue(MusicSource::Buffer { data: b, cursor: 0 });

        let mut buf = [StereoFrame::SILENT; 64];
        let mut total = 0;
        let mut saw_b = false;
        for _ in 0..100 {
            let n = player.read_frames(&mut buf);
            if n == 0 {
                break;
            }
            if buf[..n].iter().any(|f| f.left > 0.15) {
                saw_b = true;
            }
            total += n;
        }
        assert_eq!(total, 200);
        assert!(saw_b, "应切换到排队的曲目 b");
        assert_eq!(player.state, ChannelState::Stopped);
    }

    #[test]
    fn buffer_loop_count() {
        let mut player = MusicPlayer::new();
        let a = Arc::new(SoundData::from_interleaved_f32(&vec![0.1f32; 200], 8000));
        player.load(MusicSource::Buffer { data: a, cursor: 0 });
        player.play(2); // 播 3 遍

        let mut buf = [StereoFrame::SILENT; 64];
        let mut total = 0;
        for _ in 0..100 {
            let n = player.read_frames(&mut buf);
            if n == 0 {
                break;
            }
            total += n;
        }
        assert_eq!(total, 300);
    }

    #[test]
    fn stream_end_to_end() {
        let dir = std::env::temp_dir().join("starfish_audio_test");
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = dir.join("sine_1s_8000hz.wav");
        write_test_wav(&wav_path, 8000, 1.0);

        let out_rate = 44100;
        let mut player = MusicPlayer::new();
        let stream = MusicStream::from_file(wav_path.to_str().unwrap(), out_rate).unwrap();
        assert_eq!(stream.sample_rate(), out_rate);
        // WAV 容器提供 n_frames → 时长应为 1.0s
        let dur = stream.duration().unwrap();
        assert!((dur - 1.0).abs() < 0.01, "duration = {dur}");

        player.load(MusicSource::Stream(stream));
        player.play(0);

        // 模拟混音循环：读完全部输出（欠载时稍等解码线程）
        let mut total = 0usize;
        let mut buf = vec![StereoFrame::SILENT; 1024];
        let mut waits = 0;
        loop {
            let n = player.read_frames(&mut buf);
            total += n;
            if player.state == ChannelState::Stopped {
                break;
            }
            if n == 0 {
                waits += 1;
                assert!(waits < 2000, "等待解码超时");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }

        // 8000Hz → 44100Hz 重采样，总帧数应接近 44100
        assert!(
            total > 43_000 && total < 45_200,
            "总帧数 {total} 应接近 44100"
        );
        std::fs::remove_file(&wav_path).ok();
    }

    #[test]
    fn stream_seek_updates_position_and_finishes() {
        let dir = std::env::temp_dir().join("starfish_audio_test");
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = dir.join("sine_2s_8000hz.wav");
        write_test_wav(&wav_path, 8000, 2.0);

        let out_rate = 44100;
        let mut player = MusicPlayer::new();
        let stream = MusicStream::from_file(wav_path.to_str().unwrap(), out_rate).unwrap();
        player.load(MusicSource::Stream(stream));
        player.play(0);

        // 播一小段后 seek 到 1.5s
        let mut buf = vec![StereoFrame::SILENT; 1024];
        for _ in 0..5 {
            if player.read_frames(&mut buf) == 0 {
                break;
            }
        }
        player.seek(1.5);
        assert!((player.position() - 1.5).abs() < 0.01);

        // 读完剩余部分直至停止
        let mut waits = 0;
        loop {
            let n = player.read_frames(&mut buf);
            if player.state == ChannelState::Stopped {
                break;
            }
            if n == 0 {
                waits += 1;
                assert!(waits < 2000, "等待解码超时");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        // 结束位置应接近 2s（seek 落点在目标之前最近的可解码点，允许少量偏差）
        assert!(
            (player.position() - 2.0).abs() < 0.2,
            "结束位置 {} 应接近 2.0",
            player.position()
        );
        std::fs::remove_file(&wav_path).ok();
    }
}
