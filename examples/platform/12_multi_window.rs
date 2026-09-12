//! 多窗口演示：主窗（暗红）+ 运行时创建的第二窗（暗蓝），各自独立渲染
//!
//! 多窗口模型（句柄形式，与资源惰性句柄同构）：
//! - 主窗由 `run` 的 WindowConfig 声明，start 就绪（启动门等首个有效尺寸）
//! - 动态窗口经 `ctx.create_window(cfg)` 创建——返回 `InitSlot<Window>` 槽位，
//!   下一周期物化并触发 [window_created] 钩子
//! - 事件按窗口路由（[event] 首参即窗口句柄）；每窗关闭独立销毁，
//!   **最后一个窗口关闭 = 应用退出**
//!
//! 渲染适配：多窗口本质 = 多个渲染目标——每窗一个 `RenderSurface`
//! （MVP：各窗独立 wgpu 实例+设备，最简正确；共享设备的升级路径用
//! `RenderEntry::surface_from_context`）。热路径（光栅化）仍在 Rust，
//! Python 层（PyO3）后续以同款句柄模型暴露多窗口。
//!
//! 运行：cargo run --example 12_multi_window

use starfish::base::app::{run, Application, Ctx, InitSlot, WindowConfig};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::settings::{GpuSettings, SurfaceSettings};
use starfish::base::window::{Window, WindowEvent};
use wgpu::Color;

/// 构建窗口的渲染表面（async：两平台统一；MVP：各窗独立实例）
async fn build_surface(window: &Window, clear: Color) -> RenderSurface {
    let (_context, _access, mut surface) =
        RenderEntry::async_new(window, SurfaceSettings::default(), GpuSettings::default())
            .await
            .expect("RenderContext 初始化失败");
    surface.begin_frame(clear, 1.0);
    surface.present();
    surface
}

/// 单窗口单元：窗口句柄 + 异步就绪的表面 + 清屏色
struct Win {
    window: Window,
    surface: InitSlot<RenderSurface>,
    clear: Color,
}

/// 应用状态：窗口注册表（句柄形式，与资源句柄模型同构）
#[derive(Default)]
struct MultiWin {
    windows: Vec<Win>,
}

impl Application for MultiWin {
    /// 首窗就绪：建主窗表面（暗红）+ 运行时创建第二窗口（暗蓝）
    fn start(&mut self, ctx: &mut Ctx) {
        let main = ctx.window().clone();
        let clear = Color { r: 0.45, g: 0.12, b: 0.12, a: 1.0 };

        let slot = InitSlot::new();
        let w = main.clone();
        slot.init(async move { build_surface(&w, clear).await });
        self.windows.push(Win { window: main, surface: slot, clear });

        // 运行时创建第二窗口（句柄形式：下一周期物化 → window_created 钩子）
        let cfg = WindowConfig::new("第二窗口", 400, 300).with_resizable(false);
        ctx.create_window(cfg);
    }

    /// 动态窗口物化后调用：立即开始异步建表面
    fn window_created(&mut self, win: &Window, _ctx: &mut Ctx) {
        let clear = Color { r: 0.12, g: 0.2, b: 0.45, a: 1.0 }; // 次窗暗蓝

        let slot = InitSlot::new();
        let w = win.clone();
        slot.init(async move { build_surface(&w, clear).await });
        self.windows.push(Win { window: win.clone(), surface: slot, clear });
    }

    /// 帧钩子：逐窗口渲染（每窗独立表面 = 独立渲染目标）
    fn frame(&mut self, _ctx: &mut Ctx) {
        for w in &mut self.windows {
            let Some(mut surface) = w.surface.get_mut() else {
                continue; // 表面未就绪（异步构建中）→ 跳过
            };
            surface.begin_frame(w.clear, 1.0);
            surface.present();
        }
    }
}

fn main() {
    run(
        MultiWin::default(),
        WindowConfig::new("主窗口", 800, 600).with_fps_cap(60),
    );
}
