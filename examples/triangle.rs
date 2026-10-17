use std::fs;
use bytemuck::cast_slice;
use starfish::base::render::settings::SurfaceSettings;
use starfish::base::render::{render_entry::RenderEntry, settings::GpuSettings};
use starfish::base::time::Clock;
use starfish::base::window::Window;
use starfish::base::resources::shader::Shader;
use starfish_window::Event;
use wgpu::{Backends, Color, InstanceFlags, MemoryHints, PowerPreference, TextureUsages};
use starfish::base::window::next_frame;

async fn app_body() -> Result<(), Box<dyn std::error::Error>> {
    // 窗口
    let window = Window::builder()
        .title("starfish-rewrite hello")
        .size((800, 600))
        .build()
        .expect("建窗失败");
    let surface_settings = SurfaceSettings::default()
        .with_usage(TextureUsages::RENDER_ATTACHMENT)
        .with_frame_latency(2);
    let gpu_settings = GpuSettings::default()
        .with_backends(Backends::GL)
        .with_power_preference(PowerPreference::LowPower)
        .with_flags(InstanceFlags::empty())
        .with_memory_hints(MemoryHints::MemoryUsage);
    // 渲染上下文
    let (_context,resouce,mut surface) = RenderEntry::new(&window, Some(surface_settings), Some(gpu_settings))
        .expect("RenderContext 初始化失败");
    // 1. 纯色三角形着色器（无贴图）
    let shader_source: String = fs::read_to_string("examples/triangle.wgsl").unwrap();
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

    let mut clock = Clock::new();
    
    // 主循环
    loop {
        clock.tick(120);
        while let Some(event) = window.poll_event() {
            match event {
                Event::CloseRequested => return Ok(()),
                Event::Resized { width, height } => surface.resize(width, height),
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
        next_frame().await;

    }
}
starfish::app_entry!(app_body());