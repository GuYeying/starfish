//! pygame.draw 运行测试：六图元族一轮 + arc。
//!
//! 自检项（状态条顺序）：rect 填充 / rect 描边 / circle / line / lines /
//! ellipse / polygon / arc。
//! 场景：八个图元按检验参数实际绘制（返回 Rect 断言 + 视觉对照）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::display;
use starfish_pygame::draw;
use starfish_pygame::rect::Rect;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.draw");
    common::log("[draw] set_mode 完成，进入自检");

    let r1 = Rect::new(60, 120, 140, 90);
    let r2 = Rect::new(240, 120, 140, 90);
    let r5 = Rect::new(60, 320, 300, 180);
    let r6 = Rect::new(420, 320, 140, 90);
    let pts = [(420, 340), (560, 340), (600, 420), (490, 470), (380, 420)];

    // 图元参数在帧循环内逐帧重画（即时 API：积攒进批 → flip 提交）
    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);

            let rr = draw::rect(&f, [0, 200, 90], r1, 0);
            if frames == 1 {
                checks.require("rect 填充", rr.width == 140 && rr.height == 90);
            }
            let ro = draw::rect(&f, [90, 160, 255], r2, 3);
            if frames == 1 {
                checks.require("rect 描边", ro.width == 140);
            }
            let rc = draw::circle(&f, [240, 180, 60], (700, 180), 60, 0);
            if frames == 1 {
                checks.require(
                    "circle",
                    rc.left == 640 && rc.top == 120 && rc.width == 120 && rc.height == 120,
                );
            }
            let rl = draw::line(&f, [220, 80, 80], (40, 90), (200, 90), 1);
            if frames == 1 {
                // 水平线的 bounds 高度可为 0（界语义）——只断言横向覆盖
                checks.require("line", rl.width > 0);
            }
            let rls = draw::lines(&f, [180, 90, 220], false, &[(40, 300), (180, 260), (320, 300)], 1);
            if frames == 1 {
                checks.require("lines", rls.width > 0 && rls.height > 0);
            }
            let re = draw::ellipse(&f, [70, 200, 200], r5, 0);
            if frames == 1 {
                checks.require("ellipse", re.left == 60 && re.width == 300);
            }
            let rp = draw::polygon(&f, [220, 120, 60], &pts, 0);
            if frames == 1 {
                checks.require("polygon", rp.width > 0 && rp.height > 0);
            }
            let ra = draw::arc(&f, [240, 240, 240], r6, 0.0, std::f32::consts::PI, 1);
            if frames == 1 {
                checks.require("arc", ra.width == 140);
            }
        }
        display::flip();
        common::paced_frame().await;
        frames += 1;
        if frames >= common::EXIT_FRAMES {
            break;
        }
    }
    let (passed, total) = checks.summary();
    common::log(&format!("[draw] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
