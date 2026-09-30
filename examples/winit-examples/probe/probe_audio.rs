//! probe_audio：音频播放链路验证（内嵌 WAV 解码 → 混音器 SFX → 播完判定）
//!
//! 全平台同一源码零 `#[cfg]`：内嵌字节解码（symphonia）+ AudioMixer 播放。
//! Web 端自动播放策略需手势授权——无头验证加
//! `--autoplay-policy=no-user-gesture-required`。
//!
//! 判据：console 锚点 `[audio] DECODE PASS`、`[audio] PLAY PASS`、
//! `[audio] DONE PASS`（播完自动判定）。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::audio::decoder::SymphoniaReader;
use starfish::base::audio::{AudioMixer, SoundData};
use starfish::base::window::{Window, WindowEvent};

use std::sync::Arc;

struct AudioProbe {
    panel: StatusPanel,
    mixer: Option<AudioMixer>,
    sound_len: f32,
    play_elapsed: f32,
    decoded: bool,
    played: bool,
    done: bool,
}

impl AudioProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            mixer: None,
            sound_len: 0.0,
            play_elapsed: 0.0,
            decoded: false,
            played: false,
            done: false,
        }
    }

    /// 解码内嵌 WAV → 混音器播放（对齐旧 21 号的拉式解码流程）
    fn decode_and_play(&mut self) {
        let wav = include_bytes!("../../resources/audio/solid.wav").to_vec();
        let mut reader = match SymphoniaReader::from_bytes(wav) {
            Ok(r) => r,
            Err(e) => {
                self.panel.verdict("audio", "DECODE", Status::Fail, &format!("{e:?}"));
                return;
            }
        };
        let rate = reader.src_rate();
        let mut samples: Vec<f32> = Vec::new();
        loop {
            match reader.next_interleaved() {
                Ok(Some(chunk)) => samples.extend(chunk),
                Ok(None) => break,
                Err(e) => {
                    self.panel.verdict("audio", "DECODE", Status::Fail, &format!("{e:?}"));
                    return;
                }
            }
        }
        if samples.is_empty() {
            self.panel.verdict("audio", "DECODE", Status::Fail, "empty");
            return;
        }
        self.panel
            .verdict("audio", "DECODE", Status::Pass, &format!("{}B", samples.len() * 4));

        let mut mixer = match AudioMixer::new(4) {
            Ok(m) => m,
            Err(e) => {
                self.panel.verdict("audio", "PLAY", Status::Fail, &format!("mixer {e:?}"));
                return;
            }
        };
        let sound = Arc::new(SoundData::from_interleaved_f32(&samples, rate));
        self.sound_len = sound.duration();
        match mixer.play_with(sound, 0) {
            Ok(_) => {
                self.panel
                    .verdict("audio", "PLAY", Status::Pass, &format!("{:.1}s", self.sound_len));
                self.played = true;
            }
            Err(e) => self.panel.verdict("audio", "PLAY", Status::Fail, &format!("{e:?}")),
        }
        self.mixer = Some(mixer); // 保活（_drop 即停）
    }
}

impl Application for AudioProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        // 浏览器自动播放策略：页面首次点击（用户激活）后重试播放——
        // 自动起跑被策略拦下时，点一下页面即出声并继续 DONE 判定
        if let WindowEvent::MousePressed(_) = event {
            if self.decoded && !self.done {
                self.mixer = None;
                self.play_elapsed = 0.0;
                self.decode_and_play();
            }
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();
        if !self.decoded && f == 30 {
            self.decoded = true;
            self.decode_and_play();
        }

        // 播放推进：混音器通道忙 → 计时；超过时长 + 0.5s 余量 → DONE
        if self.played && !self.done {
            self.play_elapsed += ctx.delta();
            if self.play_elapsed >= self.sound_len + 0.5 {
                self.done = true;
                self.panel.verdict("audio", "DONE", Status::Pass, "");
            }
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    AudioProbe::new(),
    WindowConfig::new("probe audio", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
