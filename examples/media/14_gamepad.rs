//! 示例 14：手柄输入（设备接口第一批）
//!
//! 引擎每帧排水 gilrs 事件刷新状态表；应用在 frame 里轮询读取。
//! 无手柄环境同样可运行（空表，各查询返回默认值）。
//!
//! 运行：cargo run --features gamepad --example 14_gamepad

use starfish::base::app::{run, Application, Ctx, WindowConfig};
use starfish::base::gamepad::{Axis, Button};

struct GamepadDemo {
    /// 上帧打印的状态串（仅变化时打印，避免刷屏）
    last_report: String,
}

impl Application for GamepadDemo {
    fn start(&mut self, _ctx: &mut Ctx) {
        println!("== 手柄示例：连接手柄后查看按键/摇杆状态 ==");
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let gp = ctx.gamepad();
        let Some(id) = gp.primary() else {
            if self.last_report != "none" {
                println!("未检测到手柄（等待连接…）");
                self.last_report = "none".into();
            }
            return;
        };

        // 按键：列出当前按下集合
        let pressed: Vec<&str> = [
            (Button::South, "南(S)"),
            (Button::East, "东(E)"),
            (Button::North, "北(N)"),
            (Button::West, "西(W)"),
            (Button::Start, "Start"),
            (Button::Select, "Select"),
            (Button::LeftTrigger, "LT"),
            (Button::RightTrigger, "RT"),
            (Button::DPadUp, "十字上"),
            (Button::DPadDown, "十字下"),
            (Button::DPadLeft, "十字左"),
            (Button::DPadRight, "十字右"),
        ]
        .iter()
        .filter(|(b, _)| gp.is_pressed(id, *b))
        .map(|(_, n)| *n)
        .collect();

        // 摇杆（死区过滤）
        let (lx, ly) = (gp.axis(id, Axis::LeftStickX), gp.axis(id, Axis::LeftStickY));
        let sticks = if lx.abs() > 0.08 || ly.abs() > 0.08 {
            format!("左摇杆=({lx:+.2}, {ly:+.2}) ")
        } else {
            String::new()
        };

        let report = format!("{sticks}按键=[{}]", pressed.join(" "));
        if report != self.last_report {
            println!("{report}");
            self.last_report = report;
        }
    }
}

fn main() {
    run(
        GamepadDemo {
            last_report: String::new(),
        },
        WindowConfig::new("手柄示例", 640, 480).with_fps_cap(60),
    );
}
