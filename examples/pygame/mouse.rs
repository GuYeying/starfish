//! pygame.mouse 运行测试：光标查询/控制面。
//!
//! 自检项（状态条顺序）：get_pos 返回 / get_pressed 可查询 /
//! set_pos+get_pos 一致（原生平台） / set_visible 回环 / get_focused 可查询。
//! 场景：色块实时跟随光标（真实交互演示）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::mouse;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.mouse");
    common::log("[mouse] set_mode 完成，进入自检");

    let (px, _py) = mouse::get_pos();
    checks.require("get_pos 返回", px >= 0);

    let _pressed = mouse::get_pressed();
    checks.require("get_pressed 可查询", true);

    // 原生平台：warp 光标后位置可读回（Web 光标归属页面，不作此断言）
    if cfg!(not(target_arch = "wasm32")) {
        mouse::set_pos(400, 300);
        checks.require("set_pos+get_pos 一致", mouse::get_pos() == (400, 300));
    } else {
        checks.require("Web 形态：set_pos 不适用", true);
    }

    mouse::set_visible(false);
    let hidden = mouse::get_visible();
    mouse::set_visible(true);
    checks.require("set_visible 回环", !hidden);

    let _ = mouse::get_focused();
    checks.require("get_focused 可查询", true);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        let (mx, my) = mouse::get_pos();
        let (l, m, r) = mouse::get_pressed();
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 光标十字线 + 跟随块（按住变色）
                let c = if l || m || r {
                    [0.9020, 0.2353, 0.2353, 1.0000]
                } else {
                    [0.3529, 0.7843, 0.4314, 1.0000]
                };
                b.push_quad(0.0, my as f32, 800.0, 1.0, [0.2745, 0.3137, 0.4706, 1.0000], None);
                b.push_quad(mx as f32, 0.0, 1.0, 600.0, [0.2745, 0.3137, 0.4706, 1.0000], None);
                b.push_quad(mx as f32 - 12.0, my as f32 - 12.0, 24.0, 24.0, c, None);
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
    common::log(&format!("[mouse] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
