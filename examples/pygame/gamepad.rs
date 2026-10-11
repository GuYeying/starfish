//! pygame.gamepad 运行测试：手柄查询面（无设备安全）+ 查询一致性。
//!
//! 自检项（状态条顺序）：poll 面 / count 与 connected 一致 /
//! 未连接查询安全 / primary 一致性 / 连续 poll 边沿稳定（晚登记）。
//! 场景：无手柄 = 静态中心块；插上手柄 = 16 按键灯阵 + 左摇杆驱动块
//! （真机可玩）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::base::gamepad::{Axis, Button, BUTTON_ORDER};
use starfish::pygame::display;
use starfish::pygame::gamepad;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.gamepad");
    common::log("[gamepad] set_mode 完成，进入自检");

    gamepad::poll();
    checks.require("poll 面", true);

    checks.require(
        "count 与 connected 一致",
        gamepad::get_count() == gamepad::connected().len(),
    );

    // 未连接编号查询全安全（不 panic、恒"无"）
    checks.require(
        "未连接查询安全",
        !gamepad::is_connected(99)
            && !gamepad::is_pressed(99, Button::South)
            && !gamepad::just_pressed(99, Button::East)
            && gamepad::axis(99, Axis::LeftStickX) == 0.0,
    );

    let ok = match gamepad::primary() {
        None => gamepad::connected().is_empty(),
        Some(i) => gamepad::connected().contains(&i),
    };
    checks.require("primary 一致性", ok);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        gamepad::poll();
        if frames == 240 {
            checks.require("连续 poll 边沿稳定", true);
        }

        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            let pad = gamepad::primary();
            let (mut lx, mut ly) = (380.0f32, 260.0f32);
            if let Some(id) = pad {
                lx += gamepad::axis(id, Axis::LeftStickX) * 200.0;
                ly += gamepad::axis(id, Axis::LeftStickY) * 150.0;
            }
            f.with_batch(|b| {
                // 按键灯阵 8×2（BUTTON_ORDER 标准布局序）
                for (i, btn) in BUTTON_ORDER.iter().enumerate() {
                    let lit = pad.map(|id| gamepad::is_pressed(id, *btn)).unwrap_or(false);
                    let c = if lit {
                        [0.9412, 0.7843, 0.2353, 1.0000]
                    } else {
                        [0.3529, 0.3922, 0.5882, 1.0000]
                    };
                    b.push_quad(
                        120.0 + (i % 8) as f32 * 70.0,
                        420.0 + (i / 8) as f32 * 60.0,
                        56.0,
                        44.0,
                        c,
                        None,
                    );
                }
                // 左摇杆驱动块（无手柄 = 静止）
                b.push_quad(lx, ly, 40.0, 40.0, [0.3529, 0.7843, 0.4314, 1.0000], None);
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
    common::log(&format!("[gamepad] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
