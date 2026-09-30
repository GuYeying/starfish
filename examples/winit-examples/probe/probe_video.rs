//! probe_video：video 模块验证（硬解 → 纹理 → 全屏采样，六平台同一套）
//!
//! 资产经 `kit::asset_path` 零 cfg 解析（桌面/web = 同一相对路径；
//! Android = 内嵌字节落盘私有目录）。播放结束 1s 自动退出（无头友好）。
//! 音轨相位（2026-09-20）：`open_with_audio` 挂 mixer 流式声部——画面硬解 +
//! 音轨 symphonia 软解推声部，全平台同一链路；真机可听，无头以推帧计数判定。
//!
//! 判据：console 锚点 `[video] OPEN PASS`、`[video] FIRST PASS`、
//! `[video] AUDIO PASS frames=N`（声部收到推帧即判——混音出声为伴生效果）、
//! `[video] ENDED PASS`；每 1s 心跳 `pos/size/ended`。打开/泵失败 =
//! `FAIL` 详情上屏 + 控制台。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::audio::{AudioMixer, StreamVoice};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::sampler_desc::SamplerDescriptor;
use starfish::base::render::shader_module::shader_module::ShaderModule;
use starfish::base::resources::shader::Shader;
use starfish::base::video::{Video, VideoModule};
use starfish::base::debug::console_log;
use starfish::base::window::{Window, WindowEvent};
use wgpu::BufferUsages;

use std::sync::Arc;
use std::time::Duration;

const VIDEO_WGSL: &str = include_str!("../../resources/shaders/texture.wgsl");
const VIDEO_ASSET: &str = "resources/videos/sample-5s.mp4";

/// 全屏三角形（顶点超 NDC 出界，插值后恰好铺满屏幕；uv 0..1）
const QUAD_VERTS: &[f32] = &[
    // pos(x,y,z)    color(r,g,b)   uv(u,v)
    -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0, //
     3.0, -1.0, 0.0, 1.0, 1.0, 1.0, 2.0, 1.0, //
    -1.0,  3.0, 0.0, 1.0, 1.0, 1.0, 0.0, -1.0, //
];

/// 视频呈现三件套（首帧纹理就绪后恰好装配一次；纹理同尺寸覆写稳定）
struct VideoQuad {
    bind_group: BindGroup,
    pipeline: Arc<RenderPipeline>,
    mesh: Mesh,
}

struct VideoProbe {
    panel: StatusPanel,
    video: Option<Video>,
    quad: Option<VideoQuad>,
    /// 流式声部句柄（音轨推帧判读 + ended 淡出）
    voice: Option<StreamVoice>,
    /// mixer 保活（drop 即停设备）
    mixer: Option<AudioMixer>,
    opened: bool,
    audio_logged: bool,
    done_hold: f32,
    log_elapsed: f32,
}

impl VideoProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            video: None,
            quad: None,
            voice: None,
            mixer: None,
            opened: false,
            audio_logged: false,
            done_hold: 0.0,
            log_elapsed: 0.0,
        }
    }

    /// 打开视频（帧 30 一次）：asset_path 零 cfg 解析 + mixer 流式声部直挂
    fn open_video(&mut self) {
        let opened = kit::asset_path(VIDEO_ASSET, || include_bytes!("../../resources/videos/sample-5s.mp4"))
            .map_err(|e| format!("{e}"))
            .and_then(|path| {
                self.panel
                    .with_gpu(|gpu| {
                        let mut mixer =
                            AudioMixer::new(4).map_err(|e| format!("mixer {e:?}"))?;
                        let voice = mixer.open_stream_voice();
                        let handle = voice.clone(); // probe 侧判读/淡出句柄
                        let video = VideoModule::new(
                            gpu._context.device().clone(),
                            gpu._context.queue().clone(),
                        )
                        .open_with_audio(&path, voice)
                        .map_err(|e| format!("{e:?}"))?;
                        Ok((video, mixer, handle))
                    })
                    .ok_or_else(|| "gpu not ready".to_string())?
            });
        match opened {
            Ok((video, mixer, handle)) => {
                self.panel
                    .verdict("video", "OPEN", Status::Pass, &format!("{VIDEO_ASSET} +audio"));
                self.video = Some(video);
                self.mixer = Some(mixer); // 保活（drop 即停设备）
                self.voice = Some(handle);
            }
            Err(e) => self.panel.verdict("video", "OPEN", Status::Fail, &e),
        }
        self.opened = true;
    }

    /// 首帧纹理就绪：懒装配呈现管线（与 13 号同一画法）
    fn build_quad(&mut self) {
        let Some(view) = self.video.as_ref().and_then(|v| v.texture_view()) else {
            return;
        };
        let quad = self.panel.with_gpu(|gpu| {
            let shader = gpu
                .access
                .shader_module_builder(Shader::new(VIDEO_WGSL.to_string()))
                .build(Some("probe_video_shader"));
            let mesh = gpu
                .access
                .mesh_builder(
                    vec![
                        wgpu::VertexFormat::Float32x3,
                        wgpu::VertexFormat::Float32x3,
                        wgpu::VertexFormat::Float32x2,
                    ],
                    bytemuck::cast_slice(QUAD_VERTS).to_vec(),
                )
                .build(Some("probe_video_quad"), None);
            let sampler = Arc::new(
                gpu.access
                    .create_sampler("probe_video_sampler", &SamplerDescriptor::default()),
            );
            let bind_group = gpu
                .access
                .bind_group_builder()
                .texture_view(0, view)
                .sampler(1, Arc::clone(&sampler))
                .build(Some("probe_video_bind"));
            let pipeline = gpu
                .access
                .render_pipeline_builder_2d(&shader)
                .build(&[&bind_group], &mesh, Some("probe_video_pipeline"));
            VideoQuad { bind_group, pipeline, mesh }
        });
        if let Some(q) = quad {
            let (w, h) = self
                .video
                .as_ref()
                .map(|v| v.size())
                .unwrap_or((0, 0));
            self.panel
                .verdict("video", "FIRST", Status::Pass, &format!("{w}x{h}"));
            self.quad = Some(q);
        }
    }
}

impl Application for VideoProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        if let WindowEvent::Resized { width, height } = event {
            self.panel.on_resize(*width, *height);
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();
        if !self.opened && f == 30 {
            self.open_video();
        }

        // 泵视频：解码推进 + 纹理上传
        if let Some(video) = self.video.as_mut() {
            if let Err(e) = video.update(Duration::from_secs_f32(ctx.delta())) {
                self.panel.verdict("video", "PUMP", Status::Fail, &format!("{e:?}"));
                self.video = None; // 只报一次
            }
        }

        // 首帧 → 懒装配呈现管线
        if self.opened && self.quad.is_none() && self.video.is_some() {
            self.build_quad();
        }

        // 音轨判读：声部收到推帧即 PASS（真机可听；无头以推帧计数判定）。
        // 顺手过一遍控制面（音量直通声部）
        if !self.audio_logged {
            if let Some(voice) = &self.voice {
                if voice.pushed_frames() > 0 {
                    if let Some(video) = &self.video {
                        video.set_audio_volume(0.8);
                    }
                    self.panel.verdict(
                        "video",
                        "AUDIO",
                        Status::Pass,
                        &format!("frames={} rate={}", voice.pushed_frames(), voice.sample_rate()),
                    );
                    self.audio_logged = true;
                }
            }
        }

        // 心跳：每 1s 上报解码位置 / 帧尺寸 / 结束标记
        self.log_elapsed += ctx.delta();
        if self.log_elapsed >= 1.0 {
            self.log_elapsed = 0.0;
            match self.video.as_ref() {
                Some(v) => console_log(&format!(
                    "[video] pos={:.2}s size={:?} ended={} audio_frames={}",
                    v.position().as_secs_f32(),
                    v.size(),
                    v.ended(),
                    self.voice.as_ref().map(|s| s.pushed_frames()).unwrap_or(0)
                )),
                None => {}
            }
        }

        // 播放结束：淡出声部（标准收尾打法），停留 1s 自动退出（无头友好）
        let ended = self.video.as_ref().map(|v| v.ended()).unwrap_or(false);
        if ended {
            if self.done_hold == 0.0 {
                self.panel.verdict("video", "ENDED", Status::Pass, "");
                if let Some(voice) = &self.voice {
                    voice.fade_out_and_close(200);
                }
            }
            self.done_hold += ctx.delta();
            if self.done_hold > 1.0 {
                ctx.exit();
                return;
            }
        }

        let quad = self.quad.as_ref();
        self.panel.render(ctx, false, |pass, _access| {
            if let Some(q) = quad {
                pass.set_pipeline(&q.pipeline);
                pass.set_bind_group(0, &q.bind_group);
                pass.draw_mesh(&q.mesh);
            }
        });
    }
}

// 全屏三角形顶点字节流经 bytemuck::cast_slice（依赖由 Cargo [dependencies] 提供）

starfish::app_entry!(
    VideoProbe::new(),
    WindowConfig::new("probe video", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
