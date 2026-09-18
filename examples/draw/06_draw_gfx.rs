//! 几何绘制演示：gfx 全形状渲染（base 新循环模型）
//!
//! 运行：cargo run --example 06_draw_gfx
//!
//! 上半屏：2D 形状（像素空间正交相机；填充 + 1px 描边）
//! 全屏  ：3D 形状（透视相机绕场景缓速环绕，深度测试开启，可被地面遮挡）
//!
//! 覆盖 gfx 全部形状：rect / rect_outline / circle / circle_outline /
//! ellipse / line / polyline / regular_polygon / polygon / capsule2d /
//! cube / sphere / plane / cylinder / cone / capsule3d

use std::sync::Arc;

use bytemuck;
use glam::{Mat4, Vec2, Vec3};
use starfish::base::app::{Application, Ctx};
#[cfg(not(target_os = "android"))]
use starfish::base::app::{run, WindowConfig};
use starfish::base::gfx::{self, geometry};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use wgpu::BufferUsages;

// ===================== 应用（引擎持循环） =====================
struct GfxShapesApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    // 相机 uniform（2D 正交 / 3D 透视各一份）
    cam2d_buffer: Option<Arc<wgpu::Buffer>>,
    cam2d_bind: Option<BindGroup>,
    cam3d_buffer: Option<Arc<wgpu::Buffer>>,
    cam3d_bind: Option<BindGroup>,
    // 形状网格
    fill2d: Vec<Mesh>,
    line2d: Vec<Mesh>,
    fill3d: Vec<Mesh>,
    // 管线：2D 填充/描边 + 3D 填充（深度开启）
    fill2d_pipeline: Option<Arc<RenderPipeline>>,
    line2d_pipeline: Option<Arc<RenderPipeline>>,
    fill3d_pipeline: Option<Arc<RenderPipeline>>,
    // 运行时变量
    orbit: f32,
}

impl GfxShapesApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            cam2d_buffer: None,
            cam2d_bind: None,
            cam3d_buffer: None,
            cam3d_bind: None,
            fill2d: Vec::new(),
            line2d: Vec::new(),
            fill3d: Vec::new(),
            fill2d_pipeline: None,
            line2d_pipeline: None,
            fill3d_pipeline: None,
            orbit: 0.0,
        }
    }
}

impl Application for GfxShapesApp {
    fn start(&mut self, ctx: &mut Ctx) {
        let (_context, resouce, mut surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

        // ── 相机 uniform（04 同款：2D 正交 / 3D 透视各一份） ──
        let make_camera = || {
            let buffer = resouce.create_raw_buffer(
                Some("gfx_camera"),
                64,
                BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            );
            let bind = resouce
                .bind_group_builder()
                .uniform_raw(0, buffer.clone(), 64)
                .build(Some("gfx_camera_bind"));
            (buffer, bind)
        };
        let (cam2d_buffer, cam2d_bind) = make_camera();
        let (cam3d_buffer, cam3d_bind) = make_camera();

        // ── 2D 形状（像素空间，z = 0） ──
        let c2 = [0.95, 0.78, 0.3, 1.0]; // 琥珀
        let c2b = [0.4, 0.8, 0.95, 1.0]; // 青蓝
        let mut fill2d: Vec<Mesh> = Vec::new();
        let mut line2d: Vec<Mesh> = Vec::new();

        // 矩形：填充 + 描边
        let (min, max) = (Vec2::new(40.0, 50.0), Vec2::new(140.0, 150.0));
        fill2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::rect(min, max, c2)));
        line2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::rect_outline(min, max, c2b)));

        // 圆：填充 + 描边
        let center = Vec2::new(230.0, 100.0);
        fill2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::circle(center, 48.0, 64, c2)));
        line2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::circle_outline(center, 48.0, 64, c2b)));

        // 椭圆
        fill2d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape2d::ellipse(Vec2::new(370.0, 100.0), Vec2::new(58.0, 36.0), 64, c2),
        ));

        // 正五边形
        fill2d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape2d::regular_polygon(Vec2::new(500.0, 100.0), 48.0, 5, 0.0, c2),
        ));

        // 凹多边形（L 形）：耳切三角化
        let l_shape = [
            Vec2::new(560.0, 50.0),
            Vec2::new(660.0, 50.0),
            Vec2::new(660.0, 100.0),
            Vec2::new(610.0, 100.0),
            Vec2::new(610.0, 150.0),
            Vec2::new(560.0, 150.0),
        ];
        fill2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::polygon(&l_shape, c2)));

        // 胶囊 2D
        fill2d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape2d::capsule(Vec2::new(700.0, 80.0), Vec2::new(780.0, 140.0), 26.0, 32, c2),
        ));

        // 线段 + 折线
        line2d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape2d::line(Vec2::new(830.0, 60.0), Vec2::new(930.0, 150.0), c2b),
        ));
        let zigzag: Vec<Vec2> = (0..6)
            .map(|i| Vec2::new(960.0 + i as f32 * 24.0, if i % 2 == 0 { 60.0 } else { 140.0 }))
            .collect();
        line2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::polyline(&zigzag, false, c2b)));
        line2d.push(gfx::shape_mesh(&resouce, &geometry::shape2d::polyline(&zigzag, true, [1.0, 0.4, 0.4, 0.6])));

        // ── 3D 形状（世界空间，y 向上，排在 x 轴上） ──
        let mut fill3d: Vec<Mesh> = Vec::new();

        // 地面（14×14 平面，接收深度遮挡）
        fill3d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape3d::plane(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::X,
                Vec3::Z,
                Vec2::new(14.0, 14.0),
                [0.22, 0.25, 0.32, 1.0],
            ),
        ));

        // 立方体 / 球 / 圆柱 / 圆锥 / 胶囊 3D，沿 x 轴排开
        fill3d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape3d::cube(
                Vec3::new(-4.6, 0.0, -0.6),
                Vec3::new(-3.4, 1.2, 0.6),
                [0.9, 0.4, 0.3, 1.0],
            ),
        ));
        fill3d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape3d::sphere(Vec3::new(-2.0, 0.9, 0.0), 0.9, 32, 24, [0.3, 0.65, 0.95, 1.0]),
        ));
        fill3d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape3d::cylinder(Vec3::new(0.0, 0.0, 0.0), 0.7, 1.8, 48, [0.95, 0.8, 0.3, 1.0]),
        ));
        fill3d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape3d::cone(Vec3::new(2.0, 0.0, 0.0), 0.8, 1.8, 48, [0.4, 0.85, 0.5, 1.0]),
        ));
        fill3d.push(gfx::shape_mesh(
            &resouce,
            &geometry::shape3d::capsule(
                Vec3::new(4.0, 0.3, 0.0),
                Vec3::new(4.0, 1.5, 0.0),
                0.45,
                32,
                12,
                [0.8, 0.45, 0.9, 1.0],
            ),
        ));

        // ── 管线：2D 填充/描边 + 3D 填充（深度开启） ──
        let dummy = gfx::shape_mesh(&resouce, &geometry::Geometry::default());
        let fill2d_pipeline = gfx::fill_pipeline_2d(&resouce, &cam2d_bind, &dummy, 1);
        let line2d_pipeline = gfx::line_pipeline_2d(&resouce, &cam2d_bind, &dummy, 1);
        let fill3d_pipeline = gfx::fill_pipeline_3d(&resouce, &cam3d_bind, &dummy, 1);

        self._context = Some(_context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);
        self.cam2d_buffer = Some(cam2d_buffer);
        self.cam2d_bind = Some(cam2d_bind);
        self.cam3d_buffer = Some(cam3d_buffer);
        self.cam3d_bind = Some(cam3d_bind);
        self.fill2d = fill2d;
        self.line2d = line2d;
        self.fill3d = fill3d;
        self.fill2d_pipeline = Some(fill2d_pipeline);
        self.line2d_pipeline = Some(line2d_pipeline);
        self.fill3d_pipeline = Some(fill3d_pipeline);
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        // 帧时间（引擎时钟每帧前更新，替代原 clock.tick(120) 的返回值）
        let dt = ctx.delta();
        self.orbit += dt * 0.5;
        let orbit = self.orbit;

        let resouce = self.resouce.as_ref().unwrap();
        let surface = self.surface.as_mut().unwrap();

        // 相机矩阵
        let ortho = Mat4::from_cols_array(&[
            2.0 / 1280.0, 0.0, 0.0, 0.0,
            0.0, -2.0 / 720.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            -1.0, 1.0, 0.0, 1.0,
        ]);
        let eye = Vec3::new(orbit.sin() * 9.0, 4.5, orbit.cos() * 9.0);
        let view3d = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.8, 0.0), Vec3::Y);
        let proj3d = Mat4::perspective_rh(
            45.0f32.to_radians(),
            1280.0 / 720.0,
            0.1,
            100.0,
        );

        surface.begin_frame(wgpu::Color { r: 0.06, g: 0.07, b: 0.1, a: 1.0 }, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let depth_attachment = surface.get_current_depth_attachment();
        let mut encoder = resouce.create_command_encoder();
        let color_atts = [&color_attachment];

        // ── Pass 1：3D 形状（带深度附件；深度测试 + 写入，可被互相遮挡） ──
        let mvp3d = proj3d * view3d;
        let cam3d_buffer = self.cam3d_buffer.as_ref().unwrap();
        resouce.write_buffer(cam3d_buffer, 0, bytemuck::bytes_of(&mvp3d.to_cols_array_2d()));
        {
            let mut pass = encoder.begin_render_pass(
                "gfx_3d_pass",
                &color_atts,
                depth_attachment,
                None,
                None,
                None,
            );
            pass.set_pipeline(self.fill3d_pipeline.as_ref().unwrap());
            pass.set_bind_group(0, self.cam3d_bind.as_ref().unwrap());
            for mesh in &self.fill3d {
                pass.draw_mesh(mesh);
            }
            pass.end();
        }

        // ── Pass 2：2D 形状（无深度附件 → 无深度管线可用；颜色 Load 不清屏，浮于 3D 之上） ──
        let cam2d_buffer = self.cam2d_buffer.as_ref().unwrap();
        resouce.write_buffer(cam2d_buffer, 0, bytemuck::bytes_of(&ortho.to_cols_array_2d()));
        {
            let mut pass = encoder.begin_render_pass(
                "gfx_2d_pass",
                &color_atts,
                None,
                None,
                None,
                None,
            );
            pass.set_pipeline(self.fill2d_pipeline.as_ref().unwrap());
            pass.set_bind_group(0, self.cam2d_bind.as_ref().unwrap());
            for mesh in &self.fill2d {
                pass.draw_mesh(mesh);
            }
            pass.set_pipeline(self.line2d_pipeline.as_ref().unwrap());
            for mesh in &self.line2d {
                pass.draw_mesh(mesh);
            }
            pass.end();
        }

        let cmd = encoder.finish();
        surface.submit([cmd]);
        surface.present();
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
mod entry {
    use super::GfxShapesApp;
    use starfish::base::app::{run_android, WindowConfig};

    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        run_android(
            app,
            GfxShapesApp::new(),
            WindowConfig::new("gfx_shapes", 1280, 720).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    run(
        GfxShapesApp::new(),
        WindowConfig::new("GFX Shapes", 1280, 720)
            .with_resizable(false)
            .with_fps_cap(120),
    );
}
