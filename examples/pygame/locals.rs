//! pygame.locals 运行测试：类型化常量枢纽。
//!
//! 自检项（状态条顺序）：K_a 映射 / K_ESCAPE 映射 / K_UP 映射 /
//! K_RETURN 映射 / KMOD 位独立 / BUTTON_LEFT 对齐 / KNOWN_MASK 覆盖。
//! 场景：键帽阵（映射过的常量按 W3C code 名排成 3×2 色块键盘）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::base::window::{KeyCode, MouseButton};
use starfish::pygame::display;
use starfish::pygame::key;
use starfish::pygame::locals;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.locals");
    common::log("[locals] set_mode 完成，进入自检");

    checks.require("K_a 映射", key::name(locals::key::K_a) == "KeyA");
    checks.require("K_ESCAPE 映射", key::name(locals::key::K_ESCAPE) == "Escape");
    checks.require("K_UP 映射", key::name(locals::key::K_UP) == "ArrowUp");
    checks.require(
        "K_RETURN 映射",
        key::name(locals::key::K_RETURN) == "Enter"
            && key::name(locals::key::K_SPACE) == "Space",
    );
    checks.require(
        "KMOD 位独立",
        key::KMOD_SHIFT != key::KMOD_CTRL
            && key::KMOD_CTRL != key::KMOD_ALT
            && key::KMOD_ALT == 1 << 2,
    );
    checks.require(
        "BUTTON_LEFT 对齐",
        matches!(locals::mouse::BUTTON_LEFT, MouseButton::Left)
            && matches!(locals::mouse::BUTTON_RIGHT, MouseButton::Right),
    );
    checks.require(
        "KNOWN_MASK 覆盖",
        locals::display::KNOWN_MASK & locals::display::RESIZABLE != 0
            && locals::display::KNOWN_MASK & locals::display::FULLSCREEN != 0,
    );

    let keys = [
        locals::key::K_a,
        locals::key::K_ESCAPE,
        locals::key::K_UP,
        locals::key::K_RETURN,
        locals::key::K_SPACE,
        locals::key::K_z,
    ];

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 键帽阵 3×2；当前按下 = 高亮
                let pressed = key::get_pressed();
                for (i, k) in keys.iter().enumerate() {
                    let (x, y) = (
                        200.0 + (i % 3) as f32 * 140.0,
                        220.0 + (i / 3) as f32 * 100.0,
                    );
                    let c = if pressed.get(*k) {
                        [0.9412, 0.7843, 0.2353, 1.0000]
                    } else {
                        [0.3529, 0.3922, 0.5882, 1.0000]
                    };
                    b.push_quad(x, y, 110.0, 70.0, c, None);
                }
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
    common::log(&format!("[locals] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
