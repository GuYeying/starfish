//! 15_empty_window：跨平台空窗口模板（同一源文件，桌面 + Android 双注册）
//!
//! 用途：新模块验证骨架——把待验证模块的初始化/使用代码填进 `start`/`frame`
//! 的落点注释处，桌面即时迭代，Android 实机验证，共享同一份应用骨架。
//!
//! 运行：
//! - 桌面：`cargo run --example 15_empty_window`
//! - Android：`./scripts/android_run_example.sh 15_empty_window_android`
//!   （同一文件双 `[[example]]` 注册：bin 不能与 cdylib 混用，桌面走 bin 条目、
//!    Android 走 cdylib 条目，平台入口经 cfg 分家）
//!
//! 骨架内置：
//! - 渲染表面创建 + 尺寸自愈（Resized → surface.resize，旋转/分屏不炸）
//! - **后端可视化**：背景色即当前渲染后端（深蓝=Vulkan / 深绿=GLES /
//!   灰=其它）——手机上看不到 logcat，用颜色直接实证后端
//! - `FORCE_BACKEND` 开关：强制单一后端做真机 A/B 实验（见下方常量）

// 强制后端实验开关：None = 自动选择（wgpu 顺序：Vulkan 优先，GLES 兜底）。
// 真机验证 Vulkan 时改为 Some(wgpu::Backends::VULKAN)——若启动即退说明
// 该环境无 Vulkan ICD；Some(wgpu::Backends::GL) 可反向强制 GLES 对照。
const FORCE_BACKEND: Option<wgpu::Backends> = None;

use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::settings::GpuSettings;
use starfish::base::render::RenderContext;
use starfish::base::window::event::WindowEvent;
use starfish::base::window::Window;

// ── 共享应用骨架（平台无关）─────────────────────────────

struct EmptyApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    clear_color: wgpu::Color,
}

impl EmptyApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            clear_color: wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 },
        }
    }
}

impl Application for EmptyApp {
    fn start(&mut self, ctx: &mut Ctx) {
        // 表面 + 上下文（None 表面设置 = 默认；gpu 设置经 FORCE_BACKEND 开关）
        let gpu = FORCE_BACKEND.map(|b| GpuSettings::default().with_backends(b));
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), None, gpu).expect("RenderContext 初始化失败");

        // 后端可视化：AdapterInfo 实测数据 → 背景色编码
        let info = context.adapter_info();
        println!(
            "[15_empty_window] 后端={:?} 适配器=\"{}\" 驱动=\"{}\"",
            info.backend, info.name, info.driver_info
        );
        self.clear_color = match info.backend {
            wgpu::Backend::Vulkan => wgpu::Color { r: 0.05, g: 0.08, b: 0.28, a: 1.0 }, // 深蓝
            wgpu::Backend::Gl => wgpu::Color { r: 0.05, g: 0.22, b: 0.08, a: 1.0 },     // 深绿
            _ => wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 },                     // 灰
        };

        // ════════════════════════════════════════════════════
        // ★ 模块验证落点 1：资源构建
        //   在这里建待验证模块的资源（管线/Mesh/贴图/AudioMixer/字体/视频解码...）
        //   桌面跑通后，同代码经 Android 条目直接实机验证
        // ════════════════════════════════════════════════════

        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        // 尺寸自愈：旋转/分屏/窗口拖拽 → 表面跟随（模块代码一般无需关心）
        if let WindowEvent::Resized { width, height } = event {
            if let Some(surface) = self.surface.as_mut() {
                surface.resize(*width, *height);
            }
        }

        // ════════════════════════════════════════════════════
        // ★ 模块验证落点 2：事件处理（输入/触摸/焦点...）
        // ════════════════════════════════════════════════════
    }

    fn frame(&mut self, _ctx: &mut Ctx) {
        let Some(surface) = self.surface.as_mut() else { return };
        let Some(resouce) = &self.resouce else { return };

        // 背景清屏（颜色 = 后端编码；begin_frame 的色值即 clear 值）
        surface.begin_frame(self.clear_color, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = resouce.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("empty_clear", &color_atts, None, None, None, None);

        // ════════════════════════════════════════════════════
        // ★ 模块验证落点 3：每帧绘制
        //   pass.set_pipeline(..) / pass.set_bind_group(..) / pass.draw(..)
        // ════════════════════════════════════════════════════

        pass.end();
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

// ── Android 入口（cdylib：系统 NativeActivity 调 android_main）──
#[cfg(target_os = "android")]
mod entry {
    use super::EmptyApp;
    use starfish::base::app::{run_android, WindowConfig};

    #[unsafe(no_mangle)] // edition 2024：unsafe 属性必须显式写
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        // panic backtrace 进 logcat（stderr → RustStdoutStderr tag）
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        run_android(
            app,
            EmptyApp::new(),
            WindowConfig::new("empty", 800, 600).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    use starfish::base::app::run;
    run(
        EmptyApp::new(),
        WindowConfig::new("空窗口模板", 800, 600).with_fps_cap(120),
    );
}
