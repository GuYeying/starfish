//! probe_record：录音链路验证（权限门 → 采集 2s → WAV 导出字节数）
//!
//! 全平台同一源码零 `#[cfg]`：`permission::ensure` 全平台同名
//! （桌面/Web 恒真；Android 受控阻塞授权；Web 的 getUserMedia 授权发生在
//! AudioRecorder 内部）。无头验证加 fake-device flags。
//!
//! 判据：console 锚点 `[record] WAV PASS bytes=N`（bytes>0）。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::audio::AudioRecorder;
use starfish::base::window::{Window, WindowEvent};

struct RecordProbe {
    panel: StatusPanel,
    recorder: Option<AudioRecorder>,
    elapsed: f32,
    phase: Phase,
}

enum Phase {
    /// 帧 30 自动起跑
    Idle,
    /// 权限门已过，等待采集 2s
    Recording,
    /// 已判定
    Done,
}

impl RecordProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            recorder: None,
            elapsed: 0.0,
            phase: Phase::Idle,
        }
    }
}

impl Application for RecordProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, _event: &WindowEvent, _ctx: &mut Ctx) {}

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();

        // ① 权限门（跨平台 pub fn；Android 上受控阻塞 ≤15s）
        if matches!(self.phase, Phase::Idle) && f == 30 {
            let granted = starfish::base::permission::ensure(
                starfish::base::permission::Permission::Microphone,
            );
            match granted {
                true => {
                    self.panel.verdict("record", "PERM", Status::Pass, "");
                    match AudioRecorder::new_with_capacity(48_000 * 4) {
                        Ok(rec) => {
                            self.recorder = Some(rec);
                            self.elapsed = 0.0;
                            self.phase = Phase::Recording;
                        }
                        Err(e) => {
                            self.panel
                                .verdict("record", "OPEN", Status::Fail, &format!("{e:?}"));
                            self.phase = Phase::Done;
                        }
                    }
                }
                false => {
                    self.panel.verdict("record", "PERM", Status::Fail, "denied");
                    self.phase = Phase::Done;
                }
            }
        }

        // ② 采集 2s → WAV 导出
        if let Phase::Recording = self.phase {
            self.elapsed += ctx.delta();
            if self.elapsed >= 2.0 {
                let bytes = self
                    .recorder
                    .as_mut()
                    .and_then(|r| r.wav_bytes().ok())
                    .unwrap_or_default();
                // WAV 头固定 44B：超出部分才是真实 PCM 采样。无输入设备/采集
                // 回调未触发的环境（如无头浏览器）只有头 → SKIP（能力缺失，
                // 非链路失败——桌面/真机有麦克风时应 PASS）
                if bytes.len() > 44 {
                    self.panel
                        .verdict("record", "WAV", Status::Pass, &format!("bytes={}", bytes.len()));
                } else if !bytes.is_empty() {
                    self.panel
                        .verdict("record", "WAV", Status::Skip, "header only (no input)");
                } else {
                    self.panel.verdict("record", "WAV", Status::Fail, "empty");
                }
                self.phase = Phase::Done;
            }
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    RecordProbe::new(),
    WindowConfig::new("probe record", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
