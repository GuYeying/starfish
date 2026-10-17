//! pygame.mask 运行测试：位面操作族 + 重叠判定 + 双构建路径。
//!
//! 自检项（状态条顺序）：new+set_at/get_at / count / invert / scale /
//! overlap / overlap_area / overlap_mask / to_surface / 影子构建(同步) /
//! 回读一致性(影子 vs 回读，原生)或 GPU 出生 async(web)。
//! 场景：两个 8×8 位面的重叠关系可视化（绿=基准 / 黄=偏移 4px 的对比面
//! / 白=overlap 首个交点）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::mask::Mask;
use starfish::pygame::transform;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.mask");
    common::log("[mask] set_mode 完成，进入自检");

    let mut m1 = Mask::new((8, 8), false);
    m1.set_at(0, 0, true);
    checks.require("new+set_at/get_at", m1.get_at(0, 0) && !m1.get_at(1, 1));
    checks.require("count", m1.count() == 1);

    m1.invert();
    checks.require("invert", m1.count() == 63);

    let scaled = m1.scale((4, 4));
    checks.require("scale", scaled.size() == (4, 4));

    // 满面对满面、右移 4px：重叠区 x∈[4,8)，首个交点 (4,0)，面积 8×4
    let full_a = Mask::new((8, 8), true);
    let full_b = Mask::new((8, 8), true);
    checks.require("overlap", full_a.overlap(&full_b, (4, 0)) == Some((4, 0)));
    checks.require("overlap_area", full_a.overlap_area(&full_b, (4, 0)) == 32);

    let om = full_a.overlap_mask(&full_b, (4, 0));
    checks.require("overlap_mask", om.size() == (8, 8));

    let ts = full_a.to_surface();
    checks.require("to_surface", ts.size() == (8, 8));

    // CPU 出生面（from_rgba8 → 自带 alpha 影子）：from_surface **三端
    // 同步零回读**——8×8 不透明白、末像素全透明 → count = 63
    let mut px = vec![255u8; 8 * 8 * 4];
    if let Some(last) = px.chunks_exact_mut(4).last() {
        last.copy_from_slice(&[0, 0, 0, 0]);
    }
    let shadow_surf = starfish::pygame::render::Surface::from_rgba8((8, 8), &px);
    let m_shadow = Mask::from_surface(&shadow_surf, 1).expect("影子构建失败");
    checks.require("影子构建(同步)", m_shadow.count() == 63 && m_shadow.size() == (8, 8));

    // GPU 出生面（transform 输出，无影子）→ 回读路径；位面应与影子一致
    let gpu_born = transform::scale(&shadow_surf, (8, 8));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let m_rb = Mask::from_surface(&gpu_born, 1).expect("回读构建失败");
        common::log(&format!(
            "[mask] 影子 count = {}，回读 count = {}，回读(7,7) = {}",
            m_shadow.count(),
            m_rb.count(),
            m_rb.get_at(7, 7)
        ));
        checks.require(
            "回读一致性(影子 vs 回读)",
            m_rb.count() == m_shadow.count() && !m_rb.get_at(7, 7),
        );
    }
    #[cfg(target_arch = "wasm32")]
    {
        let m_async = Mask::from_surface_async(&gpu_born, 1)
            .await
            .expect("async 构建失败");
        checks.require(
            "GPU 出生 async",
            m_async.count() == m_shadow.count() && !m_async.get_at(7, 7),
        );
    }

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 基准满面（绿）与右移对比面（黄）与交点（白）
                b.push_quad(320.0, 240.0, 160.0, 160.0, [0.0000, 0.7843, 0.3529, 0.4706], None);
                b.push_quad(400.0, 240.0, 160.0, 160.0, [0.8627, 0.7843, 0.2353, 0.4706], None);
                b.push_quad(480.0, 240.0, 10.0, 10.0, [0.9412, 0.9412, 0.9412, 1.0000], None);
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
    common::log(&format!("[mask] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
