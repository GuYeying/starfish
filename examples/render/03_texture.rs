//! 示例 03：纹理贴图（贴图四边形）
//!
//! 引擎持循环模型：`run` 持有主循环，应用实现 `Application` 三回调：
//! start 加载着色器/网格/纹理/采样器/管线（资源就绪后显示窗口）→
//! frame 每帧绘制贴图四边形。点 × 关窗自动退出。
//!
//! 运行：cargo run --example 03_texture（需 resources/textures/wall.jpg）

use std::{fs, sync::Arc};
use bytemuck::cast_slice;
use image::ImageReader;
use starfish::base::app::{run, Application, Ctx, WindowConfig};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::shader_module::shader_module::ShaderModule;
use starfish::base::render::RenderContext;
use starfish::base::render::sampler_desc::SamplerDescriptor;
use starfish::base::render::settings::{GpuSettings, SurfaceSettings};
use starfish::base::render::texture::{TextureDescriptor, TextureDim, TextureSemantic, TextureUsage};
use starfish::base::resources::image::ImageData;
use starfish::base::resources::shader::Shader;
use wgpu::{Color, InstanceFlags, MemoryHints, PowerPreference, TextureUsages};

struct TextureApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    _shader: Option<Arc<ShaderModule>>,
    mesh: Option<Mesh>,
    _bind_group: Option<BindGroup>,
    pipeline: Option<Arc<RenderPipeline>>,
}

impl TextureApp {
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

impl Application for TextureApp {
    fn start(&mut self, ctx: &mut Ctx) {
        // 渲染上下文配置
        let surface_settings = SurfaceSettings::default()
            // 移除 TEXTURE_BINDING / COPY_SRC，仅保留基础渲染附件，交换链显存最小
            .with_usage(TextureUsages::RENDER_ATTACHMENT)
            // 帧延迟保持2（三缓冲是平衡底线，设1会卡顿，没必要牺牲流畅换少量内存）
            .with_frame_latency(2);
        let gpu_settings = GpuSettings::default()
            .with_power_preference(PowerPreference::LowPower)
            .with_flags(InstanceFlags::empty())
            .with_memory_hints(MemoryHints::MemoryUsage)
            .with_depth(false);

        // 渲染上下文（阻塞式创建；ctx.window() 即引擎建好的窗口）
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), Some(surface_settings), Some(gpu_settings))
                .expect("RenderContext 初始化失败");
        //  1: Shader
        let shader_source: String = fs::read_to_string("resources/shaders/texture.wgsl").unwrap();
        let shader = resouce.shader_module_builder(Shader::new(shader_source))
            .build(Some("shader"));
        // 正方形唯一4顶点，格式：x,y,z r,g,b u,v（每组8个f32不变）
        let square_verts: &[f32] = &[
            // 0 左上
            -0.5,  0.5, 0.0, 1.0,0.0,0.0, 0.0,0.0,
            // 1 右上
            0.5,  0.5, 0.0, 0.0,1.0,0.0, 1.0,0.0,
            // 2 左下
            -0.5, -0.5, 0.0, 0.0,0.0,1.0, 0.0,1.0,
            // 3 右下
            0.5, -0.5, 0.0, 1.0,1.0,0.0, 1.0,1.0,
        ];
        let square_indices: Vec<u32> = vec![0, 3, 1, 0, 2, 3];
        //  2:Mesh
        let layout = vec![
            wgpu::VertexFormat::Float32x3,
            wgpu::VertexFormat::Float32x3,
            wgpu::VertexFormat::Float32x2,
        ];
        let mesh = resouce.mesh_builder(layout, cast_slice(square_verts).to_vec())
                            .with_indices(square_indices)
                            .build(Some("label_vertex"),Some("label_index"));

        //  3,BindGroup
        let texture_desc = TextureDescriptor::new(
            TextureSemantic::Color,
            TextureUsage::Sampled,
            TextureDim::D2,
            None,
            None
        );
        // 创建GPU纹理资源
        let texture = {
            let img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageReader::open("resources/textures/wall.jpg").unwrap().decode().unwrap().into_rgba8();
            let image = ImageData::Rgba8(img);
            Arc::new(resouce.create_texture("test_texture",&image,texture_desc))
        };
        // 创建线性采样器
        let sampler = {
            let sampler_config = SamplerDescriptor::default();
            Arc::new(resouce.create_sampler(
                "linear_sampler",
                &sampler_config
            ))
        };
        let bind_group = resouce.bind_group_builder()
            .texture(0, texture)
            .sampler(1,sampler)
            .build(Some("texture_label"));

        //  4.Pipeline
        let pipeline = resouce.render_pipeline_builder_2d(&shader)
            .build(&[&bind_group], &mesh, Some("pipeline"));

        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);
        self._shader = Some(shader);
        self.mesh = Some(mesh);
        self._bind_group = Some(bind_group);
        self.pipeline = Some(pipeline);

        // 资源就绪，显示窗口（对应原 SDL HIDDEN → set_visible 流程）
        ctx.window().set_visible(true);
    }

    fn frame(&mut self, _ctx: &mut Ctx) {
        let surface = self.surface.as_mut().unwrap();
        let resouce = self.resouce.as_ref().unwrap();
        let pipeline = self.pipeline.as_ref().unwrap();
        let mesh = self.mesh.as_ref().unwrap();
        let bind_group = self._bind_group.as_ref().unwrap();

        surface.begin_frame(Color { r: 0.2, g: 0.3, b: 0.3, a: 1.0 },1.0);//内部创建encoder_draw进行清空内容
        let color_attachment = surface.get_current_color_attachment();
        let _depth_attachment = surface.get_current_depth_attachment();
        let mut encoder_draw = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut pass = encoder_draw.begin_render_pass(
                "render_pass",
                &color_atts,
                None,
                None,
                None,
                None,
            );
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0,bind_group);
            pass.draw_mesh(mesh);
            pass.end();
        let cmd_draw = encoder_draw.finish();
        surface.submit([cmd_draw]);
        surface.present();
    }
}

fn main() {
    // 帧率控制：120 FPS（with_fps_cap 替代原 Clock::tick 节流）
    run(
        TextureApp::new(),
        WindowConfig::new("Texture Demo", 800, 600)
            .with_resizable(false)
            .with_fps_cap(120),
    );
}
