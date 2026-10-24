//! pygame.dialog 运行测试：API 面链接验证（交互流程不自动化）。
//!
//! 自检项（状态条顺序）：API 面链接。
//! 说明：pick_file_start / save_bytes_start 会弹出真实系统对话框——
//! **自动化冒烟绝不触发**（会阻塞等待人工操作）。本案例只验证模块
//! 符号可链接、类型可引用；真实选择/保存流程由人工验收（Android SAF
//! 独立 Activity 生命周期流转见 base::dialog 模块文档）。
//! 场景：静态说明板。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::dialog;

// 类型面引用（编译即证链接；不发起任何对话框）
#[allow(unused_imports)]
use dialog::{DialogError, PickedFile};

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.dialog");
    common::log("[dialog] set_mode 完成（本案例不弹任何系统对话框）");

    checks.require("API 面链接", true);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // "文件对话框"示意（描边框 + 三行占位，纯视觉）
                b.push_quad(240.0, 180.0, 320.0, 200.0, [0.1961, 0.2157, 0.3333, 1.0000], None);
                b.push_quad(260.0, 210.0, 280.0, 16.0, [0.3529, 0.3922, 0.5882, 1.0000], None);
                b.push_quad(260.0, 240.0, 280.0, 16.0, [0.3529, 0.3922, 0.5882, 1.0000], None);
                b.push_quad(440.0, 330.0, 100.0, 30.0, [0.3529, 0.6275, 0.9412, 1.0000], None);
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
        "[dialog] {frames} 帧完成，自检 {passed}/{total}（交互流程留人工验收）"
    ));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
