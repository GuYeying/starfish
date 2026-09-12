//! Web 三角形演示：编译期内嵌着色器 + NDC 空间彩色三角形（无相机矩阵）
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 执行顺序总览（编号 [N] 与 web/11.html 里的伪代码注释一一对应，
//! 建议两个文件对照着读）：
//!
//!   [1] (浏览器)   加载 11.html → import 胶水 JS → fetch+编译 wasm → 实例化
//!   [2] (浏览器→Rust) 实例化完成，自动调用导出的 _start → 进入 web_main()
//!   [3] (Rust)     main() → run(app, config)
//!   [4] (Rust)     run() Web 变体：spawn_local(事件循环任务) —— 只是排队！
//!   [5] (Rust→浏览器) main() 返回，init() 完成 —— 控制权回到浏览器
//!                  【此刻：画布空白、GPU 未初始化、winit 记录的尺寸=0×0】
//!   ── 事件循环先跑起来；GPU/服务等待首个真实尺寸（ResizeObserver）──
//!   [6] (浏览器→Rust) 微任务：winit 事件循环任务启动（向 canvas 注册监听）
//!   [7] (浏览器→Rust) resumed 回调：接管 canvas（尺寸 0×0，服务启动暂缓）
//!                     → request_redraw 踢第一脚【启动门开启】
//!                     → winit 抛控制流异常 → Rust 栈退回浏览器
//!   [8] (浏览器)   布局测量完成 → ResizeObserver 触发【T2 线】
//!                  → winit 记录真实尺寸 → 派发 Resized(800,600)
//!   [9] (浏览器→Rust) 启动门通过（尺寸有效）→ start()：
//!                  → RenderEntry 以【正确尺寸】初始化 wgpu/表面
//!                  → spawn_local(build_gpu) 排队【T1 线】→ request_redraw
//!   [10] (浏览器→Rust) 微任务：build_gpu 执行 → await 让出/恢复 → 填入槽位
//!   [11] (浏览器→Rust) rAF 触发 → frame()：
//!                  槽位空 → 跳过本帧；就绪 → 清屏+三角形 → present
//!   [12] 稳定循环：request_redraw → rAF → frame() … 每 vsync 一帧
//!
//! 原理详解见 reference/wasm运行时生命周期与尺寸竞态问题详解.md。
//! ═══════════════════════════════════════════════════════════════════════

use std::sync::Arc;

use bytemuck::cast_slice;

use starfish::base::app::{run, Application, Ctx, InitSlot, WindowConfig};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::shader_module::shader_module::ShaderModule;
use starfish::base::render::RenderContext;
use starfish::base::render::settings::{GpuSettings, SurfaceSettings};
use starfish::base::resources::shader::Shader;
use starfish::base::web::console_log;
use starfish::base::window::{Window, WindowEvent};

const TRIANGLE_WGSL: &str = include_str!("../../resources/shaders/triangle.wgsl");

/// GPU 资源束（一次性构建，长期复用）
struct Gpu {
    _context: RenderContext,
    access: RenderResourceAccess,
    surface: RenderSurface,
    _shader: Arc<ShaderModule>,
    mesh: Mesh,
    _bind_group: BindGroup,
    pipeline: Arc<RenderPipeline>,
}

/// 应用状态：资源槽位（桌面同步填充 / Web 异步填充，代码零 cfg）
#[derive(Default)]
struct App {
    gpu: InitSlot<Gpu>,
}

/// 构建全部 GPU 资源（async：两平台共用同一份构建代码）
async fn build_gpu(window: &Window) -> Gpu {
    let (context, access, mut surface) =
        RenderEntry::async_new(window, SurfaceSettings::default(), GpuSettings::default())
            .await
            .expect("RenderContext 初始化失败");

    // 报告实际选中的后端（webgpu/webgl 自动降级在此可见）
    console_log(&format!(
        "build_gpu: 后端={:?} 设备={}",
        context.adapter_info().backend,
        context.adapter_info().name,
    ));

    // 着色器（编译期内嵌，无文件 IO）
    let shader = access
        .shader_module_builder(Shader::new(TRIANGLE_WGSL.to_string()))
        .build(Some("web_triangle_shader"));

    // 顶点：NDC 空间彩色三角形（无相机矩阵）
    let verts: &[f32] = &[
        0.0, 0.5, 0.0, 1.0, 0.0, 0.0,
        -0.5, -0.5, 0.0, 0.0, 1.0, 0.0,
        0.5, -0.5, 0.0, 0.0, 0.0, 1.0,
    ];
    let layout = vec![
        wgpu::VertexFormat::Float32x3,
        wgpu::VertexFormat::Float32x3,
    ];
    let mesh = access
        .mesh_builder(layout, cast_slice(verts).to_vec())
        .build(Some("web_tri_vb"), None);

    // 空 BindGroup（纯色三角形无纹理）
    let bind_group = access.bind_group_builder().build(Some("web_tri_bind"));

    // 2D 管线（Alpha 混合、无剔除、无深度）
    let pipeline = access
        .render_pipeline_builder_2d(&shader)
        .build(&[&bind_group], &mesh, Some("web_tri_pipeline"));

    Gpu {
        _context: context,
        access,
        surface,
        _shader: shader,
        mesh,
        _bind_group: bind_group,
        pipeline,
    }
}

impl Application for App {
    /// [9] 启动门通过（[8] 的真实尺寸已就位）后调用：
    /// 此处 ctx.size() 已可信，GPU/表面以正确尺寸初始化——尺寸竞态从顺序上消除。
    fn start(&mut self, ctx: &mut Ctx) {
        let slot = self.gpu.clone();
        let window = ctx.window().clone();

        // [9a] 桌面：阻塞跑完立即填入；Web：spawn_local 排队【T1 线启动】
        //      （Web 上排队后立即返回——绝不能阻塞浏览器主线程）
        slot.init(async move { build_gpu(&window).await });
    }

    /// [8]（T2 尺寸线：ResizeObserver 异步触发后，winit 派发 Resized）
    ///
    /// Web 关键：winit 接管 canvas 后初始 inner_size 为 0×0，真实尺寸经
    /// ResizeObserver 异步到达——该事件同时是启动门（触发 [9] start）；
    /// 句柄就绪时在此同步表面尺寸（浏览器窗口变化），未就绪则跳过
    ///（帧循环的尺寸自愈兜底）。
    /// ★ 事件可能先于 start 到达——句柄未就绪时跳过即可。
    fn event(&mut self, _win: &Window, e: &WindowEvent, _ctx: &mut Ctx) {
        if let WindowEvent::Resized { width, height } = e {
            if let Some(mut gpu) = self.gpu.get_mut() {
                gpu.surface.resize(*width, *height);
            }
        }
    }

    /// [10]/[11]（T3 帧线：每个 rAF 由 base 的 request_redraw 链驱动一次）
    fn frame(&mut self, ctx: &mut Ctx) {
        // [10a] Web：build_gpu 尚未完成时（槽位空）静默跳过本帧——这是
        //       异步初始化下的正常状态，不是错误
        let Some(mut gpu) = self.gpu.get_mut() else {
            return;
        };

        // [10b] 尺寸自愈：窗口尺寸与表面配置不一致时（[8] 先于 [9] 到达的
        //       竞态、或任何漏网路径），渲染前对齐——避免 1×1 帧缓冲被
        //       CSS 拉伸成"全屏纯色"（粉/深红的元凶）
        let size = ctx.size();
        if gpu.surface.size() != size {
            gpu.surface.resize(size.0, size.1);
        }

        // [10c] 清屏 + 三角形 + 上屏（以下渲染代码与桌面完全相同）
        gpu.surface.begin_frame(
            wgpu::Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 },
            1.0,
        );
        let color_attachment = gpu.surface.get_current_color_attachment();
        let mut encoder = gpu.access.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass(
            "web_triangle_pass",
            &color_atts,
            None,
            None,
            None,
            None,
        );
        pass.set_pipeline(&gpu.pipeline);
        pass.set_bind_group(0, &gpu._bind_group);
        pass.set_mesh(&gpu.mesh);
        pass.draw(0..gpu.mesh.vertex_count(), 0..1);
        pass.end();
        gpu.surface.submit([encoder.finish()]);
        gpu.surface.present();

        // [11] frame 返回后，base 的 request_redraw 会排下一个 rAF
        //      → 回到 [10]，每 vsync 一帧稳定循环
    }
}

/// [3] main 是两平台共用的入口写法（Web 上由 [2] 的 _start 调到这里）
fn main() {
    run(
        App::default(),
        WindowConfig::new("Web Triangle", 800, 600)
            .with_resizable(false)
            .with_fps_cap(60)
            .with_web_canvas_id("canvas"), // 接管 11.html 里的 <canvas id="canvas">
    );
    // [4] run() 的 Web 变体内部：spawn_local(事件循环任务) 后【立即返回】
    //     ——绝不能阻塞浏览器主线程；[6] 起由浏览器调度继续
}

/// [2] wasm-bindgen 入口：wasm 实例化完成后浏览器/胶水自动调用（导出 _start）；
/// 桌面目标下本宏展开为空。由 starfish::web_entry!() 一行生成，零 cfg。
starfish::web_entry!();
