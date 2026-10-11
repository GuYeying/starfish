//! pygame.event 运行测试：事件泵稳定性 + 交互事件计数。
//!
//! 自检项（状态条顺序）：泵可用 / 240 帧泵稳定（晚登记）。
//! 交互计数（不参与判定，console 汇总）：按键数 / 点击数 / 移动数。
//! 场景：任一事件到达闪黄块、点击闪红块（事件流的视觉证据）；
//! 期间按方向键 / 点鼠标可看到计数增长（真机可玩）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::Event;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.event");
    common::log("[event] set_mode 完成，进入自检");

    checks.require("泵可用", true);

    let mut frames = 0u32;
    let (mut keys, mut clicks, mut motions) = (0usize, 0usize, 0usize);
    let mut flash_event = 0u32;
    let mut flash_click = 0u32;
    loop {
        // 本案例自带泵（统计事件）——不用 common::quit_requested 排空
        for e in starfish::pygame::event::get() {
            match e {
                Event::Quit => return Ok(()),
                Event::KeyDown { key: starfish::base::window::KeyCode::Escape, .. } => return Ok(()),
                Event::KeyDown { .. } => {
                    keys += 1;
                    flash_event = 20;
                }
                Event::MouseButtonDown { .. } => {
                    clicks += 1;
                    flash_event = 20;
                    flash_click = 20;
                }
                Event::MouseMotion { .. } => {
                    motions += 1;
                    flash_event = 10;
                }
                _ => {}
            }
        }

        frames += 1;
        if frames == 240 {
            checks.require("240 帧泵稳定", true);
            common::log(&format!(
                "[event] 240 帧累计：按键 {keys} / 点击 {clicks} / 移动 {motions}"
            ));
        }

        flash_event = flash_event.saturating_sub(1);
        flash_click = flash_click.saturating_sub(1);
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                if flash_click > 0 {
                    b.push_quad(340.0, 260.0, 120.0, 80.0, [0.9020, 0.2353, 0.2353, 1.0000], None);
                } else if flash_event > 0 {
                    b.push_quad(340.0, 260.0, 120.0, 80.0, [0.9412, 0.7843, 0.2353, 1.0000], None);
                }
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
    common::log(&format!("[event] {frames} 帧完成，自检 {passed}/{total}"));
    common::log(&format!(
        "[event] 汇总：按键 {keys} / 点击 {clicks} / 移动 {motions}"
    ));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
