//! multi_window：多窗口自检（feature `multi-window`，平台白名单当前仅 Windows）。
//!
//! 运行（链接类操作须在 VS 开发者环境，见仓库 CLAUDE.md）：
//! ```bash
//! scripts\vs-cargo.bat run -p demos --features multi-window --example multi_window
//! ```
//!
//! 形态 = 上下文/表面分离模型（2026-10-23 定稿）：**一个 RenderContext
//! （设备族，进程一份）+ N 个 RenderSurface（每窗一个）**，surface 之外
//! 一切不重建。两窗各自独立事件队列与交换链，关其一另一窗继续渲染，
//! 全关退出。
//!
//! 事件拓扑：从哪个 `Window` poll 出的事件就归属谁（无 ID 路由）；每帧
//! 须轮询每个窗口（漏 poll 的窗口事件积压在自己 pending，不丢失）。

use starfish_render::render_entry::RenderEntry;
use starfish_render::render_surface::RenderSurface;
use starfish_we::next_frame;
use starfish_we::{Event, Window};
use wgpu::Color;

/// 窗口 + 它的表面。字段序 = drop 序：surface 先于 window 销毁
/// （wgpu surface 不得在窗口销毁后 present——'static 句柄契约）。
struct Win {
    surface: RenderSurface,
    window: Window,
    /// 本窗清屏色（两窗不同色 = 一眼确认各表面独立上屏）
    clear: Color,
}

async fn app_body() -> Result<(), Box<dyn std::error::Error>> {
    // 设备族一次（headless 上下文；两窗共享 instance/adapter/device/queue）
    let context = RenderEntry::async_new_context(Default::default()).await?;

    let mut wins: Vec<Win> = Vec::new();
    for (title, size, position, clear) in [
        (
            "multi-window · 主窗（关我，副窗继续跑）",
            (640, 480),
            (80, 120),
            Color { r: 0.1, g: 0.5, b: 0.9, a: 1.0 },
        ),
        (
            "multi-window · 副窗（关我，主窗继续跑）",
            (480, 360),
            (760, 320),
            Color { r: 0.9, g: 0.5, b: 0.1, a: 1.0 },
        ),
    ] {
        let window = Window::builder().title(title).size(size).build()?;
        let surface =
            RenderEntry::async_new_surface(&context, &window, Default::default()).await?;
        window.set_position(position);
        wins.push(Win { surface, window, clear });
    }

    let mut frames: u32 = 0;
    loop {
        // 事件：每帧轮询每个窗口（SDL 风格 poll，归属 = 被 poll 的窗口）
        for i in (0..wins.len()).rev() {
            let mut close = false;
            while let Some(event) = wins[i].window.poll_event() {
                match event {
                    Event::CloseRequested => close = true,
                    Event::Resized { width, height } => wins[i].surface.resize(width, height),
                    _ => {}
                }
            }
            if close {
                // 摘除即销毁：drop 先 surface 后 window（字段序契约）
                wins.swap_remove(i);
            }
        }
        if wins.is_empty() {
            starfish_std::debug::console_log(&format!(
                "[multi_window] 全部窗口已关，共 {frames} 帧"
            ));
            return Ok(());
        }

        // 渲染：每表面独立 begin/present（begin 未就绪返回 false 跳帧）
        for w in &mut wins {
            if w.surface.begin_frame(w.clear, 1.0) {
                w.surface.present();
            }
        }

        frames += 1;
        if frames % 120 == 0 {
            starfish_std::debug::console_log(&format!(
                "[multi_window] 帧 {frames}，存活 {} 窗",
                wins.len()
            ));
        }
        next_frame().await;
    }
}

// 平台入口：宏内化三平台驱动器（桌面 block_on+exit / web start 属性+
// spawn_local+panic hook / Android android_init→main 桥接）。
starfish_std::app_entry!(app_body());
