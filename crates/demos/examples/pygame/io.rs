//! pygame.io 运行测试：文本/二进制读写回环 + 错误路径。
//!
//! 自检项（状态条顺序）：不存在文件读 → Err / exists 误报检查 /
//! 文本回环 / 二进制回环（后两项原生平台专属；Web = POST 后端锚点，
//! 无后端环境只跑错误路径，晚登记"Web 形态"项）。
//! 场景：纯自检——静态状态板。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::io;

const TEXT_PATH: &str = "__pygame_io_txt__.txt";
const BIN_PATH: &str = "__pygame_io_bin__.bin";

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.io");
    common::log("[io] set_mode 完成，进入自检");

    // 错误路径（三平台一致）：缺失文件读取返回 Err 而非 panic
    // （base::io 签名统一 async——桌面/Android 同步实现，await 零成本）
    checks.require(
        "不存在文件读 → Err",
        io::read_text("__pygame_missing__.txt").await.is_err(),
    );
    checks.require(
        "exists 缺失 = false",
        !io::exists("__pygame_missing__.txt").await.unwrap_or(true),
    );

    if cfg!(target_arch = "wasm32") {
        checks.require("Web 形态：仅错误路径自检", true);
    } else {
        // 文本回环（相对路径锚定 base_dir：桌面 = 可执行文件旁）
        let text = "starfish io 回环测试 — line2\n";
        io::write_text(TEXT_PATH, text).await.expect("write_text 失败");
        checks.require(
            "文本回环",
            io::exists(TEXT_PATH).await.unwrap_or(false)
                && io::read_text(TEXT_PATH).await.map(|s| s == text).unwrap_or(false),
        );

        // 二进制回环（含 0/255 边界字节）
        let bytes: Vec<u8> = vec![0, 1, 127, 128, 255, 42];
        io::write(BIN_PATH, bytes.clone()).await.expect("write 失败");
        checks.require(
            "二进制回环",
            io::read(BIN_PATH).await.map(|b| b == bytes).unwrap_or(false),
        );
        common::log("[io] 原生回环完成（产物留在 base_dir，便于人工复核）");
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
                // 静态状态板底纹（自检块由 draw_status 呈现）
                for i in 0..5 {
                    b.push_quad(
                        120.0 + i as f32 * 115.0,
                        300.0,
                        90.0,
                        26.0,
                        [0.2353, 0.2745, 0.4314, 1.0000],
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
    common::log(&format!("[io] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
