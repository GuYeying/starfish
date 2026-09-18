//! Android 三角形渲染验证（02_triangles 的 Android 入口变体）
//!
//! 构建与运行（一键，NativeActivity 无 Java 模板）：
//! 1. `./scripts/android_run_example.sh 02_triangles_android`
//! 2. 脚本完成：cargo ndk 编译 → aapt2 打包 → apksigner 签名 → adb 安装启动
//!
//! 分步说明与原理见 `reference/android构建与运行指南.md`。

/// Android 入口（系统 NativeActivity 经 android-activity 调此函数）
///
/// `AndroidApp` 类型来自 `android-activity` crate（经 winit 再导出）。
/// starfish 的 `run_android` 封装：用 AndroidApp 创建 winit EventLoop
/// （`with_android_app`）→ 创建原生窗口 Surface → start/event/frame 回调
/// 与桌面完全一致。
#[cfg(target_os = "android")]
mod entry {
    use starfish::base::app::{run_android, Application, Ctx, WindowConfig};
    use starfish::base::render::render_entry::RenderEntry;
    use starfish::base::render::render_resource_access::RenderResourceAccess;
    use starfish::base::render::render_surface::RenderSurface;
    use starfish::base::render::RenderContext;
    use starfish::base::render::mesh::mesh::Mesh;
    use starfish::base::render::bind_group::bind_group::BindGroup;
    use starfish::base::render::pipeline::RenderPipeline;
    use starfish::base::render::shader_module::shader_module::ShaderModule;
    use starfish::base::resources::shader::Shader;
    use starfish::base::window::Window;

    // edition 2024：no_mangle 是 unsafe 属性，必须写 #[unsafe(no_mangle)]
    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        // panic backtrace 进 logcat（stderr → RustStdoutStderr tag），崩溃排查用
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        // log→logcat 桥：wgpu/hal 的 log::error!（如 surface configure 失败详情）
        // 无桥时在 Android 上完全不可见。
        // ⚠️ x86_64 原生模拟器上启用；arm64 Berberis 转译下 android_logger 的
        // jni find_class 会踩 jni-0.22 异常断言 panic（实测），转译环境禁用
        // android_logger::init_once(
        //     android_logger::Config::default()
        //         .with_max_level(log::LevelFilter::Info)
        //         .with_tag("RustLog"),
        // );

        let game = TriangleGame::new();
        run_android(
            app,
            game,
            WindowConfig::new("starfish 三角形", 800, 600).with_fps_cap(60),
        );
    }

    /// 三角形游戏（同 02_triangles 的逻辑）
    struct TriangleGame {
        _context: Option<RenderContext>,
        resouce: Option<RenderResourceAccess>,
        surface: Option<RenderSurface>,
        _shader: Option<std::sync::Arc<ShaderModule>>,
        mesh: Option<Mesh>,
        _bind_group: Option<BindGroup>,
        pipeline: Option<std::sync::Arc<RenderPipeline>>,
    }

    impl TriangleGame {
        fn new() -> Self {
            Self {
                _context: None,
                resouce: None,
                surface: None,
                _shader: None,
                mesh: None,
                _bind_group: None,
                pipeline: None,
            }
        }
    }

    impl Application for TriangleGame {
        fn start(&mut self, ctx: &mut Ctx) {
            let (context, resouce, mut surface) =
                RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

            // 三角形顶点数据（同 02_triangles）
            let verts: &[f32] = &[
                0.0, 0.5, 0.0,
                -0.5, -0.5, 0.0,
                0.5, -0.5, 0.0,
            ];
            let colors: &[f32] = &[
                1.0, 0.0, 0.0,
                0.0, 1.0, 0.0,
                0.0, 0.0, 1.0,
            ];
            let mut all = Vec::new();
            all.extend_from_slice(verts);
            all.extend_from_slice(colors);

            let shader_code = r#"
struct VertexOutput {
    @builtin(position) pos: vec4f,
    @location(0) color: vec3f,
};
@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    let positions = array(
        vec2f(0.0, 0.5),
        vec2f(-0.5, -0.5),
        vec2f(0.5, -0.5),
    );
    let colors = array(
        vec3f(1.0, 0.0, 0.0),
        vec3f(0.0, 1.0, 0.0),
        vec3f(0.0, 0.0, 1.0),
    );
    var out: VertexOutput;
    out.pos = vec4f(positions[idx], 0.0, 1.0);
    out.color = colors[idx];
    return out;
}
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    return vec4f(in.color, 1.0);
}
"#;

            let shader = resouce.shader_module_builder(Shader::new(shader_code.to_string()))
                .build(Some("android_triangle_shader"));

            let mesh = resouce.mesh_builder(
                vec![
                    wgpu::VertexFormat::Float32x3, // pos
                    wgpu::VertexFormat::Float32x3, // color
                ],
                bytemuck::cast_slice(all.as_slice()).to_vec(),
            ).build(Some("android_triangle_mesh"), None);

            let bind_group = resouce.bind_group_builder().build(Some("android_triangle_bind"));
            let pipeline = resouce.render_pipeline_builder_2d(&shader)
                .build(&[&bind_group], &mesh, Some("android_triangle_pipeline"));

            self._context = Some(context);
            self.resouce = Some(resouce);
            self.surface = Some(surface);
            self._shader = Some(shader);
            self.mesh = Some(mesh);
            self._bind_group = Some(bind_group);
            self.pipeline = Some(pipeline);
        }

        fn event(&mut self, _win: &Window, _event: &starfish::base::window::event::WindowEvent, _ctx: &mut Ctx) {}

        fn frame(&mut self, _ctx: &mut Ctx) {
            let Some(surface) = self.surface.as_mut() else { return };
            let Some(resouce) = &self.resouce else { return };
            let Some(pipeline) = &self.pipeline else { return };
            let Some(mesh) = &self.mesh else { return };
            let Some(bind_group) = &self._bind_group else { return };

            surface.begin_frame(wgpu::Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }, 1.0);
            let color_attachment = surface.get_current_color_attachment();
            let mut encoder = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut pass = encoder.begin_render_pass(
                "android_triangle",
                &color_atts,
                None,
                None,
                None,
                None,
            );
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group);
            pass.set_mesh(mesh);
            pass.draw(0..mesh.vertex_count(), 0..1);
            pass.end();
            surface.submit([encoder.finish()]);
            surface.present();
        }
    }
}

/// 占位入口：cargo-ndk 将 example 以 cdylib 产出（android_main 才是真入口），
/// 但 bin 目标必须存在 main——两个平台都编译它，运行期永远不会被调用。
fn main() {}
