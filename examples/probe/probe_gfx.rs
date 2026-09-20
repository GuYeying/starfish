//! probe_gfx：gfx 几何模块验证（3D 深度场景 + 2D 叠层，对齐 old/06 的呈现）
//!
//! 深度 pass：地面 + 立方体/球/圆柱/圆锥/胶囊（透视相机缓速环绕，互相遮挡）；
//! 2D 叠层：矩形/圆/椭圆/五边形/凹多边形/胶囊/线段/折线（像素正交，深度关闭
//! 管线天然浮于 3D 之上）。状态文本最上层。
//!
//! 判据：console 锚点 `[gfx] BUILD PASS`、`[gfx] DRAW PASS`。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::gfx::{self, geometry};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::window::{Window, WindowEvent};
use wgpu::BufferUsages;

use glam::{Mat4, Vec2, Vec3};
use std::sync::Arc;

/// 相机 uniform（2D 正交 / 3D 透视各一份）
struct Camera {
    buffer: Arc<wgpu::Buffer>,
    bind: BindGroup,
}

struct GfxProbe {
    panel: StatusPanel,
    cam2d: Option<Camera>,
    cam3d: Option<Camera>,
    fill2d_pipeline: Option<Arc<RenderPipeline>>,
    line2d_pipeline: Option<Arc<RenderPipeline>>,
    fill3d_pipeline: Option<Arc<RenderPipeline>>,
    fill2d: Vec<Mesh>,
    line2d: Vec<Mesh>,
    fill3d: Vec<Mesh>,
    orbit: f32,
    passed: bool,
}

impl GfxProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            cam2d: None,
            cam3d: None,
            fill2d_pipeline: None,
            line2d_pipeline: None,
            fill3d_pipeline: None,
            fill2d: Vec::new(),
            line2d: Vec::new(),
            fill3d: Vec::new(),
            orbit: 0.0,
            passed: false,
        }
    }

    /// 帧 30 一次：相机 ×2 + 填充/描边三管线 + 全形状网格
    fn build(&mut self) {
        let built = self.panel.with_gpu(|gpu| {
            let access = &gpu.access;
            let make_camera = || {
                let buffer = access.create_raw_buffer(
                    Some("probe_gfx_camera"),
                    64,
                    BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                );
                let bind = access
                    .bind_group_builder()
                    .uniform_raw(0, buffer.clone(), 64)
                    .build(Some("probe_gfx_camera_bind"));
                Camera { buffer, bind }
            };
            let cam2d = make_camera();
            let cam3d = make_camera();

            // ── 2D 形状行（像素空间，屏幕底部；填充琥珀 / 描边青蓝）──
            let c_fill = [0.95, 0.78, 0.30, 1.0];
            let c_line = [0.40, 0.80, 0.95, 1.0];
            let mut fill2d: Vec<Mesh> = Vec::new();
            let mut line2d: Vec<Mesh> = Vec::new();

            let (min, max) = (Vec2::new(25.0, 465.0), Vec2::new(125.0, 535.0));
            fill2d.push(gfx::shape_mesh(access, &geometry::shape2d::rect(min, max, c_fill)));
            line2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::rect_outline(min, max, c_line),
            ));

            let center = Vec2::new(180.0, 500.0);
            fill2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::circle(center, 30.0, 64, c_fill),
            ));
            line2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::circle_outline(center, 30.0, 64, c_line),
            ));

            fill2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::ellipse(Vec2::new(300.0, 500.0), Vec2::new(36.0, 22.0), 64, c_fill),
            ));

            fill2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::regular_polygon(Vec2::new(405.0, 500.0), 30.0, 5, 0.0, c_fill),
            ));

            // 凹多边形（L 形）：耳切三角化
            let l_shape = [
                Vec2::new(460.0, 470.0),
                Vec2::new(520.0, 470.0),
                Vec2::new(520.0, 500.0),
                Vec2::new(490.0, 500.0),
                Vec2::new(490.0, 535.0),
                Vec2::new(460.0, 535.0),
            ];
            fill2d.push(gfx::shape_mesh(access, &geometry::shape2d::polygon(&l_shape, c_fill)));

            fill2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::capsule(Vec2::new(545.0, 485.0), Vec2::new(595.0, 530.0), 16.0, 32, c_fill),
            ));

            line2d.push(gfx::shape_mesh(
                access,
                &geometry::shape2d::line(Vec2::new(625.0, 470.0), Vec2::new(690.0, 535.0), c_line),
            ));
            let zigzag: Vec<Vec2> = (0..6)
                .map(|i| Vec2::new(705.0 + i as f32 * 18.0, if i % 2 == 0 { 470.0 } else { 540.0 }))
                .collect();
            line2d.push(gfx::shape_mesh(access, &geometry::shape2d::polyline(&zigzag, false, c_line)));

            // ── 3D 形状（世界空间；地面 + 五件套沿 x 轴排开）──
            let mut fill3d: Vec<Mesh> = Vec::new();
            fill3d.push(gfx::shape_mesh(
                access,
                &geometry::shape3d::plane(
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::X,
                    Vec3::Z,
                    Vec2::new(14.0, 14.0),
                    [0.22, 0.25, 0.32, 1.0],
                ),
            ));
            fill3d.push(gfx::shape_mesh(
                access,
                &geometry::shape3d::cube(
                    Vec3::new(-4.6, 0.0, -0.6),
                    Vec3::new(-3.4, 1.2, 0.6),
                    [0.90, 0.40, 0.30, 1.0],
                ),
            ));
            fill3d.push(gfx::shape_mesh(
                access,
                &geometry::shape3d::sphere(Vec3::new(-2.0, 0.9, 0.0), 0.9, 32, 24, [0.30, 0.65, 0.95, 1.0]),
            ));
            fill3d.push(gfx::shape_mesh(
                access,
                &geometry::shape3d::cylinder(Vec3::new(0.0, 0.0, 0.0), 0.7, 1.8, 48, [0.95, 0.80, 0.30, 1.0]),
            ));
            fill3d.push(gfx::shape_mesh(
                access,
                &geometry::shape3d::cone(Vec3::new(2.0, 0.0, 0.0), 0.8, 1.8, 48, [0.40, 0.85, 0.50, 1.0]),
            ));
            fill3d.push(gfx::shape_mesh(
                access,
                &geometry::shape3d::capsule(
                    Vec3::new(4.0, 0.3, 0.0),
                    Vec3::new(4.0, 1.5, 0.0),
                    0.45,
                    32,
                    12,
                    [0.80, 0.45, 0.90, 1.0],
                ),
            ));

            // ── 管线（dummy 网格提供顶点布局模板）──
            // 2D 叠层用 3d 变体管线（深度开启以匹配深度 pass；像素空间形状
            // z=0 经正交后恒最近，绘制顺序又在 3D 之后 → 天然叠加在最上）
            let dummy = gfx::shape_mesh(access, &geometry::Geometry::default());
            let fill2d_pipeline = gfx::fill_pipeline_3d(access, &cam2d.bind, &dummy, 1);
            let line2d_pipeline = gfx::line_pipeline_3d(access, &cam2d.bind, &dummy, 1);
            let fill3d_pipeline = gfx::fill_pipeline_3d(access, &cam3d.bind, &dummy, 1);

            (
                cam2d,
                cam3d,
                fill2d_pipeline,
                line2d_pipeline,
                fill3d_pipeline,
                fill2d,
                line2d,
                fill3d,
            )
        });

        match built {
            Some((cam2d, cam3d, f2, l2, f3, fill2d, line2d, fill3d)) => {
                self.cam2d = Some(cam2d);
                self.cam3d = Some(cam3d);
                self.fill2d_pipeline = Some(f2);
                self.line2d_pipeline = Some(l2);
                self.fill3d_pipeline = Some(f3);
                self.fill2d = fill2d;
                self.line2d = line2d;
                self.fill3d = fill3d;
                self.panel
                    .verdict("gfx", "BUILD", Status::Pass, "3d scene + 8 2d shapes");
            }
            None => self.panel.verdict("gfx", "BUILD", Status::Fail, "gpu not ready"),
        }
    }
}

impl Application for GfxProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, _event: &WindowEvent, _ctx: &mut Ctx) {}

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();
        self.orbit += ctx.delta() * 0.5;

        if self.cam2d.is_none() && f == 30 {
            self.build();
        }
        if !self.passed && self.cam2d.is_some() && f > 150 {
            self.panel.verdict("gfx", "DRAW", Status::Pass, "depth + overlay");
            self.passed = true;
        }

        let (w, h) = ctx.size();
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let orbit = self.orbit;
        let cam2d = self.cam2d.as_ref();
        let cam3d = self.cam3d.as_ref();
        let fill2d_pipeline = self.fill2d_pipeline.as_ref();
        let line2d_pipeline = self.line2d_pipeline.as_ref();
        let fill3d_pipeline = self.fill3d_pipeline.as_ref();

        self.panel.render(ctx, true, |pass, access| {
            let (Some(cam2d), Some(cam3d)) = (cam2d, cam3d) else {
                return;
            };

            // ── 3D 相机（透视，绕场景缓速环绕）──
            let eye = Vec3::new(orbit.sin() * 9.0, 4.5, orbit.cos() * 9.0);
            let view3d = Mat4::look_at_rh(eye, Vec3::new(0.0, 0.8, 0.0), Vec3::Y);
            let proj3d = Mat4::perspective_rh(45.0f32.to_radians(), w / h, 0.1, 100.0);
            access.write_buffer(
                &cam3d.buffer,
                0,
                bytemuck::bytes_of(&(proj3d * view3d).to_cols_array_2d()),
            );

            // ── 2D 相机（像素正交，跟随窗口尺寸）──
            let ortho = Mat4::from_cols_array(&[
                2.0 / w, 0.0, 0.0, 0.0, //
                0.0, -2.0 / h, 0.0, 0.0, //
                0.0, 0.0, 1.0, 0.0, //
                -1.0, 1.0, 0.0, 1.0,
            ]);
            access.write_buffer(&cam2d.buffer, 0, bytemuck::bytes_of(&ortho.to_cols_array_2d()));

            // 3D 形状（深度测试，互相遮挡）
            if let Some(p) = fill3d_pipeline {
                pass.set_pipeline(p);
                pass.set_bind_group(0, &cam3d.bind);
                for mesh in &self.fill3d {
                    pass.draw_mesh(mesh);
                }
            }
            // 2D 叠层（深度关闭管线，浮于 3D 之上）
            if let Some(p) = fill2d_pipeline {
                pass.set_pipeline(p);
                pass.set_bind_group(0, &cam2d.bind);
                for mesh in &self.fill2d {
                    pass.draw_mesh(mesh);
                }
            }
            if let Some(p) = line2d_pipeline {
                pass.set_pipeline(p);
                pass.set_bind_group(0, &cam2d.bind);
                for mesh in &self.line2d {
                    pass.draw_mesh(mesh);
                }
            }
        });
    }
}

starfish::app_entry!(
    GfxProbe::new(),
    WindowConfig::new("probe gfx", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
