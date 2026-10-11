//! pygame.font 运行测试：字体内嵌加载 + 度量 + 渲染面。
//!
//! 自检项（状态条顺序）：from_bytes 加载 / line_height / size 度量 /
//! render 面 / 双字号渲染（晚登记——第二轮渲染无 panic）。
//! 场景：两行白/黄文字上屏（Antonio-Regular 内嵌，三平台零 IO）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::font::Font;

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.font");
    common::log("[font] set_mode 完成，进入自检");

    let font = Font::from_bytes(
        include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
        28,
    )
    .expect("Antonio-Regular.ttf 解析失败");
    checks.require("from_bytes 加载", true);

    let lh = font.line_height();
    checks.require("line_height", lh > 0);

    let (w, h) = font.size("starfish pygame");
    checks.require("size 度量", w > 0 && h > 0);

    let text = font.render("starfish pygame", true, [255, 255, 240, 255]);
    checks.require("render 面", text.size().0 > 0 && text.size().1 > 0);

    let big = Font::from_bytes(
        include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
        48,
    )
    .expect("48px 字号解析失败");
    let big_text = big.render("starfish", true, [240, 200, 60, 255]);
    // 行高随字号走（宽断言不成立——长字符串小字号可宽于短串大字号）
    checks.require("双字号渲染", big_text.size().1 > text.size().1);

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.blit(&big_text, (120, 160));
            f.blit(&text, (120, 260));
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
        "[font] {frames} 帧完成，自检 {passed}/{total}（line_height={lh}，size={w}×{h}）"
    ));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
