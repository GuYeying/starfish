//! pygame.touch 运行测试：触摸查询面（无触屏安全）。
//!
//! 自检项（状态条顺序）：count 与 fingers 一致 / 无效 finger → None /
//! 连续查询稳定（晚登记）。
//! 场景：底部触点条（有触摸设备时逐指画块；桌面恒空——视觉默认静默）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::touch;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.touch");
    common::log("[touch] set_mode 完成，进入自检");

    checks.require(
        "count 与 fingers 一致",
        touch::get_count() == touch::fingers().len(),
    );
    checks.require("无效 finger → None", touch::get_pos(u64::MAX).is_none());

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        if frames == 120 {
            // 桌面 120 帧恒空表 / 触屏设备持续采样均无 panic
            let _ = (touch::get_count(), touch::fingers(), touch::get_pos(0));
            checks.require("连续查询稳定", true);
        }

        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 触点条：按 finger 位置画块（桌面恒无）
                for finger in touch::fingers().iter() {
                    let x = finger.x as f32;
                    let y = finger.y as f32;
                    b.push_quad(
                        x - 16.0,
                        y - 16.0,
                        32.0,
                        32.0,
                        [0.3529, 0.7843, 0.4314, 1.0000],
                        None,
                    );
                }
                // 底部触区提示框
                b.push_quad(40.0, 520.0, 720.0, 60.0, [0.1961, 0.2157, 0.3333, 1.0000], None);
            });
            draw_status(&f, &checks);
            f.end();
        }
        display::flip();
        common::paced_frame().await;
        frames += 1;
        if frames >= common::EXIT_FRAMES {
            break;
        }
    }
    let (passed, total) = checks.summary();
    common::log(&format!("[touch] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
