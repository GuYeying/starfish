//! pygame.rect 运行测试：构造/几何变换/碰撞族。
//!
//! 自检项（状态条顺序）：new / center / move_ / inflate / collidepoint /
//! colliderect / contains / clamp。
//! 场景：自检涉及的矩形按坐标可视化（绿=r / 紫=move_ / 黄=inflate /
//! 蓝=clamp 目标 / 白=clamp 结果）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::rect::Rect;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.rect");
    common::log("[rect] set_mode 完成，进入自检");

    let r = Rect::new(100, 100, 200, 150);
    checks.require(
        "new",
        r.left == 100 && r.top == 100 && r.width == 200 && r.height == 150,
    );

    let c = r.center();
    checks.require("center", c == (200, 175));

    let m = r.move_(30, -20);
    checks.require("move_", m.left == 130 && m.top == 80 && m.width == 200 && m.height == 150);

    let inf = r.inflate(40, 30);
    checks.require(
        "inflate",
        inf.width == 240 && inf.height == 180 && inf.center() == (200, 175),
    );

    checks.require(
        "collidepoint",
        r.collidepoint((150, 150)) && !r.collidepoint((50, 50)),
    );
    checks.require(
        "colliderect",
        r.colliderect(&m) && !r.colliderect(&Rect::new(400, 400, 50, 50)),
    );
    checks.require(
        "contains",
        r.contains(Rect::new(120, 120, 50, 40)) && !r.contains(Rect::new(350, 120, 50, 40)),
    );

    // 越界矩形（右/下超出目标界）钳回贴目标边；完全在界内的 clamp 原样返回
    let clamped = Rect::new(750, 550, 100, 80).clamp(Rect::new(0, 0, 800, 600));
    checks.require("clamp", clamped.left == 700 && clamped.top == 520);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // clamp 目标界（蓝，全屏描边）与结果（白）
                let t = 6.0;
                b.push_quad(0.0, 0.0, 800.0, t, [0.1569, 0.3529, 0.8627, 1.0000], None);
                b.push_quad(0.0, 600.0 - t, 800.0, t, [0.1569, 0.3529, 0.8627, 1.0000], None);
                b.push_quad(0.0, 0.0, t, 600.0, [0.1569, 0.3529, 0.8627, 1.0000], None);
                b.push_quad(800.0 - t, 0.0, t, 600.0, [0.1569, 0.3529, 0.8627, 1.0000], None);
                b.push_quad(
                    clamped.left as f32,
                    clamped.top as f32,
                    clamped.width as f32,
                    clamped.height as f32,
                    [0.9412, 0.9412, 0.9412, 1.0000],
                    None,
                );
                // inflate（黄）与 r 本体（绿）与 move_（紫）
                b.push_quad(
                    inf.left as f32,
                    inf.top as f32,
                    inf.width as f32,
                    inf.height as f32,
                    [0.7843, 0.7059, 0.1569, 1.0000],
                    None,
                );
                b.push_quad(
                    r.left as f32,
                    r.top as f32,
                    r.width as f32,
                    r.height as f32,
                    [0.0000, 0.7843, 0.3529, 1.0000],
                    None,
                );
                b.push_quad(
                    m.left as f32,
                    m.top as f32,
                    m.width as f32,
                    m.height as f32,
                    [0.6275, 0.2353, 0.7843, 1.0000],
                    None,
                );
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
    common::log(&format!("[rect] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
