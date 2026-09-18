//! 17_permission_probe：Android 运行时权限全流程探针（hello 窗口 + 权限 + 屏显诊断）
//!
//! 目的：隔离"录音授权框不弹"问题——单变量最小复现，每一步结果都写在屏幕上：
//! 1. 点屏幕触发：CHECK（checkSelfPermission 初始态）→ REQ（requestPermissions
//!    调用是否抛异常，异常消息直接上屏）→ POLL（每 0.5s 轮询授权状态，12s 上限）
//! 2. 最终：绿 = GRANTED（可继续录音验证）/ 红 = 仍 DENIED（需容器设置手动授权）
//!
//! 桌面：无运行时权限概念，直接绿屏。
//!
//! 运行：
//! - Android：`cargo xtask android 17_permission_probe`

use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::font::{self, Font, GlyphAtlas};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use starfish::base::window::event::WindowEvent;
use starfish::base::window::Window;
use wgpu::BufferUsages;

use std::sync::Arc;

const ATLAS_CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";

struct ProbeApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    atlas: Option<GlyphAtlas>,
    _camera_buffer: Option<Arc<wgpu::Buffer>>,
    camera_bind: Option<BindGroup>,
    atlas_bind: Option<BindGroup>,
    pipeline: Option<Arc<RenderPipeline>>,
    meshes: Vec<Mesh>,
    lines: Vec<String>,
    clear_color: wgpu::Color,
    frame_count: u32,
    poll_elapsed: f32,
    poll_done: bool,
    text_dirty: bool,
}

impl ProbeApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            atlas: None,
            _camera_buffer: None,
            camera_bind: None,
            atlas_bind: None,
            pipeline: None,
            meshes: Vec::new(),
            lines: vec!["tap to start probe".into()],
            clear_color: wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 },
            frame_count: 0,
            poll_elapsed: 0.0,
            poll_done: true,
            text_dirty: false,
        }
    }

    fn set_line(&mut self, idx: usize, text: impl Into<String>) {
        // 过滤到图集字符集（非 ASCII → ?）
        let t: String = text
            .into()
            .chars()
            .map(|c| if ATLAS_CHARS.contains(c) { c } else { '?' })
            .collect();
        while self.lines.len() <= idx {
            self.lines.push(String::new());
        }
        if self.lines[idx] != t {
            self.lines[idx] = t;
            self.text_dirty = true;
        }
    }

    /// 重建全部文本网格（状态变化时一次）
    fn rebuild_meshes(&mut self) {
        if let (Some(resouce), Some(atlas)) = (self.resouce.as_ref(), self.atlas.as_ref()) {
            self.meshes = self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .map(|(i, t)| {
                    font::text_mesh_tf(
                        resouce,
                        atlas,
                        t,
                        &glam::Mat4::from_translation(glam::Vec3::new(30.0, 60.0 + i as f32 * 70.0, 0.0)),
                        1.0,
                        [1.0, 1.0, 1.0, 1.0],
                    )
                })
                .collect();
        }
        self.text_dirty = false;
    }

    /// 采集状态检查（jni 0.21 经 robius 环境桥）
    #[cfg(target_os = "android")]
    fn check_granted(&self) -> Result<bool, String> {
        use jni::objects::JValue;
        let inner = robius_android_env::with_activity(|env, activity| -> Result<bool, String> {
            let perm = env
                .new_string(RECORD_AUDIO)
                .map_err(|e| format!("new_string: {e:?}"))?;
            env.call_method(
                activity,
                "checkSelfPermission",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&perm)],
            )
            .and_then(|v| v.i())
            .map(|i| i == 0)
            .map_err(|e| format!("checkSelfPermission: {e:?}"))
        });
        inner
            .map_err(|e| format!("STEP-0 env: {e:?}"))
            .and_then(|r| r)
    }

    /// 发起 requestPermissions；返回 Ok(已发起) 或 Err(具体 Java 异常消息)
    #[cfg(target_os = "android")]
    fn request(&self) -> Result<(), String> {
        use jni::objects::{JObject, JString, JValue};
        let inner = robius_android_env::with_activity(|env, activity| -> Result<(), String> {
            let perm = env
                .new_string(RECORD_AUDIO)
                .map_err(|e| format!("new_string: {e:?}"))?;
            let arr = env
                .new_object_array(1, "java/lang/String", &perm)
                .map_err(|e| format!("new_object_array: {e:?}"))?;
            if let Err(e) = env.call_method(
                activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&arr), JValue::Int(7001)],
            ) {
                // 捕获 pending Java 异常详情（ExceptionDescribe 不可见，读 message）
                let detail = if env.exception_check().unwrap_or(false) {
                    let t = env.exception_occurred().map_err(|e| format!("occurred: {e:?}"))?;
                    env.exception_clear();
                    let msg = env
                        .call_method(&t, "getMessage", "()Ljava/lang/String;", &[])
                        .and_then(|v| v.l())
                        .ok();
                    match msg {
                        Some(o) if !o.is_null() => {
                            let js = JString::from(o);
                            let s = env.get_string(&js).map_err(|e| format!("{e:?}"))?;
                            format!("{e:?} | {}", s.to_string_lossy())
                        }
                        _ => format!("{e:?}"),
                    }
                } else {
                    format!("{e:?}")
                };
                return Err(format!("requestPermissions threw: {detail}"));
            }
            Ok(())
        });
        inner
            .map_err(|e| format!("STEP-0 env: {e:?}"))
            .and_then(|r| r)
    }

    fn run_probe(&mut self) {
        #[cfg(not(target_os = "android"))]
        {
            self.set_line(0, "desktop: no runtime permission needed");
            self.clear_color = wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 };
            return;
        }

        // 1+2. Android 权限全流程（desktop 在上方已 return）
        #[cfg(target_os = "android")]
        {
            // 1. 初始授权态
            match self.check_granted() {
                Ok(true) => {
                    self.set_line(0, "CHECK: GRANTED (already)");
                    self.set_line(1, "tap = re-probe; mic should work");
                    self.clear_color = wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 };
                    return;
                }
                Ok(false) => self.set_line(0, "CHECK: DENIED -> requesting..."),
                Err(e) => {
                    self.set_line(0, format!("CHECK ERR: {e}"));
                    self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                    return;
                }
            }

            // 2. 发起申请（异常消息直接上屏——这是本探针的核心诊断点）
            self.clear_color = wgpu::Color { r: 0.35, g: 0.30, b: 0.05, a: 1.0 }; // 黄=申请中
            match self.request() {
                Ok(()) => {
                    self.set_line(1, "REQ: sent (dialog should appear)");
                    self.poll_elapsed = 0.0;
                    self.poll_done = false; // 启动轮询窗口
                }
                Err(e) => {
                    self.set_line(1, format!("REQ ERR: {e}"));
                    self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                }
            }
        }
    }
}

impl Application for ProbeApp {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

        let font = Font::from_bytes(
            include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
            34.0,
        )
        .expect("字体加载失败");
        let atlas = font
            .build_atlas(ATLAS_CHARS.chars(), &resouce)
            .expect("图集构建失败");
        let camera_buffer = resouce.create_raw_buffer(
            Some("probe_camera"),
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let camera_bind = resouce
            .bind_group_builder()
            .uniform_raw(0, camera_buffer.clone(), 64)
            .build(Some("probe_camera_bind"));
        let atlas_bind = font::atlas_bind_group(&resouce, &atlas);
        let first = font::text_mesh_tf(
            &resouce,
            &atlas,
            &self.lines[0],
            &glam::Mat4::from_translation(glam::Vec3::new(30.0, 60.0, 0.0)),
            1.0,
            [1.0, 1.0, 1.0, 1.0],
        );
        let pipeline = font::text_pipeline(&resouce, &camera_bind, &atlas_bind, &first, 1);

        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);
        self.atlas = Some(atlas);
        self._camera_buffer = Some(camera_buffer);
        self.camera_bind = Some(camera_bind);
        self.atlas_bind = Some(atlas_bind);
        self.meshes = vec![first];
        self.pipeline = Some(pipeline);
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        if let WindowEvent::Resized { width, height } = event {
            if let Some(surface) = self.surface.as_mut() {
                surface.resize(*width, *height);
            }
        }
        if matches!(event, WindowEvent::MousePressed(_)) {
            self.run_probe();
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        // 启动 ~0.5s 后自动跑一轮
        if self.frame_count == 30 {
            self.run_probe();
        }
        self.frame_count += 1;

        // 轮询窗口：REQ 已发出且未出结果 → 12s 内出结论
        #[cfg(target_os = "android")]
        if self.frame_count > 30 && !self.poll_done {
            self.poll_elapsed += ctx.delta();
            let secs = self.poll_elapsed as u32;
            self.set_line(2, format!("POLL... {}s / 12s", secs));
            if self.poll_elapsed >= 12.0 {
                self.poll_done = true;
                match self.check_granted() {
                    Ok(true) => {
                        self.set_line(2, "RESULT: GRANTED");
                        self.set_line(3, "mic ready -> run 10_record_mic");
                        self.clear_color = wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 };
                    }
                    Ok(false) => {
                        self.set_line(2, "RESULT: STILL DENIED");
                        self.set_line(3, "grant manually in settings, then reopen");
                        self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                    }
                    Err(e) => self.set_line(3, format!("POLL ERR: {e}")),
                }
            }
        }

        if self.text_dirty {
            self.rebuild_meshes();
        }

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(resouce) = self.resouce.as_ref() else { return };
        let Some(camera_buffer) = self._camera_buffer.as_ref() else { return };
        let Some(camera_bind) = self.camera_bind.as_ref() else { return };
        let Some(atlas_bind) = self.atlas_bind.as_ref() else { return };
        let Some(pipeline) = self.pipeline.as_ref() else { return };

        surface.begin_frame(self.clear_color, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = resouce.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("probe_bg", &color_atts, None, None, None, None);
        for mesh in &self.meshes {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, camera_bind);
            pass.set_bind_group(1, atlas_bind);
            pass.set_mesh(mesh);
            pass.draw(0..mesh.vertex_count(), 0..1);
        }
        pass.end();
        let (w, h) = ctx.size();
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let projection = glam::Mat4::from_cols_array(&[
            2.0 / w, 0.0, 0.0, 0.0,
            0.0, -2.0 / h, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            -1.0, 1.0, 0.0, 1.0,
        ]);
        resouce.write_buffer(camera_buffer, 0, bytemuck::bytes_of(&projection.to_cols_array_2d()));
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
mod entry {
    use super::ProbeApp;
    use starfish::base::app::{run_android, WindowConfig};

    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        run_android(
            app,
            ProbeApp::new(),
            WindowConfig::new("perm probe", 800, 600).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    use starfish::base::app::run;
    run(
        ProbeApp::new(),
        WindowConfig::new("权限探针", 800, 600).with_fps_cap(60),
    );
}
