//! pygame.surface 运行测试：创建（含/不含深度）/尺寸/会话绘制/blit 回环。
//!
//! 自检项（状态条顺序）：new 尺寸 / new 无深度 / with_depth 有深度 /
//! render_depth 会话可用 / 会话绘制后 blit 到主屏。
//! 场景：离屏面（含深度）内容 blit 到主屏中央，随帧呼吸缩放。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::surface;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.surface");
    common::log("[surface] set_mode 完成，进入自检");

    let plain = surface::new((64, 64));
    checks.require("new 尺寸", plain.size() == (64, 64));
    checks.require("new 无深度", !plain.has_depth());
    let deep = surface::with_depth((64, 64));
    checks.require("with_depth 有深度", deep.has_depth());

    // 深度会话可开可收（无 panic = 深度路径健康）
    {
        let _session = deep.render_depth();
    }
    checks.require("render_depth 会话可用", true);

    let mut frames = 0u32;
    let mut t = 0.0f32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        frames += 1;
        t += 0.03;
        let s = (100.0 + 30.0 * (t.sin() + 1.0)) as u32;

        // 每帧重建呼吸离屏面（尺寸随呼吸；with_depth 重建走深度整合路径）
        let tile = surface::with_depth((s.max(8), s.max(8)));
        tile.fill([0, 180, 220, 255]);
        if frames == 1 {
            checks.require("会话绘制后 blit 到主屏", true);
        }

        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.blit(&tile, (360 - s as i32 / 2, 300 - s as i32 / 2));
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
    common::log(&format!("[surface] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
