//! RGB 三角形（base 新循环模型规范示例）
//!
//! 循环模型：引擎持循环（`run`），应用实现 `Application` 三回调：
//! start 建表面/建资源 → event 收平台事件 → frame 更新+渲染。
//! 窗口关闭（点 ×）自动退出；`WindowConfig::with_fps_cap` 节流。
//!
//! 运行：cargo run --example 02_triangles

use std::fs;
use bytemuck::cast_slice;

use starfish::base::app::{run, Application, Ctx, WindowConfig};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::shader_module::shader_module::ShaderModule;
use starfish::base::resources::shader::Shader;
use wgpu::Color;

struct TriangleApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    _shader: Option<std::sync::Arc<ShaderModule>>,
    mesh: Option<Mesh>,
    _bind_group: Option<BindGroup>,
    pipeline: Option<std::sync::Arc<RenderPipeline>>,
}

impl TriangleApp {
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

impl Application for TriangleApp {
    fn start(&mut self, ctx: &mut Ctx) {
        // 渲染上下文（阻塞式创建；ctx.window() 即引擎建好的窗口）
        let (context, resouce, mut surface) = RenderEntry::new(ctx.window(), None, None)
            .expect("RenderContext 初始化失败");

        // 1. 纯色三角形着色器（无贴图）
        let shader_source: String = fs::read_to_string("resources/shaders/triangle.wgsl").unwrap();
        let shader = resouce
            .shader_module_builder(Shader::new(shader_source))
            .build(Some("triangle_shader"));
        // 2. 三角形顶点数据：pos xyz + color rgb，共3个顶点，无索引
        let tri_verts: &[f32] = &[
            0.0,  0.5, 0.0, 1.0, 0.0, 0.0,
            -0.5, -0.5, 0.0, 0.0, 1.0, 0.0,
            0.5, -0.5, 0.0, 0.0, 0.0, 1.0,
        ];
        // 顶点布局：仅位置+颜色，无UV
        let layout = vec![wgpu::VertexFormat::Float32x3, wgpu::VertexFormat::Float32x3];
        // 构建Mesh：不使用索引缓冲区
        let mesh = resouce
            .mesh_builder(layout, cast_slice(tri_verts).to_vec())
            .build(Some("tri_vb"), Some("tri_ib"));
        // 3. 材质：不需要纹理、采样器
        let bind_group = resouce
            .bind_group_builder()
            .build(Some("tri_bind_group"));
        // 4. 2D管线（无深度）
        let pipeline = resouce
            .render_pipeline_builder_2d(&shader)
            .build(&[&bind_group], &mesh, Some("tri_pipeline"));

        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);
        self._shader = Some(shader);
        self.mesh = Some(mesh);
        self._bind_group = Some(bind_group);
        self.pipeline = Some(pipeline);
    }

    fn frame(&mut self, _ctx: &mut Ctx) {
        let surface = self.surface.as_mut().unwrap();
        let resouce = self.resouce.as_ref().unwrap();
        let pipeline = self.pipeline.as_ref().unwrap();
        let mesh = self.mesh.as_ref().unwrap();
        let bind_group = self._bind_group.as_ref().unwrap();

        surface.begin_frame(Color { r: 0.1, g: 0.1, b: 0.15, a: 1.0 }, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder_draw = resouce.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder_draw.begin_render_pass(
            "triangle_pass",
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
        let cmd_draw = encoder_draw.finish();
        surface.submit([cmd_draw]);
        surface.present();
    }
}

fn main() {
    run(
        TriangleApp::new(),
        WindowConfig::new("RGB Triangle Demo", 800, 600)
            .with_resizable(false)
            .with_fps_cap(120),
    );
}
