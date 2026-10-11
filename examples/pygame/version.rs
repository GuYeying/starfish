//! pygame.version 运行测试：版本面一致性。
//!
//! 自检项（状态条顺序）：VERSION 非空 / vernum 与 VERSION 一致 /
//! get_sdl_version 兼容值。
//! 场景：版本徽标（版本三元组以色块高度可视化——_major/_minor/patch）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::version;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.version");
    common::log("[version] set_mode 完成，进入自检");

    checks.require("VERSION 非空", !version::VERSION.is_empty());

    // vernum() 应与 VERSION 字符串解析一致（同一事实源）
    let parts: Vec<u8> = version::VERSION
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    let v = version::vernum();
    checks.require(
        "vernum 与 VERSION 一致",
        parts.len() >= 3 && (v.0, v.1) == (parts[0], parts[1]),
    );

    let sdl = version::get_sdl_version();
    checks.require("get_sdl_version 兼容值", sdl == (2, 0, 20));

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 三元组徽标：三根立柱，高度 = 分量 × 40（patch 可 >255 上限截断）
                let bars = [v.0 as f32, v.1 as f32, v.2.min(255) as f32];
                let colors = [
                    [0.9412, 0.3529, 0.3529, 1.0000],
                    [0.9412, 0.7843, 0.2353, 1.0000],
                    [0.3529, 0.6275, 0.9412, 1.0000],
                ];
                for (i, h) in bars.iter().enumerate() {
                    let height = (h * 40.0).clamp(20.0, 380.0);
                    b.push_quad(
                        220.0 + i as f32 * 130.0,
                        460.0 - height,
                        80.0,
                        height,
                        colors[i],
                        None,
                    );
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
    common::log(&format!(
        "[version] {frames} 帧完成，自检 {passed}/{total}（VERSION = {}）",
        version::VERSION
    ));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
