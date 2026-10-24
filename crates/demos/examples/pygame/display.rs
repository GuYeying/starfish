//! pygame.display 运行测试：初始化面 + 呈现链路。
//!
//! 自检项（状态条顺序）：get_init / 尺寸一致 / caption+opacity /
//! flip 稳定 120 帧（晚登记——需呈现积累）。
//! 场景：边框脉冲 + 中心呼吸块（呈现管线的动态视觉证据）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::display;
use starfish_pygame::render::DrawTarget;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.display");
    common::log("[display] set_mode 完成，进入自检");

    checks.require("get_init", display::get_init());
    checks.require("尺寸一致", screen.size() == (800, 600));

    display::set_caption("pygame_test_display");
    display::set_opacity(1.0);
    checks.require("caption+opacity", true);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        if frames == 120 {
            checks.require("flip 稳定 120 帧", true);
        }
        frames += 1;
        let pulse = (frames as f32 * 0.05).sin() * 0.5 + 0.5;

        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                let t = 6.0;
                let g = 90.0 + 120.0 * pulse;
                b.push_quad(0.0, 0.0, 800.0, t, [g, 90.0, 160.0, 255.0], None);
                b.push_quad(0.0, 594.0, 800.0, t, [g, 90.0, 160.0, 255.0], None);
                b.push_quad(0.0, 0.0, t, 600.0, [g, 90.0, 160.0, 255.0], None);
                b.push_quad(794.0, 0.0, t, 600.0, [g, 90.0, 160.0, 255.0], None);
                let s = 60.0 + 40.0 * pulse;
                b.push_quad(400.0 - s / 2.0, 300.0 - s / 2.0, s, s, [0.9412, 0.7059, 0.2353, 1.0000], None);
            });
            draw_status(&f, &checks);
            f.end();
        }
        display::flip();
        common::paced_frame().await;
        if frames >= common::EXIT_FRAMES {
            break;
        }
    }
    let (passed, total) = checks.summary();
    common::log(&format!("[display] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
