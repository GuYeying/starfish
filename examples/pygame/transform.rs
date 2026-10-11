//! pygame.transform 运行测试：缩放/旋转/翻转/rotozoom 族。
//!
//! 自检项（状态条顺序）：scale 尺寸 / rotate 90 交换宽高 / flip_x 保持 /
//! flip_y 保持 / rotozoom 放大。
//! 场景：原面与各变换结果一排 blit（红 32×16 源面，尺寸变化直接可视）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::surface;
use starfish::pygame::transform;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.transform");
    common::log("[transform] set_mode 完成，进入自检");

    let src = surface::new((32, 16));
    src.fill([220, 60, 60, 255]);

    let scaled = transform::scale(&src, (64, 32));
    checks.require("scale 尺寸", scaled.size() == (64, 32));

    let rotated = transform::rotate(&src, 90.0);
    checks.require("rotate 90 交换宽高", rotated.size() == (16, 32));

    let fx = transform::flip_x(&src);
    let fy = transform::flip_y(&src);
    checks.require("flip_x 保持", fx.size() == (32, 16));
    checks.require("flip_y 保持", fy.size() == (32, 16));

    let zoomed = transform::rotozoom(&src, 0.0, 2.0);
    checks.require("rotozoom 放大", zoomed.size() == (64, 32));

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            // 一排：原面 / scale / rotate90 / flip_x / flip_y / rotozoom
            f.blit(&src, (60, 280));
            f.blit(&scaled, (160, 272));
            f.blit(&rotated, (300, 264));
            f.blit(&fx, (390, 280));
            f.blit(&fy, (490, 280));
            f.blit(&zoomed, (590, 264));
            // rotate 动画帧（45° 步进展示任意角）
            let ang = (frames % 240) as f32 * 1.5;
            let spinning = transform::rotozoom(&src, ang, 2.0);
            f.blit(&spinning, (300, 380));
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
    common::log(&format!("[transform] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
