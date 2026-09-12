//! 示例 04：坐标系（索引化立方体 + MVP 矩阵旋转）
//!
//! 引擎持循环模型：`run` 持有主循环，应用实现 `Application` 三回调：
//! start 建表面/加载立方体资源 → frame 每帧按 `ctx.delta()` 累积旋转、
//! 上传 MVP Uniform 并绘制。点 × 关窗自动退出。
//!
//! 运行：cargo run --example 04_coord_system（需 resources/textures/container.jpg）

use std::{fs, sync::{Arc}};
use bytemuck::{cast_slice};
use image::ImageReader;
use glam::{Mat4, Vec3};
use wgpu::{
    AddressMode, Color, FilterMode, InstanceFlags, MemoryHints, MipmapFilterMode,
    PowerPreference, TextureUsages,
};
use starfish::base::app::{run, Application, Ctx, WindowConfig};
use starfish::base::{
    render::{
        bind_group::{
            bind_group::BindGroup, field_type::StructType, field_value::StructValue,
            struct_layout::StructLayout, uniform_buffer::UniformBuffer,
        }, mesh::mesh::Mesh, pipeline::RenderPipeline, render_entry::RenderEntry,
        render_resource_access::RenderResourceAccess, render_surface::RenderSurface,
        RenderContext, sampler_desc::SamplerDescriptor,
        settings::{GpuSettings, SurfaceSettings},
        shader_module::shader_module::ShaderModule,
        texture::{TextureDescriptor, TextureDim, TextureSemantic, TextureUsage},
    }, resources::{image::ImageData, shader::Shader},
};
use glam::camera::rh::view::look_at_mat4;
use glam::camera::rh::proj::directx::perspective;

/// 窗口参数（与 WindowConfig 保持一致，宽高比据此计算）
const WIN_SIZE: (u32, u32) = (800, 600);

// ===================== 立方体顶点（唯一24个顶点：xyz uv）=====================
// 每个面4个唯一顶点，6面合计24个，通过索引复用
const CUBE_UNIQUE_VERTS: &[f32] = &[

    // ---------- Front (+Z)
    -0.5,-0.5, 0.5, 0.0,0.0,
     0.5,-0.5, 0.5, 1.0,0.0,
     0.5, 0.5, 0.5, 1.0,1.0,
    -0.5, 0.5, 0.5, 0.0,1.0,

    // ---------- Back (-Z)
     0.5,-0.5,-0.5, 0.0,0.0,
    -0.5,-0.5,-0.5, 1.0,0.0,
    -0.5, 0.5,-0.5, 1.0,1.0,
     0.5, 0.5,-0.5, 0.0,1.0,

    // ---------- Left (-X)
    -0.5,-0.5,-0.5, 0.0,0.0,
    -0.5,-0.5, 0.5, 1.0,0.0,
    -0.5, 0.5, 0.5, 1.0,1.0,
    -0.5, 0.5,-0.5, 1.0,1.0,

    // ---------- Right (+X)
     0.5,-0.5, 0.5, 0.0,0.0,
     0.5,-0.5,-0.5, 1.0,0.0,
     0.5, 0.5,-0.5, 1.0,1.0,
     0.5, 0.5, 0.5, 0.0,1.0,

    // ---------- Top (+Y)
    -0.5, 0.5, 0.5, 0.0,0.0,
     0.5, 0.5, 0.5, 1.0,0.0,
     0.5, 0.5,-0.5, 1.0,1.0,
    -0.5, 0.5,-0.5, 0.0,1.0,

    // ---------- Bottom (-Y)
    -0.5,-0.5,-0.5, 0.0,0.0,
     0.5,-0.5,-0.5, 1.0,0.0,
     0.5,-0.5, 0.5, 1.0,1.0,
    -0.5,-0.5, 0.5, 0.0,1.0,

];
// ===================== 立方体索引数组 u16 =====================
// 每个面2个三角形(6索引)，6个面合计36索引，对应上面24个唯一顶点
const CUBE_INDICES: &[u16] = &[
    // Front
    0, 1, 2,
    0, 2, 3,
    // Back
    4, 5, 6,
    4, 6, 7,
    // Left
    8, 9,10,
    8,10,11,
    // Right
    12,13,14,
    12,14,15,
    // Top
    16,17,18,
    16,18,19,
    // Bottom
    20,21,22,
    20,22,23,
];//98-32 = 60

struct CubeApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    _shader: Option<Arc<ShaderModule>>,
    mesh: Option<Mesh>,
    tex_bind_group: Option<BindGroup>,
    matrix_bind_group: Option<BindGroup>,
    mvp_buffer: Option<UniformBuffer>,
    pipeline: Option<Arc<RenderPipeline>>,
}

impl CubeApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            _shader: None,
            mesh: None,
            tex_bind_group: None,
            matrix_bind_group: None,
            mvp_buffer: None,
            pipeline: None,
        }
    }
}

impl Application for CubeApp {
    fn start(&mut self, ctx: &mut Ctx) {
        // 渲染上下文配置
        let surface_settings = SurfaceSettings::default()
            .with_usage(TextureUsages::RENDER_ATTACHMENT)
            .with_frame_latency(2);
        let gpu_settings = GpuSettings::default()
            .with_power_preference(PowerPreference::LowPower)
            .with_flags(InstanceFlags::empty())
            .with_memory_hints(MemoryHints::MemoryUsage)
            .with_depth(true); // 3D深度缓冲开启

        // 渲染上下文（阻塞式创建；ctx.window() 即引擎建好的窗口）
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), Some(surface_settings), Some(gpu_settings))
                .expect("RenderContext 初始化失败");
        // ===================== 1. 着色器模块 =====================
        let shader_source = fs::read_to_string("resources/shaders/coord_system.wgsl").unwrap();
        let shader = resouce.shader_module_builder(Shader::new(shader_source))
            .build(Some("cube_transform_shader"));
        // ===================== 2. 网格构建【核心：传入索引缓冲】 =====================
        // 顶点布局：Position(f32x3) + UV(f32x2)
        let vertex_layout = vec![
            wgpu::VertexFormat::Float32x3,
            wgpu::VertexFormat::Float32x2,
        ];
        // 关键：with_short_indices 传入索引字节切片，绘制时框架自动识别 indexed draw
        let cube_mesh = resouce.mesh_builder(vertex_layout, cast_slice(CUBE_UNIQUE_VERTS).to_vec())
            .with_short_indices(cast_slice(CUBE_INDICES).to_vec())
            .build(Some("cube_indexed_mesh"), None);
        // ===================== 3. 纹理 + 采样器 + 纹理BindGroup =====================
        let texture_desc = TextureDescriptor::new(
            TextureSemantic::Color,
            TextureUsage::Sampled,
            TextureDim::D2,
            None,
            None,
        );
        // 加载纹理
        let img = ImageReader::open("resources/textures/container.jpg")
            .unwrap()
            .decode()
            .unwrap()
            .into_rgba8();
        let image = ImageData::Rgba8(img);
        let texture = Arc::new(resouce.create_texture("cube_container_tex", &image, texture_desc));
        // 线性重复采样器
        let sampler_config = SamplerDescriptor::new(
            FilterMode::Linear,
            FilterMode::Linear,
            MipmapFilterMode::Linear,
            AddressMode::Repeat,
            AddressMode::Repeat,
            AddressMode::Repeat,
            None,
        );
        let sampler = Arc::new(resouce.create_sampler("cube_linear_sampler", &sampler_config));
        // 纹理采样器绑定组
        let tex_bind_group = resouce.bind_group_builder()
            .texture(0, texture)
            .sampler(1, sampler)
            .build(Some("tex_sampler_bind_group"));
        // ===================== 4. MVP Uniform缓冲 + 矩阵BindGroup =====================
        let binding_layout = Arc::new(StructLayout::new(&[StructType::Mat4,StructType::Mat4,StructType::Mat4,]));
        let mvp_buffer: UniformBuffer = resouce.create_uniform_buffer(&binding_layout);
        let matrix_bind_group = resouce.bind_group_builder()
            .uniform(0, &mvp_buffer)
            .build(Some("mvp_uniform_bind_group"));
        // ===================== 5. 3D渲染管线 =====================
        let pipeline = resouce.render_pipeline_builder_3d(&shader)
            .build(&[&tex_bind_group, &matrix_bind_group], &cube_mesh, Some("cube_3d_render_pipeline"));

        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);
        self._shader = Some(shader);
        self.mesh = Some(cube_mesh);
        self.tex_bind_group = Some(tex_bind_group);
        self.matrix_bind_group = Some(matrix_bind_group);
        self.mvp_buffer = Some(mvp_buffer);
        self.pipeline = Some(pipeline);
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let surface = self.surface.as_mut().unwrap();
        let resouce = self.resouce.as_ref().unwrap();
        let pipeline = self.pipeline.as_ref().unwrap();
        let cube_mesh = self.mesh.as_ref().unwrap();
        let tex_bind_group = self.tex_bind_group.as_ref().unwrap();
        let matrix_bind_group = self.matrix_bind_group.as_ref().unwrap();
        let mvp_buffer = self.mvp_buffer.as_mut().unwrap();

        // 相机&矩阵计算（帧间隔来自 ctx.delta()，替代原 Clock::tick 返回值）
        let time = ctx.delta();
        let camera_pos = Vec3::new(0.0, 0.0, 3.0);
        let view = look_at_mat4(camera_pos, Vec3::ZERO, Vec3::Y);
        let fov = std::f32::consts::PI / 4.0;
        let aspect = WIN_SIZE.0 as f32 / WIN_SIZE.1 as f32;
        let proj = perspective(fov, aspect, 0.01, 500.0);

        // 模型旋转
        let mut model = Mat4::IDENTITY;
        model *= Mat4::from_rotation_y(time);
        model *= Mat4::from_rotation_x(time * 0.7);
        //设置槽位数值
        mvp_buffer.set(0, StructValue::Mat4(model));
        mvp_buffer.set(1, StructValue::Mat4(view));
        mvp_buffer.set(2, StructValue::Mat4(proj));
        //将数据上传到gpu
        resouce.update_uniform_buffer(mvp_buffer);
        // 帧开始
        surface.begin_frame(Color::BLACK,1.0);

        let depth_attachment = surface.get_current_depth_attachment();
        let color_attachment = surface.get_current_color_attachment();
        // 绘制指令编码
        let mut encoder_draw = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut render_pass = encoder_draw.begin_render_pass(
                "cube_indexed_render_pass",
                &color_atts,
                depth_attachment,
                None,
                None,
                None,
            );
            render_pass.set_pipeline(pipeline);
            render_pass.set_bind_groups(&[(0, tex_bind_group), (1, matrix_bind_group)]);
            // 框架内部自动识别索引缓冲，执行 indexed draw，无需修改draw调用
            render_pass.draw_mesh(cube_mesh);
            render_pass.end();
        let cmd = encoder_draw.finish();
        surface.submit([cmd]);
        surface.present();
    }
}

fn main() {
    // 帧率控制：120 FPS（with_fps_cap 替代原 Clock::tick 节流）
    run(
        CubeApp::new(),
        WindowConfig::new("Indexed Cube 3D Demo", WIN_SIZE.0, WIN_SIZE.1)
            .with_resizable(false)
            .with_fps_cap(120),
    );
}
