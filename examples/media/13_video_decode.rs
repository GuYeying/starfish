//! 视频播放演示：窗口内播放 H.264/MP4（平台硬解 → NV12 → RGBA 纹理 → 全屏绘制）
//!
//! 平台：Windows(MF) / Linux(Ubuntu) GStreamer / macOS·iOS VideoToolbox /
//! Android(MediaCodec)。硬解唯一策略：平台无硬件解码器时 open 直接报
//! NoHardwareDecoder。
//!
//! 链路：VideoModule（与窗口共享 wgpu 设备）手动泵解码 → 帧纹理 →
//! 采样管线全屏绘制。视频纹理同尺寸覆写稳定（绑定一次管到底）。
//!
//! 资产：resources/videos/sample-5s.mp4（H.264/MP4，1080p）
//! 运行：cargo run --example 13_video_decode
//! Android：cargo xtask android 13_video_decode
//! （同源文件双注册；视频经 include_bytes 内嵌 → 私有目录落盘 → 引擎路径加载；
//!   打开/泵解码失败走阶段化色屏（橙/紫），不闪退）
//! 建议 --release：CPU 色彩转换在 debug 下 ~35ms/帧（v2 GPU 转换后消除）

use std::{sync::Arc, time::Duration};

use bytemuck::cast_slice;

use starfish::base::app::{Application, Ctx};
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use starfish::base::app::{run, WindowConfig};
#[cfg(target_os = "android")]
use starfish::base::app::{run_android, WindowConfig};
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
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
const VIDEO_PATH: &str = "resources/videos/sample-5s.mp4";

/// Android：应用私有目录（android_main 注入；提取内嵌视频用）
#[cfg(target_os = "android")]
static ANDROID_DATA_DIR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 视频路径：桌面相对路径；Android 内嵌 → 私有目录落盘（幂等：已存在不重写）
#[cfg(target_os = "android")]
fn video_path() -> Result<String, String> {
    let dir = ANDROID_DATA_DIR
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "internal_data_path unavailable".to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {e}"))?;
    let path = format!("{dir}/sample-5s.mp4");
    if !std::path::Path::new(&path).exists() {
        println!("[13] 提取内嵌视频到私有目录...");
        std::fs::write(&path, include_bytes!("../../resources/videos/sample-5s.mp4"))
            .map_err(|e| format!("write: {e}"))?;
    }
    Ok(path)
}
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn video_path() -> Result<String, String> {
    Ok(VIDEO_PATH.to_string())
}

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
    /// 阶段化错误（非致命）：红 = 提取失败 / 橙 = 打开失败 / 紫 = 泵失败
    error: Option<(Color, String)>,
    /// 播放结束后停留计时（1s 后自动退出）
    done_hold: f32,
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
            error: None,
            done_hold: 0.0,
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

        self.context = Some(context);
        self.access = Some(access);
        self.surface = Some(surface);
        self.shader = Some(shader);
        self.mesh = Some(mesh);
        self.sampler = Some(sampler);

        // 视频管理器：与窗口共享同一 wgpu 设备（纹理上传必需）
        let vpath = match video_path() {
            Ok(p) => p,
            Err(e) => {
                println!("[13] 提取失败: {e}");
                self.error = Some((
                    Color { r: 0.4, g: 0.05, b: 0.05, a: 1.0 },
                    format!("EXTRACT FAIL: {e}"),
                ));
                return;
            }
        };
        println!("[13] 打开视频: {vpath}");

        let opened = VideoModule::new(
            self.context.as_ref().unwrap().device().clone(),
            self.context.as_ref().unwrap().queue().clone(),
        )
        .open(&vpath);
        let video = match opened {
            Ok(v) => v,
            Err(e) => {
                println!("[13] 打开视频失败: {e:?}");
                self.error = Some((
                    Color { r: 0.5, g: 0.28, b: 0.0, a: 1.0 },
                    format!("OPEN FAIL: {e:?}"),
                ));
                return;
            }
        };
        self.video = Some(video);
    }

    fn event(&mut self, _win: &Window, _e: &WindowEvent, _ctx: &mut Ctx) {}

    /// 帧钩子：泵视频 → 装配呈现管线（首帧一次）→ 全屏绘制视频纹理
    fn frame(&mut self, ctx: &mut Ctx) {
        // 错误态：纯色屏（红 = 提取失败 / 橙 = 打开失败）+ 停留 3s 后退出
        if let Some((color, msg)) = &self.error {
            println!("[13] 错误态: {msg}");
            let surface = self.surface.as_mut().unwrap();
            surface.begin_frame(*color, 1.0);
            let color_attachment = surface.get_current_color_attachment();
            let mut encoder = self.access.as_ref().unwrap().create_command_encoder();
            let color_atts = [&color_attachment];
            let mut pass =
                encoder.begin_render_pass("video_err", &color_atts, None, None, None, None);
            pass.end();
            surface.submit([encoder.finish()]);
            surface.present();

            self.done_hold += ctx.delta();
            if self.done_hold > 3.0 {
                println!("[13] 错误态退出");
                ctx.exit();
            }
            return;
        }

        let Some(video) = self.video.as_mut() else { return };

        // ① 手动泵：解码到主时钟（遮挡/暂停场景由 enabled 控制跳过）
        if let Err(e) = video.update(Duration::from_secs_f32(ctx.delta())) {
            println!("[13] 泵解码失败: {e:?}");
            self.error = Some((
                Color { r: 0.5, g: 0.10, b: 0.35, a: 1.0 },
                format!("PUMP FAIL: {e:?}"),
            ));
            return;
        }

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

        // ④ 播放结束：停留 1s 后自动退出
        if video.ended() {
            self.done_hold += ctx.delta();
            if self.done_hold > 1.0 {
                println!("[13] 播放结束，自动退出");
                ctx.exit();
            }
        }
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
mod entry {
    use super::Player;
    use starfish::base::app::{run_android, WindowConfig};

    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        {
            let mut dir = super::ANDROID_DATA_DIR.lock().unwrap();
            *dir = app
                .internal_data_path()
                .map(|p| p.to_string_lossy().into_owned());
        }
        run_android(
            app,
            Player::new(),
            WindowConfig::new("video", 1280, 720).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    run(
        Player::new(),
        WindowConfig::new("视频播放", 1280, 720).with_fps_cap(60),
    );
}
