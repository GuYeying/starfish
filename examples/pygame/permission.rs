//! pygame.permission 运行测试：权限申请面（非阻塞项）。
//!
//! 自检项（状态条顺序）：ensure(Internet) / ensure_async(Internet) /
//! 枚举 Debug 面。
//! 平台语义（模块文档）：Internet = normal 权限——桌面直通 / Web 委派 /
//! Android 清单声明即授予，**三平台均非阻塞**，可自动化。
//! 注意：Microphone = dangerous 权限（Android 弹系统授权框阻塞等待），
//! **自动化冒烟绝不触碰**。
//! 场景：静态盾形徽标。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::permission::{self, Permission};

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.permission");
    common::log("[permission] set_mode 完成，进入自检");

    checks.require("ensure(Internet)", permission::ensure(Permission::Internet));

    let ok = permission::ensure_async(Permission::Internet).await;
    checks.require("ensure_async(Internet)", ok);

    checks.require(
        "枚举 Debug 面",
        format!("{:?}", Permission::Microphone).len() > 0,
    );

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 盾形徽标（梯形身 + 矩形柄）
                b.push_quad(340.0, 200.0, 120.0, 130.0, [0.3529, 0.6275, 0.9412, 1.0000], None);
                b.push_quad(360.0, 330.0, 80.0, 40.0, [0.2745, 0.5098, 0.8235, 1.0000], None);
                b.push_quad(385.0, 230.0, 30.0, 70.0, [0.8627, 0.9020, 0.9804, 1.0000], None);
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
    common::log(&format!("[permission] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
