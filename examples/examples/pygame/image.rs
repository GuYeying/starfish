//! pygame.image 运行测试：解码 + 扩展面 + save/load 回环。
//!
//! 自检项（状态条顺序）：load_from_bytes / get_extended / 非法格式 → Err /
//! save→load 回环（原生平台专属；Web 保存走 POST 后端，无后端环境晚登记
//! "Web 形态"项）。
//! 场景：awesomeface 原图 + 放大版上屏。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish_pygame::render::DrawTarget;
use starfish_pygame::display;
use starfish_pygame::image;

const IMG_PATH: &str = "__pygame_test_img__.png";

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.image");
    common::log("[image] set_mode 完成，进入自检");

    let face = image::load_from_bytes(include_bytes!("../../resources/textures/awesomeface.png"))
        .expect("awesomeface.png 解码失败");
    checks.require("load_from_bytes", face.size().0 > 0 && face.size().1 > 0);

    checks.require("get_extended", image::get_extended());

    let (fw, fh) = face.size();
    let garbage = image::load_from_bytes(&[1, 2, 3, 4, 5, 6, 7, 8]);
    checks.require("非法格式 → Err", garbage.is_err());

    // 回环：GPU 回读落盘 → 重新解码 → 尺寸一致（save 为桌面专属；
    // Web 保存走 save_async 的 POST 后端路径，无后端环境不跑）
    #[cfg(not(target_arch = "wasm32"))]
    {
        image::save(&face, IMG_PATH).expect("save 失败");
        let back = image::load(IMG_PATH).expect("save 产物 load 失败");
        checks.require("save→load 回环", back.size() == (fw, fh));
    }
    #[cfg(target_arch = "wasm32")]
    {
        checks.require("Web 形态：仅解码自检", true);
    }

    let big = starfish_pygame::transform::scale(&face, (fw * 3, fh * 3));

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.blit(&face, (160, 240));
            f.blit(&big, (360, 180));
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
        "[image] {frames} 帧完成，自检 {passed}/{total}（源 {fw}×{fh}）"
    ));
    common::reside().await;
    Ok(())
}

starfish_std::app_entry!(run());
