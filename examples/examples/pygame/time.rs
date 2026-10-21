//! pygame.time 运行测试：Clock tick 节流 + fps 统计 + delay。
//!
//! 自检项（状态条顺序）：Clock::new / delay(0) / tick 间隔合理 /
//! get_fps 建立 / raw_delta 合理（后三项在第 30 帧晚登记——需时序积累）。
//! ⚠️ API 差异：本层 `tick` 返回**秒**（pygame 为毫秒，绑定位换算）。
//! 场景：横条按累计时间匀速左移循环（时间流动的可视证据）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::time;
use starfish_pygame::display;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.time");
    common::log("[time] set_mode 完成，进入自检");

    let mut clock = time::Clock::new();
    checks.require("Clock::new", true);

    time::delay(0);
    checks.require("delay(0)", true);

    let mut frames = 0u32;
    let mut elapsed_s = 0.0f32;
    let mut min_dt = f32::MAX;
    let mut max_dt = 0.0f32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        let dt = clock.tick(60); // 秒（非 pygame 的毫秒）
        elapsed_s += dt;
        if frames >= 2 {
            min_dt = min_dt.min(dt);
            max_dt = max_dt.max(dt);
        }

        // 第 30 帧晚登记：时序统计已积累
        if frames == 30 {
            checks.require("tick 间隔合理", min_dt > 0.0005 && max_dt < 0.25);
            checks.require("get_fps 建立", clock.get_fps() > 0.0);
            let d = clock.raw_delta();
            checks.require("raw_delta 合理", d > 0.0 && d < 0.25);
            common::log(&format!(
                "[time] tick min/max = {:.1}/{:.1} ms, fps = {:.1}",
                min_dt * 1000.0,
                max_dt * 1000.0,
                clock.get_fps()
            ));
        }

        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            let x = elapsed_s * 150.0 % 760.0;
            f.with_batch(|b| {
                b.push_quad(x, 280.0, 40.0, 40.0, [0.9412, 0.7059, 0.2353, 1.0000], None);
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
    common::log(&format!("[time] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
