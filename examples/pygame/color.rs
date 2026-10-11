//! pygame.color 运行测试：构造族 + 色彩空间变换。
//!
//! 自检项（状态条顺序）：from_name / new+rgb / from_tuple / normalize /
//! lerp / grayscale / invert / premul_alpha / hsva。
//! 场景：色板行——red / green(名称查表) / lerp 中点 / grayscale / invert。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::color;
use starfish::pygame::display;
use starfish::pygame::Color;

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.02
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.color");
    common::log("[color] set_mode 完成，进入自检");

    // 颜色表装载（src/data/colors.json 编译期内嵌——三平台零 IO）
    color::init_colors(include_str!("../../src/data/colors.json"), true)
        .expect("colors.json 装载失败");

    let red = Color::from_name("red").expect("名称查表 red 失败");
    checks.require(
        "from_name",
        red.normalize().0 >= 0.99 && red.normalize().1 == 0.0 && red.normalize().2 == 0.0,
    );

    let c = Color::new(1, 2, 3, 4);
    checks.require(
        "new+rgb",
        near(c.normalize().0, 1.0 / 255.0) && near(c.normalize().3, 4.0 / 255.0),
    );

    let t = Color::from_tuple((10u8, 20u8, 30u8));
    checks.require(
        "from_tuple",
        near(t.normalize().2, 30.0 / 255.0) && near(t.normalize().3, 1.0),
    );

    checks.require(
        "normalize",
        red.normalize() == (1.0, 0.0, 0.0, 1.0),
    );

    let mid = red.lerp(&Color::rgb(0, 0, 255), 0.5);
    checks.require(
        "lerp",
        near(mid.normalize().0, 0.5) && near(mid.normalize().2, 0.5),
    );

    let g = red.grayscale();
    let gn = g.normalize();
    checks.require("grayscale", near(gn.0, gn.1) && near(gn.1, gn.2));

    let inv = red.invert();
    checks.require(
        "invert",
        inv.normalize().0 == 0.0 && near(inv.normalize().1, 1.0) && near(inv.normalize().2, 1.0),
    );

    let pm = Color::new(255, 0, 0, 128).premul_alpha();
    checks.require(
        "premul_alpha",
        near(pm.normalize().0, 0.5) && near(pm.normalize().3, 128.0 / 255.0),
    );

    let mut h = Color::rgb(255, 0, 0);
    let (hue, sat, val, _a) = h.hsva();
    checks.require("hsva", near(hue, 0.0) && near(sat, 1.0) && near(val, 1.0));

    let green = Color::from_name("green").unwrap_or(Color::rgb(0, 200, 90));
    let gn = green.normalize();
    let gray = grayscale_rgba(&red);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 色板行：red / green(查表) / lerp 中点 / grayscale / invert
                let swatches: [[f32; 4]; 5] = [
                    [1.0000, 0.0000, 0.0000, 1.0000],
                    [gn.0, gn.1, gn.2, gn.3],
                    [0.4980, 0.0000, 0.4980, 1.0000],
                    gray,
                    [0.0000, 1.0000, 1.0000, 1.0000],
                ];
                for (i, rgba) in swatches.iter().enumerate() {
                    b.push_quad(80.0 + i as f32 * 130.0, 240.0, 110.0, 110.0, *rgba, None);
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
    common::log(&format!("[color] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

/// Color → 色板用 [f32;4]（走 normalize，保持单一路径）
fn grayscale_rgba(c: &Color) -> [f32; 4] {
    let n = c.grayscale().normalize();
    [n.0, n.1, n.2, n.3]
}

starfish::app_entry!(run());
