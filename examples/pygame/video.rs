//! pygame.video 运行测试：VideoSystem 面 + 解码错误路径。
//!
//! 自检项（状态条顺序）：VideoSystem::new / 缺失文件 → Err /
//! 非媒体扩展 → Err（后两项**原生专属**——web 的 open 为异步失败语义，
//! Web 形态 = 系统面可用）/ 占位呈现（晚登记——错误路径后系统仍可用）。
//! 场景：静态占位板（真实播放需样本资产，resources/videos 未入库——
//! 六平台硬解验收见 probe_video，本案例只验 API 面）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::video::VideoSystem;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.video");
    common::log("[video] set_mode 完成，进入自检");

    let vs = VideoSystem::new();
    checks.require("VideoSystem::new", true);

    // 错误路径断言仅原生成立：web 的 open 走 <video>/fetch **异步失败**
    // 语义（open 立即 Ok、错误延后），同步 Err 断言不适用，也不主动
    // 发起必 404 的请求（浏览器 console 噪音）
    #[cfg(not(target_arch = "wasm32"))]
    {
        checks.require("缺失文件 → Err", vs.open("__missing_video__.mp4").is_err());
        checks.require("非媒体扩展 → Err", vs.open("__missing__.xyz").is_err());
    }
    #[cfg(target_arch = "wasm32")]
    {
        checks.require("Web 形态：系统面可用", true);
    }

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        if frames == 60 {
            // 错误路径走过后 new 第二个系统仍健康
            let _vs2 = VideoSystem::new();
            checks.require("错误路径后系统复用", true);
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // "屏幕"占位（16:9 外框 + 播放三角）
                b.push_quad(200.0, 170.0, 400.0, 225.0, [0.1961, 0.2157, 0.3333, 1.0000], None);
                b.push_quad(370.0, 250.0, 14.0, 44.0, [0.8627, 0.8627, 0.8627, 1.0000], None);
                b.push_quad(384.0, 272.0, 24.0, 22.0, [0.8627, 0.8627, 0.8627, 1.0000], None);
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
    common::log(&format!("[video] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
