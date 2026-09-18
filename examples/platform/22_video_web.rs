//! 22_video_web：Web 视频解码独立探针（完整渲染版）
//!
//! 单一职责：验证 WebCodecs 解码链路 + 视频纹理全屏渲染。
//! 画面动起来 = fetch + 解复用 + 硬解 + 色彩转换 + 纹理上传 + 采样绘制全链路通。
//! 播放结束 1 秒后自动退出；打开/泵失败 = 紫屏 + 详情走控制台（3s 后退出）。
//!
//! 渲染画法与 13_video_decode 一致（全屏三角形 + texture.wgsl 采样管线，
//! 纹理就绪后懒装配管线），差异仅在装配时机：Web 无阻塞初始化，GPU 家当
//! 走 InitSlot 异步装配（见 start）。
//!
//! 运行：wasm 构建 + 服务器同源部署（`--web-dir ./web`），
//!       并确保 `web/sample-5s.mp4` 存在。
//! 桌面：`cargo run --example 22_video_web`（行为对照）

use starfish::base::app::{Application, Ctx, InitSlot, WindowConfig};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::sampler_desc::SamplerDescriptor;
use starfish::base::render::settings::{GpuSettings, SurfaceSettings};
use starfish::base::render::shader_module::shader_module::ShaderModule;
use starfish::base::render::RenderContext;
use starfish::base::resources::shader::Shader;
use starfish::base::video::{Video, VideoModule};
use starfish::base::web::console_log;
use starfish::base::window::{Window, WindowEvent};
use wgpu::Color;

use std::sync::Arc;
use std::time::Duration;

const VIDEO_WGSL: &str = include_str!("../../resources/shaders/texture.wgsl");

/// GPU 资源包（InitSlot 异步装配完成后的渲染侧全部家当）
struct Gpu {
    _context: RenderContext,
    access: RenderResourceAccess,
    surface: RenderSurface,
    shader: Arc<ShaderModule>,
    /// 全屏三角形（3 顶点覆盖 NDC，白色顶点色 → 纹理原色）
    mesh: Mesh,
    sampler: Arc<wgpu::Sampler>,
    /// 呈现管线 + 绑定组：视频首帧纹理就绪后于 frame 中懒装配（见 13 的 ② 步）
    pipeline: Option<Arc<RenderPipeline>>,
    bind_group: Option<BindGroup>,
    /// 视频句柄
    video: Option<Video>,
    /// 泵/打开失败（紫屏 + 详情在控制台，3s 后自动退出）
    error: Option<(Color, String)>,
}

struct VideoProbe {
    gpu: InitSlot<Gpu>,
    /// 播放结束/错误态后的停留计时（自动退出）
    done_hold: f32,
    /// 进度心跳计时（每 1s 上报解码位置）
    log_elapsed: f32,
}

impl VideoProbe {
    fn new() -> Self {
        Self {
            gpu: InitSlot::default(),
            done_hold: 0.0,
            log_elapsed: 0.0,
        }
    }
}

impl Application for VideoProbe {
    /// 首窗就绪：异步建 GPU 家当 + 打开视频
    fn start(&mut self, ctx: &mut Ctx) {
        let window = ctx.window().clone();
        let slot = self.gpu.clone();

        slot.init(async move {
            let (context, access, surface) = RenderEntry::async_new(
                &window,
                SurfaceSettings::default(),
                GpuSettings::default(),
            )
            .await
            .expect("rc");

            // 呈现静态资源：着色器 / 全屏三角形 / 线性采样器（不依赖视频）
            let shader = access
                .shader_module_builder(Shader::new(VIDEO_WGSL.to_string()))
                .build(Some("video_shader"));
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
                    bytemuck::cast_slice(verts).to_vec(),
                )
                .build(Some("video_quad"), None);
            let sampler = Arc::new(access.create_sampler(
                "video_sampler",
                &SamplerDescriptor::default(),
            ));

            // 视频（Web = fetch 页面相对 URL；桌面 = resources 路径）
            #[cfg(target_arch = "wasm32")]
            let opened = VideoModule::new(context.device().clone(), context.queue().clone())
                .open("sample-5s.mp4");
            #[cfg(not(target_arch = "wasm32"))]
            let opened = VideoModule::new(context.device().clone(), context.queue().clone())
                .open("resources/videos/sample-5s.mp4");

            let (video, error) = match opened {
                Ok(v) => (Some(v), None),
                Err(e) => {
                    console_log("[22] 视频打开失败");
                    console_log(&format!("[22] {e:?}"));
                    let err = (
                        Color { r: 0.35, g: 0.08, b: 0.40, a: 1.0 },
                        format!("open fail: {e:?}"),
                    );
                    (None, Some(err))
                }
            };

            Gpu {
                _context: context,
                access,
                surface,
                shader,
                mesh,
                sampler,
                pipeline: None,
                bind_group: None,
                video,
                error,
            }
        });
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        // Web 尺寸竞态（CLAUDE.md 坑位 2）：真实尺寸异步到达，必须回写表面
        if let WindowEvent::Resized { width, height } = event {
            if let Some(mut gpu) = self.gpu.get_mut() {
                gpu.surface.resize(*width, *height);
            }
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let Some(mut gpu) = self.gpu.get_mut() else { return };

        // 错误态：紫屏（详情已在进入时打过控制台），停留 3s 自动退出
        if let Some((color, _)) = gpu.error {
            gpu.surface.begin_frame(color, 1.0);
            let color_attachment = gpu.surface.get_current_color_attachment();
            let mut encoder = gpu.access.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut pass = encoder.begin_render_pass("vp_err", &color_atts, None, None, None, None);
            pass.end();
            gpu.surface.submit([encoder.finish()]);
            gpu.surface.present();

            self.done_hold += ctx.delta();
            if self.done_hold > 3.0 {
                console_log("[22] 错误态，自动退出");
                ctx.exit();
            }
            return;
        }

        // ① 泵视频：解码推进 + 纹理上传
        let pump_err = gpu.video.as_mut().and_then(|video| {
            video
                .update(Duration::from_secs_f32(ctx.delta()))
                .err()
                .map(|e| format!("PUMP FAIL: {e:?}"))
        });
        if let Some(e) = pump_err {
            console_log(&format!("[22] {e}"));
            gpu.error = Some((
                Color { r: 0.35, g: 0.08, b: 0.40, a: 1.0 },
                e,
            ));
            return;
        }

        // ② 呈现管线装配（视频首帧纹理就绪后恰好一次；裸视图直绑）
        if gpu.pipeline.is_none() {
            if let Some(view) = gpu.video.as_ref().and_then(|v| v.texture_view()) {
                let bind_group = gpu
                    .access
                    .bind_group_builder()
                    .texture_view(0, view)
                    .sampler(1, Arc::clone(&gpu.sampler))
                    .build(Some("video_bind"));
                let pipeline = gpu
                    .access
                    .render_pipeline_builder_2d(&gpu.shader)
                    .build(&[&bind_group], &gpu.mesh, Some("video_pipeline"));
                gpu.bind_group = Some(bind_group);
                gpu.pipeline = Some(pipeline);
                console_log("[22] 呈现管线就绪");
            }
            // 纹理未就绪（首帧前）：本帧只清屏，下一帧继续
        }

        // ③ 全屏绘制：有管线则采样视频纹理，否则纯清屏等待
        gpu.surface
            .begin_frame(Color { r: 0.05, g: 0.05, b: 0.08, a: 1.0 }, 1.0);
        let color_attachment = gpu.surface.get_current_color_attachment();
        let mut encoder = gpu.access.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("vp", &color_atts, None, None, None, None);
        if let (Some(pipeline), Some(bind_group)) = (&gpu.pipeline, &gpu.bind_group) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group);
            pass.draw_mesh(&gpu.mesh);
        }
        pass.end();
        gpu.surface.submit([encoder.finish()]);
        gpu.surface.present();

        // 进度心跳（诊断）：每 1s 上报解码位置 / 帧尺寸 / 结束标记
        self.log_elapsed += ctx.delta();
        if self.log_elapsed >= 1.0 {
            self.log_elapsed = 0.0;
            match gpu.video.as_ref() {
                Some(v) => console_log(&format!(
                    "[22] pos={:.2}s size={:?} ended={}",
                    v.position().as_secs_f32(),
                    v.size(),
                    v.ended()
                )),
                None => console_log("[22] video=None"),
            }
        }

        // ④ 播放结束：停留 1s 后自动退出
        let ended = gpu.video.as_ref().map(|v| v.ended()).unwrap_or(false);
        if ended {
            self.done_hold += ctx.delta();
            if self.done_hold > 1.0 {
                console_log("[22] 播放结束，自动退出");
                ctx.exit();
            }
        }
    }
}

// ── 入口 ──
fn main() {
    use starfish::base::app::run;
    run(
        VideoProbe::new(),
        WindowConfig::new("video web", 1280, 720)
            .with_fps_cap(60)
            .with_web_canvas_id("canvas"),
    );
}

starfish::web_entry!();
