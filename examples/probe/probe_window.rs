//! probe_window：窗口 / 循环 / 输入 / 尺寸自愈 / 诊断面板 —— probe 家族骨架验证
//!
//! 验证点：真实尺寸经 Resized 到达（web 尺寸竞态）、delta 正常流动、
//! 点击计数、面板渲染链路。全平台同一源码，应用代码零 `#[cfg]`。
//!
//! 判据：console 锚点 `[window] SIZE PASS`（约 2.5s 后自动判定）；
//! Esc 关窗（桌面）。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::window::{Window, WindowEvent};

struct WindowProbe {
    panel: StatusPanel,
    taps: u32,
    resized: bool,
    passed: bool,
}

impl WindowProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            taps: 0,
            resized: false,
            passed: false,
        }
    }
}

impl Application for WindowProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        match event {
            WindowEvent::Resized { width, height } => {
                if *width > 0 && *height > 0 {
                    self.panel.on_resize(*width, *height);
                    self.resized = true;
                }
            }
            WindowEvent::MousePressed(_) => self.taps += 1,
            _ => {}
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();
        // 每 60 帧刷新实时信息行（size / delta / taps）
        if f % 60 == 0 {
            let (w, h) = ctx.size();
            self.panel.set_line(
                0,
                &format!(
                    "size {w}x{h}  dt {:.2}ms  taps {}",
                    ctx.delta() * 1000.0,
                    self.taps
                ),
            );
        }
        // ~2.5s 后判定：尺寸到达 + 时间流动 + 循环稳定推进
        if !self.passed && f > 150 && self.resized && ctx.delta() > 0.0 {
            let (w, h) = ctx.size();
            self.panel
                .verdict("window", "SIZE", Status::Pass, &format!("{w}x{h}"));
            self.passed = true;
        }
        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    WindowProbe::new(),
    WindowConfig::new("probe window", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
