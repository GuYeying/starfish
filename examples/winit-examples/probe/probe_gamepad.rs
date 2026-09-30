//! probe_gamepad：手柄状态表轮询验证（对齐键鼠的轮询模式）
//!
//! 判据：console 锚点 `[gamepad] POLL PASS pads=N`。pads=0（未插手柄 /
//! Android Stub 空表）同样 PASS——验证的是状态表 API 可调用、轮询管线
//! 稳定；插上手柄后按键/摇杆值实时上屏。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::gamepad::{Axis, Button};
use starfish::base::window::{Window, WindowEvent};

struct GamepadProbe {
    panel: StatusPanel,
    passed: bool,
}

impl GamepadProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            passed: false,
        }
    }
}

impl Application for GamepadProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, _event: &WindowEvent, _ctx: &mut Ctx) {}

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();
        let gp = ctx.gamepad();

        // 实时状态行（每 15 帧刷新一次即可）
        if f % 15 == 0 {
            let pads = gp.connected().count();
            let line = match gp.primary() {
                Some(id) => {
                    let buttons: u32 = [
                        Button::South,
                        Button::East,
                        Button::North,
                        Button::West,
                        Button::Start,
                    ]
                    .into_iter()
                    .enumerate()
                    .filter(|(_, b)| gp.is_pressed(id, *b))
                    .fold(0u32, |acc, (i, _)| acc | (1 << i));
                    format!(
                        "pads {pads}  pad{id} btn:{buttons:05b}  L({:.2},{:.2}) R({:.2},{:.2})",
                        gp.axis(id, Axis::LeftStickX),
                        gp.axis(id, Axis::LeftStickY),
                        gp.axis(id, Axis::RightStickX),
                        gp.axis(id, Axis::RightStickY),
                    )
                }
                None => format!("pads {pads}  (no pad connected)"),
            };
            self.panel.set_line(1, &line);
        }

        // ~2.5s 判定：状态表 API 可调用、轮询管线稳定即 PASS
        if !self.passed && f > 150 {
            let pads = gp.connected().count();
            self.panel
                .verdict("gamepad", "POLL", Status::Pass, &format!("pads={pads}"));
            self.passed = true;
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    GamepadProbe::new(),
    WindowConfig::new("probe gamepad", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
