//! 示例 01：Hello World——最小渲染循环（清屏）
//!
//! 引擎持循环模型：`run` 持有主循环，应用实现 `Application` 三回调：
//! start 建渲染表面 → frame 每帧清屏 + present。点 × 关窗自动退出。
//!
//! 运行：cargo run --example 01_hello_world

use starfish::base::app::{run, Application, Ctx, WindowConfig};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use wgpu::Color;

struct HelloApp {
    _context: Option<RenderContext>,
    _resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
}

impl HelloApp {
    fn new() -> Self {
        Self {
            _context: None,
            _resouce: None,
            surface: None,
        }
    }
}

impl Application for HelloApp {
    fn start(&mut self, ctx: &mut Ctx) {
        // 渲染上下文（阻塞式创建；ctx.window() 即引擎建好的窗口）
        let (context, resouce, surface) = RenderEntry::new(ctx.window(), None, None)
            .expect("RenderContext 初始化失败");
        self._context = Some(context);
        self._resouce = Some(resouce);
        self.surface = Some(surface);
    }

    fn frame(&mut self, _ctx: &mut Ctx) {
        let surface = self.surface.as_mut().unwrap();
        // begin_frame 内部清屏；无绘制指令，直接 present
        surface.begin_frame(Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }, 1.0);
        surface.present();
    }
}

fn main() {
    // 帧率控制：120 FPS（with_fps_cap 替代原 Clock::tick 节流）
    run(
        HelloApp::new(),
        WindowConfig::new("Hello World", 800, 600)
            .with_resizable(false)
            .with_fps_cap(120),
    );
}
