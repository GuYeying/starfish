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
use starfish::pygame::render;
use starfish::pygame::transform;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.transform");
    common::log("[transform] set_mode 完成，进入自检");

    let src = render::Surface::new((32, 16));
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

    // 旋转动画帧：**创建一次**（性能定约——每帧 rotozoom 新建 GPU 表面，
    // 在 ADR-5 延迟提交下曾因 bind 缓存指针键撞键而闪烁；引擎已改唯一
    // id 键，但逐帧建面的分配/上传开销依旧违反定约），循环内只轮换 blit
    let spin_frames: Vec<starfish::pygame::render::Surface> = (0..60)
        .map(|i| transform::rotozoom(&src, i as f32 * 6.0, 2.0))
        .collect();

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
            // rotate 动画帧（预建 60 帧 × 6° 轮换——每秒一整圈）；
            // 旋转包围盒尺寸随角度变化——**中心锚 blit** 原地转
            let spinning = &spin_frames[(frames % spin_frames.len() as u32) as usize];
            f.blit_centered(spinning, (400, 400));
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
