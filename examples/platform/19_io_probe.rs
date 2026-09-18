//! 19_io_probe：跨平台 IO 模块全流程探针（hello 窗口 + 字体上屏诊断）
//!
//! 流程（启动 ~0.5s 自动跑一轮，点屏幕重跑）：
//! 1. WRITE  → io::write 写入 16B 测试字节
//! 2. EXISTS → io::exists 确认落盘
//! 3. READ   → io::read 读回并逐字节比对
//! 4. TEXT   → io::write_text / read_text 往返
//! 全部通过 → 绿屏 "ALL PASS"；任一步失败 → 红屏 + 失败行详情
//!
//! 位置：桌面 = CWD（或基准目录）；Android = 应用私有目录
//! （android_main 注入 internal_data_path → io::set_base_dir，相对路径
//!   自动落沙箱，屏显 PARMA 行展示实际路径）
//!
//! 运行：
//! - 桌面：`cargo run --example 19_io_probe`
//! - Android：`cargo xtask android 19_io_probe`

use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::font::{self, Font, GlyphAtlas};
use starfish::base::io;
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use starfish::base::window::{Window, WindowEvent};
use wgpu::BufferUsages;

use std::sync::Arc;

const ATLAS_CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";

struct IoProbe {
    _context: Option<RenderContext>,
    access: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    atlas: Option<GlyphAtlas>,
    _camera_buffer: Option<Arc<wgpu::Buffer>>,
    camera_bind: Option<BindGroup>,
    atlas_bind: Option<BindGroup>,
    text_meshes: Vec<Mesh>,
    pipeline: Option<Arc<RenderPipeline>>,
    clear_color: wgpu::Color,
    lines: Vec<String>,
    text_dirty: bool,
    frame_count: u32,
}

impl IoProbe {
    fn new() -> Self {
        Self {
            _context: None,
            access: None,
            surface: None,
            atlas: None,
            _camera_buffer: None,
            camera_bind: None,
            atlas_bind: None,
            text_meshes: Vec::new(),
            pipeline: None,
            clear_color: wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 },
            lines: vec!["tap to start io probe".into()],
            text_dirty: false,
            frame_count: 0,
        }
    }

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
        if let (Some(access), Some(atlas)) = (self.access.as_ref(), self.atlas.as_ref()) {
            self.text_meshes = self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .map(|(i, t)| {
                    font::text_mesh_tf(
                        access,
                        atlas,
                        t,
                        &glam::Mat4::from_translation(glam::Vec3::new(
                            24.0,
                            50.0 + i as f32 * 64.0,
                            0.0,
                        )),
                        1.0,
                        [0.6, 0.9, 1.0, 1.0],
                    )
                })
                .collect();
        }
        self.text_dirty = false;
    }

    /// io 全流程：写 → 存在 → 读回比对 → 文本往返
    fn run_probe(&mut self) {
        let payload: Vec<u8> = (0..16u8).map(|i| i.wrapping_mul(17)).collect();

        // 1. 写
        if let Err(e) = pollster::block_on(io::write("starfish_io_probe.bin", payload.clone())) {
            self.set_lines(vec!["WRITE FAIL".into(), format!("{e}")]);
            self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
            return;
        }

        // 2. 存在探测
        match pollster::block_on(io::exists("starfish_io_probe.bin")) {
            Ok(true) => self.set_lines(vec!["WRITE OK / EXISTS OK".into()]),
            Ok(false) => {
                self.set_lines(vec!["EXISTS FAIL: written file not found".into()]);
                self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                return;
            }
            Err(e) => {
                self.set_lines(vec![format!("EXISTS ERR: {e}")]);
                self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                return;
            }
        }

        // 3. 读回逐字节比对
        let read_back = match pollster::block_on(io::read("starfish_io_probe.bin")) {
            Ok(b) => b,
            Err(e) => {
                self.set_lines(vec!["READ FAIL".into(), format!("{e}")]);
                self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                return;
            }
        };
        if read_back != payload {
            self.set_lines(vec!["READ MISMATCH".into(), format!("{} vs {} bytes", read_back.len(), payload.len())]);
            self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
            return;
        }

        // 4. 文本往返
        if let Err(e) = pollster::block_on(io::write_text("starfish_io_probe.txt", "io probe text ok")) {
            self.set_lines(vec!["TEXT WRITE FAIL".into(), format!("{e}")]);
            self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
            return;
        }
        let text = match pollster::block_on(io::read_text("starfish_io_probe.txt")) {
            Ok(t) => t,
            Err(e) => {
                self.set_lines(vec!["TEXT READ FAIL".into(), format!("{e}")]);
                self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
                return;
            }
        };
        if text != "io probe text ok" {
            self.set_lines(vec!["TEXT MISMATCH".into(), text]);
            self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
            return;
        }

        // 全部通过
        self.set_lines(vec![
            "ALL PASS".into(),
            "bin 16B ok / text ok".into(),
            "tap = re-run".into(),
        ]);
        self.clear_color = wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 }; // 绿
    }
}

impl Application for IoProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, access, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

        let font = Font::from_bytes(
            include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
            30.0,
        )
        .expect("字体加载失败");
        let atlas = font
            .build_atlas(ATLAS_CHARS.chars(), &access)
            .expect("图集构建失败");
        let camera_buffer = access.create_raw_buffer(
            Some("io_camera"),
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let camera_bind = access
            .bind_group_builder()
            .uniform_raw(0, camera_buffer.clone(), 64)
            .build(Some("io_camera_bind"));
        let atlas_bind = font::atlas_bind_group(&access, &atlas);
        let first = font::text_mesh_tf(
            &access,
            &atlas,
            &self.lines[0],
            &glam::Mat4::from_translation(glam::Vec3::new(24.0, 50.0, 0.0)),
            1.0,
            [0.6, 0.9, 1.0, 1.0],
        );
        let pipeline = font::text_pipeline(&access, &camera_bind, &atlas_bind, &first, 1);

        self._context = Some(context);
        self.access = Some(access);
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

        if self.text_dirty {
            self.rebuild_meshes();
        }

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(access) = self.access.as_ref() else { return };
        let Some(camera_buffer) = self._camera_buffer.as_ref() else { return };
        let Some(camera_bind) = self.camera_bind.as_ref() else { return };
        let Some(atlas_bind) = self.atlas_bind.as_ref() else { return };
        let Some(pipeline) = self.pipeline.as_ref() else { return };

        surface.begin_frame(self.clear_color, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = access.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("io_probe", &color_atts, None, None, None, None);
        for mesh in &self.text_meshes {
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
        access.write_buffer(camera_buffer, 0, bytemuck::bytes_of(&projection.to_cols_array_2d()));
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
mod entry {
    use super::IoProbe;
    use starfish::base::app::{run_android, WindowConfig};
    use starfish::base::io;

    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        // 沙箱基准目录：io 的相对路径自动落到应用私有目录
        if let Some(dir) = app.internal_data_path() {
            io::set_base_dir(&dir);
        }
        run_android(
            app,
            IoProbe::new(),
            WindowConfig::new("io probe", 800, 600).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    use starfish::base::app::run;
    run(
        IoProbe::new(),
        WindowConfig::new("IO 探针", 800, 600).with_fps_cap(60),
    );
}
