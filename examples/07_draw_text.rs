//! 文本渲染演示：Font → 图集 → 标准渲染对象 → 02-04 同构绘制
//!
//! 运行：cargo run --example 07_draw_text
//!
//! 与 triangles/texture 示例完全同构的调用风格：
//! shader / bind_group / mesh / pipeline 全部是标准渲染对象，
//! 绘制走 pass.set_pipeline + set_bind_group + set_mesh + draw。
//!
//! 展示：
//! - 字体加载与图集构建（build_atlas）
//! - 文本网格 = 标准 Mesh（实时更新走 resouce.write_vertex_buffer）
//! - 120 FPS 帧控

use bytemuck::cast_slice;
use glam::{Mat4, Vec3};
use sdl3::{event::Event, video::WindowFlags};
use starfish::base::font::{self, Font};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::subsystem::{EventSubsystem, VideoSubsystem};
use starfish::base::time::Clock;
use starfish::base::window::Window;
use wgpu::BufferUsages;

fn main() {
    let sdl = sdl3::init().expect("SDL init failed.");
    let video = VideoSubsystem::new(&sdl);
    let _event = EventSubsystem::new(&sdl);
    let window = Window::new(&video, "Text Demo", (800, 600), WindowFlags::default())
        .expect("Window creation failed");

    let (_context, resouce, mut surface) =
        RenderEntry::new(&window, None, None).expect("RenderContext 初始化失败");

    // ── 1. 字体：加载 → 图集（一次构建，多次渲染） ──
    let font = Font::from_file("resources/fonts/Antonio-Regular.ttf", 48.0)
        .expect("字体加载失败");
    let sample = "Hello, starfish! 0123456789 .,!?";
    let atlas = font
        .build_atlas(sample.chars(), &resouce)
        .expect("图集构建失败");
    println!(
        "图集 {}×{}，字形 {} 个，行高 {:.1}px",
        atlas.size[0],
        atlas.size[1],
        atlas.glyphs.len(),
        atlas.line_height
    );

    // ── 2. 标准渲染对象（与 02-04 示例同构） ──
    // 相机 uniform（04 同款）：裸缓冲 + bind group，MVP 由开发者计算写入
    let camera_buffer = resouce.create_raw_buffer(
        Some("text_camera"),
        64,
        BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    );
    let camera_bind = resouce
        .bind_group_builder()
        .uniform_raw(0, camera_buffer.clone(), 64)
        .build(Some("text_camera_bind"));
    let atlas_bind = font::atlas_bind_group(&resouce, &atlas);

    // 文本网格 = 标准 Mesh；构建一次长期复用，
    // 放置矩阵（平移/旋转/缩放，glam 合成）；实时更新走 resouce.write_vertex_buffer
    let hello_mesh = font::text_mesh_tf(
        &resouce,
        &atlas,
        "Hello, starfish!",
        &Mat4::from_translation(Vec3::new(40.0, 80.0, 0.0)),
        1.0,
        [1.0, 1.0, 1.0, 1.0],
    );
    let digits_mesh = font::text_mesh_tf(
        &resouce,
        &atlas,
        "0123456789 .,!? px*2",
        &Mat4::from_translation(Vec3::new(40.0, 160.0, 0.0)),
        2.0,
        [1.0, 0.65, 0.25, 1.0],
    );
    let sample_mesh = font::text_mesh_tf(
        &resouce,
        &atlas,
        sample,
        &Mat4::from_translation(Vec3::new(40.0, 280.0, 0.0)),
        1.0,
        [0.5, 0.8, 1.0, 1.0],
    );

    // 文本渲染管线
    let pipeline = font::text_pipeline(&resouce, &camera_bind, &atlas_bind, &hello_mesh, 1);

    // ── 3. HUD 相机（04 同款）：像素正交投影（y 向下）+ 恒等 view/model ──
    // 世界空间文本/billboard：projection 换游戏透视投影，model 传面向相机的逐对象变换
    let projection = Mat4::from_cols_array(&[
        2.0 / 800.0, 0.0, 0.0, 0.0,
        0.0, -2.0 / 600.0, 0.0, 0.0,
        0.0, 0.0, 1.0, 0.0,
        -1.0, 1.0, 0.0, 1.0,
    ]);
    let view = Mat4::IDENTITY;
    let model = Mat4::IDENTITY;

    // ── 4. 帧率控制：120 FPS ──
    let mut clock = Clock::new();

    let mut running = true;
    while running {
        let _dt = clock.tick(120);

        for event in sdl.event_pump().unwrap().poll_iter() {
            if let Event::Quit { .. } = event {
                running = false;
            }
        }

        surface.begin_frame(wgpu::Color { r: 0.08, g: 0.09, b: 0.14, a: 1.0 }, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = resouce.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass(
            "text_pass",
            &color_atts,
            None,
            None,
            None,
            None,
        );

        // MVP = projection × view × model，写入相机 uniform
        let mvp = projection * view * model;
        resouce.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&mvp.to_cols_array_2d()));

        // 02-04 同款绘制调用
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &camera_bind);
        pass.set_bind_group(1, &atlas_bind);
        pass.set_mesh(&hello_mesh);
        pass.draw(0..hello_mesh.vertex_count(), 0..1);
        pass.set_mesh(&digits_mesh);
        pass.draw(0..digits_mesh.vertex_count(), 0..1);
        pass.set_mesh(&sample_mesh);
        pass.draw(0..sample_mesh.vertex_count(), 0..1);

        pass.end();
        let cmd = encoder.finish();
        surface.submit([cmd]);
        surface.present();
    }

    // 布局函数也可以直接产顶点数据（纯数据，不经过 Mesh/GPU）
    let _raw_verts: Vec<font::TextVertex> =
        font::layout_text(&atlas, "raw data", [0.0, 0.0, 0.0], 1.0, [1.0; 4]);
    let _ = cast_slice::<font::TextVertex, u8>(&_raw_verts).len();
}
