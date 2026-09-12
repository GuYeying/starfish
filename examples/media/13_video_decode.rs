//! 视频播放演示：窗口内播放 H.264/MP4（平台硬解 → NV12 → RGBA 纹理 → 全屏绘制）
//!
//! 平台：Windows MF / Linux(Ubuntu) GStreamer / macOS·iOS VideoToolbox。
//! 硬解唯一策略：平台无硬件解码器时 open 直接报 NoHardwareDecoder。
//!
//! 链路：VideoModule（与窗口共享 wgpu 设备）手动泵解码 → 帧纹理 →
//! 采样管线全屏绘制。视频纹理同尺寸覆写稳定（绑定一次管到底）。
//!
//! 资产：resources/videos/sample-5s.mp4（H.264/MP4，1080p）
//! 运行：cargo run --example 13_video_decode
//! 建议 --release：CPU 色彩转换在 debug 下 ~35ms/帧（v2 GPU 转换后消除）

use std::{sync::Arc, time::Duration};

use bytemuck::cast_slice;

use starfish::base::app::{run, Application, Ctx, WindowConfig};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::sampler_desc::SamplerDescriptor;
use starfish::base::render::shader_module::shader_module::ShaderModule;
use starfish::base::render::RenderContext;
use starfish::base::resources::shader::Shader;
use starfish::base::video::{Video, VideoModule};
use starfish::base::window::{Window, WindowEvent};
use wgpu::Color;

const VIDEO_WGSL: &str = include_str!("../../resources/shaders/texture.wgsl");
const VIDEO_PATH: &str = "resources/videos/sample-5s.mp4";

/// 单窗口渲染单元：窗口 + 共享设备的视频句柄 + 呈现管线（就绪后装配）
struct Player {
    context: Option<RenderContext>,
    access: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    shader: Option<Arc<ShaderModule>>,
    /// 全屏三角形（3 顶点覆盖 NDC 全屏，白色顶点色 → 纹理原色）
    mesh: Option<Mesh>,
    sampler: Option<Arc<wgpu::Sampler>>,
    bind_group: Option<BindGroup>,
    pipeline: Option<Arc<RenderPipeline>>,
    video: Option<Video>,
}

impl Player {
    fn new() -> Self {
        Self {
            context: None,
            access: None,
            surface: None,
            shader: None,
            mesh: None,
            sampler: None,
            bind_group: None,
            pipeline: None,
            video: None,
        }
    }
}

impl Application for Player {
    /// 首窗就绪（启动门：真实尺寸已就位）：建表面 + 视频管理器 + 打开视频
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, access, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

        let shader = access
            .shader_module_builder(Shader::new(VIDEO_WGSL.to_string()))
            .build(Some("video_shader"));

        // 全屏三角形（顶点超 NDC 出界，插值后可见区恰好铺满屏幕；uv 对应 0..1）
        let verts: &[f32] = &[
            // pos(x,y,z)    color(r,g,b)   uv(u,v)
            -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, // 左下
             3.0, -1.0, 0.0, 1.0, 1.0, 1.0, 2.0, 1.0, // 右下（出界）
            -1.0,  3.0, 0.0, 1.0, 1.0, 1.0, 0.0, -1.0, // 左上（出界）
        ];
        let mesh = access
            .mesh_builder(
                vec![
                    wgpu::VertexFormat::Float32x3,
                    wgpu::VertexFormat::Float32x3,
                    wgpu::VertexFormat::Float32x2,
                ],
                cast_slice(verts).to_vec(),
            )
            .build(Some("video_quad"), None);

        // 线性采样器
        let sampler = Arc::new(access.create_sampler("video_sampler", &SamplerDescriptor::default()));

        // 视频管理器：与窗口共享同一 wgpu 设备（纹理上传必需）
        let video = VideoModule::new(context.device().clone(), context.queue().clone())
            .open(VIDEO_PATH)
            .expect("打开视频失败");

        self.context = Some(context);
        self.access = Some(access);
        self.surface = Some(surface);
        self.shader = Some(shader);
        self.mesh = Some(mesh);
        self.sampler = Some(sampler);
        self.video = Some(video);
    }

    fn event(&mut self, _win: &Window, _e: &WindowEvent, _ctx: &mut Ctx) {}

    /// 帧钩子：泵视频 → 装配呈现管线（首帧一次）→ 全屏绘制视频纹理
    fn frame(&mut self, ctx: &mut Ctx) {
        let Some(video) = self.video.as_mut() else {
            return;
        };

        // ① 手动泵：解码到主时钟（遮挡/暂停场景由 enabled 控制跳过）
        video
            .update(Duration::from_secs_f32(ctx.delta()))
            .expect("视频解码失败");

        // ② 呈现管线装配（视频首帧纹理就绪后恰好一次）
        if self.pipeline.is_none() {
            if let Some(view) = video.texture_view() {
                let access = self.access.as_ref().unwrap();
                let shader = self.shader.as_ref().unwrap();
                let sampler = self.sampler.as_ref().unwrap();
                // 裸视图直绑：视频帧纹理不经库 Texture 包装
                let bind_group = access
                    .bind_group_builder()
                    .texture_view(0, view)
                    .sampler(1, Arc::clone(sampler))
                    .build(Some("video_bind"));
                let pipeline = access
                    .render_pipeline_builder_2d(shader)
                    .build(&[&bind_group], self.mesh.as_ref().unwrap(), Some("video_pipeline"));
                self.bind_group = Some(bind_group);
                self.pipeline = Some(pipeline);
            } else {
                return; // 纹理未就绪（首帧前），下一帧继续
            }
        }

        // ③ 全屏绘制视频纹理（清屏 + 采样）
        let surface = self.surface.as_mut().unwrap();
        let access = self.access.as_ref().unwrap();
        let pipeline = self.pipeline.as_ref().unwrap();
        let mesh = self.mesh.as_ref().unwrap();
        let bind_group = self.bind_group.as_ref().unwrap();

        surface.begin_frame(Color { r: 0.05, g: 0.05, b: 0.08, a: 1.0 }, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = access.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass(
            "video_pass",
            &color_atts,
            None,
            None,
            None,
            None,
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group);
        pass.draw_mesh(mesh);
        pass.end();
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

fn main() {
    run(
        Player::new(),
        WindowConfig::new("视频播放", 1280, 720).with_fps_cap(60),
    );
}
