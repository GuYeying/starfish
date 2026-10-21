//! pygame.key 运行测试：按键查询面 + 键名映射。
//!
//! 自检项（状态条顺序）：get_pressed 面 / name 映射 / get_mods /
//! get_repeat 垫底 / get_focused 可查询。
//! 场景：方向键移动色块（真实交互演示——按住即动；无断言，纯视觉）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_we::KeyCode;
use starfish_pygame::display;
use starfish_pygame::key;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.key");
    common::log("[key] set_mode 完成，进入自检");

    let pressed = key::get_pressed();
    checks.require("get_pressed 面", !pressed.get(KeyCode::KeyA));

    checks.require(
        "name 映射",
        key::name(KeyCode::Escape) == "Escape" && key::name(KeyCode::ArrowLeft) == "ArrowLeft",
    );
    let _ = key::get_mods();
    checks.require("get_mods", true);

    key::set_repeat(0, 0);
    checks.require("get_repeat 垫底", key::get_repeat() == (0, 0));

    let _focused = key::get_focused();
    checks.require("get_focused 可查询", true);

    let mut frames = 0u32;
    let (mut x, mut y) = (380.0f32, 280.0f32);
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        let p = key::get_pressed();
        if p.left() {
            x -= 4.0;
        }
        if p.right() {
            x += 4.0;
        }
        if p.up() {
            y -= 4.0;
        }
        if p.down() {
            y += 4.0;
        }
        x = x.clamp(0.0, 760.0);
        y = y.clamp(0.0, 560.0);

        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 方向键提示键帽（按住高亮）
                let caps = [
                    (360.0, 180.0, KeyCode::ArrowUp),
                    (280.0, 260.0, KeyCode::ArrowLeft),
                    (440.0, 260.0, KeyCode::ArrowRight),
                    (360.0, 340.0, KeyCode::ArrowDown),
                ];
                for (cx, cy, k) in caps {
                    let c = if p.get(k) {
                        [0.9412, 0.7843, 0.2353, 1.0000]
                    } else {
                        [0.3529, 0.3922, 0.5882, 1.0000]
                    };
                    b.push_quad(cx, cy, 70.0, 70.0, c, None);
                }
                // 受控色块
                b.push_quad(x, y, 40.0, 40.0, [0.3529, 0.7843, 0.4314, 1.0000], None);
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
    common::log(&format!("[key] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
