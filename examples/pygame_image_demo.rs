//! pygame image demo（契约 §十 P4 验收：带文字的精灵 demo，三平台）
//!
//! 验收面：`image::load_from_bytes`（PNG RGBA + JPEG RGB 两条解码路）→
//! Surface → blit 链路；文字上屏（font）；draw 图元叠加。600 帧 ≈ 10 秒
//! 后桌面退出 / web-Android 驻留（同 probe 约定）。
//!
//! 资产经 `include_bytes!` 内嵌（全平台零 IO——Web 无文件系统、Android
//! 资产通道 v2 前的内嵌方案，见 image 模块文档）。

use starfish::base::debug::console_log;
use starfish::base::window::{Event, KeyCode, Window};
use starfish::pygame::font::Font;
use starfish::pygame::image;
use starfish::pygame::render::{DrawTarget, Screen, Surface};

/// 自动退出帧数（600 ≈ 10 秒）
const AUTO_EXIT_FRAMES: u32 = 600;

async fn app_body() -> Result<(), Box<dyn std::error::Error>> {
    let mut window = Window::builder()
        .title("starfish pygame image demo")
        .size((800, 600))
        .build()
        .expect("建窗失败");
    let screen = Screen::new(&window).await.expect("GPU 初始化失败");

    // image.load → Surface（PNG RGBA 与 JPEG RGB 两条解码路）
    let face = image::load_from_bytes(include_bytes!("../resources/textures/awesomeface.png"))
        .expect("awesomeface.png 解码失败");
    let container =
        image::load_from_bytes(include_bytes!("../resources/textures/container.jpg"))
            .expect("container.jpg 解码失败");

    // 文字（font 资源制备路线）
    let font = Font::from_bytes(
        include_bytes!("../resources/fonts/Antonio-Regular.ttf").to_vec(),
        48,
    )
    .expect("字体解析失败");
    let text = font.render("starfish image demo", true, [255, 255, 255, 255]);

    let mut frames: u32 = 0;
    loop {
        while let Some(event) = window.poll_event() {
            match event {
                Event::CloseRequested => return Ok(()),
                Event::KeyDown { key, .. } if key == KeyCode::Escape => return Ok(()),
                Event::Resized { width, height } => screen.resize(width, height),
                _ => {}
            }
        }

        // ── 主会话：全场景合成（统一 RenderPass 形态）──
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.blit(&container, (150, 100)); // JPEG 路
            f.blit(&face, (500, 130)); // PNG 路
            f.blit(&face, (560, 260)); // 同源二次 blit（alpha 混合）
            f.with_batch(|b| {
                b.push_quad(140.0, 420.0, 520.0, 3.0, [0.2, 0.8, 1.0, 1.0], None); // 分隔线
            });
            f.blit(&text, (240, 450));
            f.end();
        }

        screen.present();
        frames += 1;
        // save 验收（帧 30 一次）：合成离屏面 → GPU 回读 → 编码 → IO 落盘
        // （桌面同步壳；Web 用 save_async——字节 POST 到服务端端点落盘）
        #[cfg(not(target_arch = "wasm32"))]
        if frames == 30 {
            let shot = Surface::new((320, 180));
            {
                let f = shot.render();
                f.fill([24, 24, 40, 255]);
                f.blit(&face, (16, 16));
                f.blit(&container, (168, 16));
                f.blit(&text, (16, 110));
                f.end();
            }
            match image::save(&shot, "saved_demo.png") {
                Ok(()) => console_log("[image_demo] 已保存 saved_demo.png"),
                Err(e) => console_log(&format!("[image_demo] 保存失败: {e}")),
            }
        }
        if frames >= AUTO_EXIT_FRAMES {
            console_log(&format!(
                "[image_demo] {AUTO_EXIT_FRAMES} 帧完成，{}",
                if cfg!(target_arch = "wasm32") { "Web 驻留" } else { "正常退出" }
            ));
            // 桌面/Android：真实退出；Web：驻留不返回（同 probe 约定）
            #[cfg(not(target_arch = "wasm32"))]
            return Ok(());
            #[cfg(target_arch = "wasm32")]
            std::future::pending::<()>().await;
        }
        starfish::base::window::next_frame().await;
    }
}

starfish::app_entry!(app_body());
