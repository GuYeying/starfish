//! starfish-rewrite hello：统一 async 应用体，三平台同一份代码。
//!
//! 结构对齐 Python asyncio 帧循环：
//! ```python
//! async def main():
//!     ...init...
//!     while True:
//!         pygame.event.get()
//!         pygame.display.flip()
//!         await asyncio.sleep(0)   ← next_frame().await
//! asyncio.run(main())
//! ```
//! 桌面：pollster::block_on 同步驱动（next_frame = no-op async）。
//! Web：spawn_local 驱动（next_frame = rAF yield）。

use starfish::base::render::render_entry::RenderEntry;
use starfish::base::window::next_frame;
use starfish::base::window::{Event, Window};
use wgpu::Color;

const CLEAR: Color = Color { r: 0.0, g: 1.0, b: 0.0, a: 1.0 }; // 纯绿（用户要求） // 纯绿（用户要求：易于辨认渲染是否工作）

async fn app_body() -> Result<(), Box<dyn std::error::Error>> {
    let mut window = Window::builder()
        .title("starfish-rewrite hello")
        .size((800, 600))
        .build()
        .expect("建窗失败");

    let (_context, _access, mut surface) = RenderEntry::async_new(
        &window,
        Default::default(),
        Default::default(),
    )
    .await
    .expect("GPU 初始化失败");

    let mut frames: u32 = 0;
    loop {
        while let Some(event) = window.poll_event() {
            match event {
                Event::CloseRequested => return Ok(()),
                Event::Resized { width, height } => surface.resize(width, height),
                _ => {}
            }
        }
        surface.begin_frame(CLEAR, 1.0);
        surface.present();
        frames += 1;
        if frames % 120 == 0 {
            starfish::base::debug::console_log(&format!("[hello] 帧 {frames}"));
        }
        // 帧拍（桌面: no-op async；web: rAF yield）
        next_frame().await;
    }
}

// 平台入口：宏内化三平台驱动器（桌面 block_on+exit / web start 属性+
// spawn_local+panic hook / Android android_init→main 桥接）。
// 错误出口：表达式形态，Err 统一 console_log 后干净退出。
starfish::app_entry!(app_body());
