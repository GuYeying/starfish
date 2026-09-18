//! 16_dialog：对话框模块跨平台验证（桌面 rfd / Android robius，同一 API）
//!
//! **轮询式模式**（Android 推荐）：对话框是独立 Activity，覆盖期间会触发
//! 本应用生命周期流转——阻塞式等待会让事件泵停摆（实测返回后卡顿）。
//! 本示例演示标准姿势：`pick_file_start` / `save_bytes_start` 非阻塞发起，
//! 每帧 `try_result()` 收结果，主线程全程保持事件泵。
//!
//! 流程（无需操作即可观察）：
//! 1. 启动 ~0.5s 后自动弹出"打开文件"（黄色 = 打开中）
//! 2. 选取 → 绿 + 屏显文件名与字节数；取消 → 蓝；出错 → 红 + 屏显异常详情
//! 3. 之后每次点击/触摸交替触发"保存数据"（青绿 = 保存流程）
//!
//! 运行：
//! - 桌面：`cargo run --example 16_dialog`
//! - Android：`cargo xtask android 16_dialog`

use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::dialog::{self, PickJob, SaveJob};
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

/// 图集字符集：可打印 ASCII（错误详情与状态文本均在此范围）
const ATLAS_CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

struct DialogApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    atlas: Option<GlyphAtlas>,
    _camera_buffer: Option<Arc<wgpu::Buffer>>,
    camera_bind: Option<BindGroup>,
    atlas_bind: Option<BindGroup>,
    text_meshes: Vec<Mesh>,
    pipeline: Option<Arc<RenderPipeline>>,
    clear_color: wgpu::Color,
    lines: Vec<String>,
    frame_count: u32,
    /// false = 下次触发 pick_file；true = 下次触发 save_bytes
    next_is_save: bool,
    pick_job: Option<PickJob>,
    save_job: Option<SaveJob>,
    text_dirty: bool,
}

impl DialogApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            atlas: None,
            _camera_buffer: None,
            camera_bind: None,
            atlas_bind: None,
            text_meshes: Vec::new(),
            pipeline: None,
            clear_color: wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 },
            lines: vec!["tap to start probe".into()],
            frame_count: 0,
            next_is_save: false,
            pick_job: None,
            save_job: None,
            text_dirty: false,
        }
    }

    /// 更新屏显文本行（字符过滤到图集范围；未知字符 → ?）
    fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines
            .into_iter()
            .map(|t| {
                t.chars()
                    .map(|c| if ATLAS_CHARS.contains(c) { c } else { '?' })
                    .collect::<String>()
            })
            .collect();
        self.text_dirty = true;
    }

    /// 重建全部文本网格（状态变化时一次）
    fn rebuild_meshes(&mut self) {
        if let (Some(resouce), Some(atlas)) = (self.resouce.as_ref(), self.atlas.as_ref()) {
            self.text_meshes = self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .map(|(i, t)| {
                    font::text_mesh_tf(
                        resouce,
                        atlas,
                        t,
                        &glam::Mat4::from_translation(glam::Vec3::new(
                            30.0,
                            60.0 + i as f32 * 70.0,
                            0.0,
                        )),
                        1.0,
                        [1.0, 1.0, 1.0, 1.0],
                    )
                })
                .collect();
        }
        self.text_dirty = false;
    }

    /// 非阻塞发起对话框（轮询式；不阻塞事件循环 → SAF 期间生命周期正常流转）
    fn start_dialog(&mut self) {
        if self.pick_job.is_some() || self.save_job.is_some() {
            return; // 已有任务进行中
        }
        self.clear_color = wgpu::Color { r: 0.35, g: 0.30, b: 0.05, a: 1.0 }; // 黄=打开中
        if self.next_is_save {
            self.set_lines(vec!["SAVE: open dialog...".into()]);
            match dialog::save_bytes_start(
                "starfish_test.txt",
                "starfish dialog verify".as_bytes().to_vec(),
            ) {
                Ok(job) => self.save_job = Some(job),
                Err(e) => {
                    self.set_lines(vec![format!("SAVE START ERR: {e:?}")]);
                    self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                }
            }
        } else {
            self.set_lines(vec!["PICK: open dialog...".into()]);
            match dialog::pick_file_start(Some("选择任意文件"), &[("任意文件", &["*"])]) {
                Ok(job) => self.pick_job = Some(job),
                Err(e) => {
                    self.set_lines(vec![format!("PICK START ERR: {e:?}")]);
                    self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                }
            }
        }
        self.next_is_save = !self.next_is_save;
    }

    /// 收割已完成的对话框结果（每帧调用）
    fn poll_jobs(&mut self) {
        if let Some(job) = &mut self.pick_job {
            if let Some(res) = job.try_result() {
                self.pick_job = None;
                match res {
                    Ok(Some(pf)) => {
                        let bytes = pf.read().map(|b| b.len()).unwrap_or(0);
                        self.set_lines(vec![
                            "PICK OK".into(),
                            format!("name: {}", pf.name()),
                            format!("size: {bytes} bytes"),
                            "tap = next (save)".into(),
                        ]);
                        self.clear_color = wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 }; // 绿
                    }
                    Ok(None) => {
                        self.set_lines(vec!["PICK CANCELLED".into(), "tap = next (save)".into()]);
                        self.clear_color = wgpu::Color { r: 0.05, g: 0.10, b: 0.30, a: 1.0 }; // 蓝
                    }
                    Err(e) => {
                        self.set_lines(vec!["PICK ERR".into(), format!("{e:?}")]);
                        self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 }; // 红
                    }
                }
            }
        }
        if let Some(job) = &mut self.save_job {
            if let Some(res) = job.try_result() {
                self.save_job = None;
                match res {
                    Ok(Some(path)) => {
                        self.set_lines(vec!["SAVE OK".into(), format!("path: {}", path.display())]);
                        self.clear_color = wgpu::Color { r: 0.05, g: 0.28, b: 0.20, a: 1.0 }; // 青绿
                    }
                    Ok(None) => {
                        self.set_lines(vec!["SAVE CANCELLED".into(), "tap = next (pick)".into()]);
                        self.clear_color = wgpu::Color { r: 0.05, g: 0.10, b: 0.30, a: 1.0 }; // 蓝
                    }
                    Err(e) => {
                        self.set_lines(vec!["SAVE ERR".into(), format!("{e:?}")]);
                        self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 }; // 红
                    }
                }
            }
        }
    }
}

impl Application for DialogApp {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

        // 字体内嵌（Android 无 resources 文件可读；桌面同样走内存构造，单一代码路径）
        let font = Font::from_bytes(
            include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
            34.0,
        )
        .expect("字体加载失败");
        let atlas = font
            .build_atlas(ATLAS_CHARS.chars(), &resouce)
            .expect("图集构建失败");
        let camera_buffer = resouce.create_raw_buffer(
            Some("dlg_camera"),
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let camera_bind = resouce
            .bind_group_builder()
            .uniform_raw(0, camera_buffer.clone(), 64)
            .build(Some("dlg_camera_bind"));
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
        self.text_meshes = vec![first];
        self.pipeline = Some(pipeline);
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        if let WindowEvent::Resized { width, height } = event {
            if let Some(surface) = self.surface.as_mut() {
                surface.resize(*width, *height);
            }
        }
        // 点击/触摸（winit 把主触摸映射为鼠标左键）→ 再触发一轮
        if matches!(event, WindowEvent::MousePressed(_)) {
            self.start_dialog();
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        // 启动 ~0.5s 后自动触发第一轮
        if self.frame_count == 30 {
            self.start_dialog();
        }
        self.frame_count += 1;

        self.poll_jobs(); // 收割对话框结果（非阻塞）

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
        let mut pass = encoder.begin_render_pass("dialog_bg", &color_atts, None, None, None, None);
        for mesh in &self.text_meshes {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, camera_bind);
            pass.set_bind_group(1, atlas_bind);
            pass.set_mesh(mesh);
            pass.draw(0..mesh.vertex_count(), 0..1);
        }
        pass.end();
        // HUD 相机：像素正交（y 向下），随窗口尺寸
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
    use super::DialogApp;
    use starfish::base::app::{run_android, WindowConfig};

    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        run_android(
            app,
            DialogApp::new(),
            WindowConfig::new("dialog", 800, 600).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    use starfish::base::app::run;
    run(
        DialogApp::new(),
        WindowConfig::new("对话框验证", 800, 600).with_fps_cap(120),
    );
}
