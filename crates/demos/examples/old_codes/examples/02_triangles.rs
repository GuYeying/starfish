use std::fs;
use bytemuck::cast_slice;
use sdl3::{event::Event, video::WindowFlags};
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::subsystem::{EventSubsystem, VideoSubsystem};
use starfish::base::window::Window;
use starfish::base::resources::shader::Shader;
use wgpu::Color;
fn main() {
    // SDL 初始化
    let sdl = sdl3::init().expect("SDL init failed.");
    let video = VideoSubsystem::new(&sdl);
    let _event = EventSubsystem::new(&sdl);
    // 窗口
    let window = Window::new(
        &video,
        "RGB Triangle Demo",
        (800, 600),
        WindowFlags::default(),
    )
    .expect("Window creation failed");
    // 渲染上下文
    let (context,resouce,mut surface) = RenderEntry::new(&window, None, None)
        .expect("RenderContext 初始化失败");
    // 1. 纯色三角形着色器（无贴图）
    let shader_source: String = fs::read_to_string("resources/shaders/triangle.wgsl").unwrap();
    let shader = resouce.shader_module_builder(Shader::new(shader_source))
        .build(Some("triangle_shader"));
    // 2. 三角形顶点数据：pos xyz + color rgb，共3个顶点，无索引
    let tri_verts: &[f32] = &[
        0.0,  0.5, 0.0, 1.0, 0.0, 0.0,
        -0.5, -0.5, 0.0, 0.0, 1.0, 0.0,
        0.5, -0.5, 0.0, 0.0, 0.0, 1.0,
    ];
    // 顶点布局：仅位置+颜色，无UV
    let layout = vec![wgpu::VertexFormat::Float32x3,wgpu::VertexFormat::Float32x3,];
    // 构建Mesh：不使用索引缓冲区
    let mesh = resouce.mesh_builder(layout, cast_slice(tri_verts).to_vec())
        .build( Some("tri_vb"), Some("tri_ib"));
    // 3. 材质：不需要纹理、采样器
    let bind_group = resouce.bind_group_builder()
        .build(Some("tri_bind_group"));
    // 4. 2D管线（无深度）
    let pipeline = resouce.render_pipeline_builder_2d(&shader)
        .build(&[&bind_group], &mesh, Some("tri_pipeline"));
    // 主循环
    let mut running = true;
    while running {
        for event in sdl.event_pump().unwrap().poll_iter() {
            match event {
                Event::Quit { .. } => running = false,
                _ => {}
            }
        }
        surface.begin_frame(Color {r: 0.1, g: 0.1, b: 0.15, a: 1.0,},1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder_draw = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut pass = encoder_draw.begin_render_pass(
                "triangle_pass",
                &color_atts,
                None,
                None,
                None,
                None,
            );
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group);
            pass.set_mesh(&mesh);
            pass.draw(0..mesh.vertex_count(), 0..1);
            pass.end();
        let cmd_draw = encoder_draw.finish();
        surface.submit([cmd_draw]);
        surface.present();
        //timer::delay(16);
    }
}