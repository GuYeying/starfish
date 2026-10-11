//! pygame.math 运行测试：插值/映射/平滑函数族。
//!
//! 自检项（状态条顺序）：lerp / clamp / inv_lerp / remap / smoothstep
//! 单调 / smootherstep 端点。
//! 场景：三个缓动曲线点阵（linear/smoothstep/smootherstep 并排——
//! 逐点采样画块，曲线形态直接可视）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::math;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.math");
    common::log("[math] set_mode 完成，进入自检");

    checks.require("lerp", math::lerp(0.0, 10.0, 0.5) == 5.0);
    checks.require(
        "clamp",
        math::clamp(5.0, 0.0, 3.0) == 3.0 && math::clamp(-1.0, 0.0, 3.0) == 0.0,
    );
    checks.require("inv_lerp", math::inv_lerp(0.0, 10.0, 2.5) == 0.25);
    checks.require(
        "remap",
        (math::remap(0.0, 10.0, 100.0, 200.0, 5.0) - 150.0).abs() < 1e-5,
    );
    checks.require(
        "smoothstep 单调",
        math::smoothstep(0.0, 1.0, 0.25) < math::smoothstep(0.0, 1.0, 0.5)
            && math::smoothstep(0.0, 1.0, 0.5) < math::smoothstep(0.0, 1.0, 0.75)
            && math::smoothstep(0.0, 1.0, 0.0) == 0.0
            && math::smoothstep(0.0, 1.0, 1.0) == 1.0,
    );
    checks.require(
        "smootherstep 端点",
        math::smootherstep(0.0, 1.0, 0.0) == 0.0 && math::smootherstep(0.0, 1.0, 1.0) == 1.0,
    );

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 三条缓动曲线并排（64 点采样）
                let curves: [(&str, fn(f32) -> f32, [f32; 4]); 3] = [
                    ("linear", |t| t, [0.3529, 0.6275, 0.9412, 1.0000]),
                    ("smoothstep", |t| math::smoothstep(0.0, 1.0, t), [0.3529, 0.7843, 0.4314, 1.0000]),
                    ("smootherstep", |t| math::smootherstep(0.0, 1.0, t), [0.9412, 0.7059, 0.2353, 1.0000]),
                ];
                for (col, (_name, fns, color)) in curves.iter().enumerate() {
                    for i in 0..=64 {
                        let t = i as f32 / 64.0;
                        let v = fns(t);
                        let x = 120.0 + col as f32 * 220.0 + t * 180.0;
                        let y = 420.0 - v * 240.0;
                        b.push_quad(x, y, 6.0, 6.0, *color, None);
                    }
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
    common::log(&format!("[math] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
