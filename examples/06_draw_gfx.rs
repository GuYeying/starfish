//! 几何绘制演示：gfx 全形状渲染
//!
//! 运行：cargo run --example 06_draw_gfx
//!
//! 上半屏：2D 形状（像素空间正交相机；填充 + 1px 描边）
//! 全屏  ：3D 形状（透视相机绕场景缓速环绕，深度测试开启，可被地面遮挡）
//!
//! 覆盖 gfx 全部形状：rect / rect_outline / circle / circle_outline /
//! ellipse / line / polyline / regular_polygon / polygon / capsule2d /
//! cube / sphere / plane / cylinder / cone / capsule3d

use bytemuck;
use glam::{Mat4, Vec2, Vec3};
use sdl3::{event::Event, video::WindowFlags};
use starfish::base::gfx::{self, geometry};
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::subsystem::{EventSubsystem, VideoSubsystem};
use starfish::base::time::Clock;
use starfish::base::window::Window;
use wgpu::BufferUsages;

fn main() {
    let sdl = sdl3::init().expect("SDL init failed.");
    let video = VideoSubsystem::new(&sdl);
    let _event = EventSubsystem::new(&sdl);
    let window = Window::new(&video, "GFX Shapes", (1280, 720), WindowFlags::default())
        .expect("Window creation failed");

    let (_context, resouce, mut surface) =
        RenderEntry::new(&window, None, None).expect("RenderContext 初始化失败");

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

    // ── 帧率控制：120 FPS ──
    let mut clock = Clock::new();
    let mut orbit: f32 = 0.0;

    let mut running = true;
    while running {
        let dt = clock.tick(120);
        orbit += dt * 0.5;

        for event in sdl.event_pump().unwrap().poll_iter() {
            if let Event::Quit { .. } = event {
                running = false;
            }
        }

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
        resouce.write_buffer(&cam3d_buffer, 0, bytemuck::bytes_of(&mvp3d.to_cols_array_2d()));
        {
            let mut pass = encoder.begin_render_pass(
                "gfx_3d_pass",
                &color_atts,
                depth_attachment,
                None,
                None,
                None,
            );
            pass.set_pipeline(&fill3d_pipeline);
            pass.set_bind_group(0, &cam3d_bind);
            for mesh in &fill3d {
                pass.draw_mesh(mesh);
            }
            pass.end();
        }

        // ── Pass 2：2D 形状（无深度附件 → 无深度管线可用；颜色 Load 不清屏，浮于 3D 之上） ──
        resouce.write_buffer(&cam2d_buffer, 0, bytemuck::bytes_of(&ortho.to_cols_array_2d()));
        {
            let mut pass = encoder.begin_render_pass(
                "gfx_2d_pass",
                &color_atts,
                None,
                None,
                None,
                None,
            );
            pass.set_pipeline(&fill2d_pipeline);
            pass.set_bind_group(0, &cam2d_bind);
            for mesh in &fill2d {
                pass.draw_mesh(mesh);
            }
            pass.set_pipeline(&line2d_pipeline);
            for mesh in &line2d {
                pass.draw_mesh(mesh);
            }
            pass.end();
        }

        let cmd = encoder.finish();
        surface.submit([cmd]);
        surface.present();
    }
}
