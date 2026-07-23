use std::{fs, sync::Arc};
use bytemuck::cast_slice;
use sdl3::{event::Event, video::WindowFlags};
use image::ImageReader;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::sampler_desc::SamplerDescriptor;
use starfish::base::render::settings::{GpuSettings, SurfaceSettings};
use starfish::base::render::texture::{TextureDescriptor, TextureDim, TextureSemantic, TextureUsage};
use starfish::base::resources::image::ImageData;
use starfish::base::subsystem::{EventSubsystem, VideoSubsystem};
use starfish::base::window::Window;
use starfish::base::{resources::shader::Shader};
use wgpu::{Color, InstanceFlags, MemoryHints, PowerPreference, TextureUsages};

fn main() {
    // 窗口创建前第一行执行
    sdl3::hint::set("SDL_VIDEO_EXTERNAL_CONTEXT", "1");
    // 可选：禁用SDL渲染器自动初始化
    sdl3::hint::set("SDL_RENDER_DRIVER", "none");

    // SDL 初始化
    let sdl = sdl3::init().expect("SDL init failed.");
    //子系统初始化
    let video = VideoSubsystem::new(&sdl);
    let _event = EventSubsystem::new(&sdl);
    //窗口创建
    let window = Window::new(&video, "Texture Demo", (800, 600), WindowFlags::HIDDEN).expect("Window creation failed: abnormal window parameters/graphics card/system permissions");
    // 异步创建渲染上下文，处理 Result
    let surface_settings = SurfaceSettings::default()
        // 移除 TEXTURE_BINDING / COPY_SRC，仅保留基础渲染附件，交换链显存最小
        .with_usage(TextureUsages::RENDER_ATTACHMENT)
        // 帧延迟保持2（三缓冲是平衡底线，设1会卡顿，没必要牺牲流畅换少量内存）
        .with_frame_latency(2);
    let gpu_settings = GpuSettings::default()
        .with_power_preference(PowerPreference::LowPower)
        .with_flags(InstanceFlags::empty())
        .with_memory_hints(MemoryHints::MemoryUsage)
        .with_depth(false);

    let (context,resouce,mut surface) = RenderEntry::new(&window, Some(surface_settings), Some(gpu_settings))
        .expect("RenderContext 初始化失败");
    //  1: Shader
    let shader_source: String = fs::read_to_string("resources/shaders/texture.wgsl").unwrap();
    let shader = resouce.shader_module_builder(Shader::new(shader_source))
        .build(Some("shader"));
    // 正方形唯一4顶点，格式：x,y,z r,g,b u,v（每组8个f32不变）
    let square_verts: &[f32] = &[
        // 0 左上
        -0.5,  0.5, 0.0, 1.0,0.0,0.0, 0.0,0.0,
        // 1 右上
        0.5,  0.5, 0.0, 0.0,1.0,0.0, 1.0,0.0,
        // 2 左下
        -0.5, -0.5, 0.0, 0.0,0.0,1.0, 0.0,1.0,
        // 3 右下
        0.5, -0.5, 0.0, 1.0,1.0,0.0, 1.0,1.0,
    ];
    let square_indices: Vec<u32> = vec![0, 3, 1, 0, 2, 3];
    //  2:Mesh
    let layout = vec![
        wgpu::VertexFormat::Float32x3,
        wgpu::VertexFormat::Float32x3,
        wgpu::VertexFormat::Float32x2,
    ];
    let mesh = resouce.mesh_builder(layout, cast_slice(square_verts).to_vec())
                        .with_indices(square_indices)
                        .build(Some("label_vertex"),Some("label_index"));

    //  3,BindGroup
    let texture_desc = TextureDescriptor::new(
        TextureSemantic::Color,
        TextureUsage::Sampled,
        TextureDim::D2, 
        None,
        None
    );
    // 创建GPU纹理资源
    let texture = {
        let img: image::ImageBuffer<image::Rgba<u8>, Vec<u8>> = ImageReader::open("resources/textures/wall.jpg").unwrap().decode().unwrap().into_rgba8();
        let image = ImageData::Rgba8(img);
        Arc::new(resouce.create_texture("test_texture",&image,texture_desc))
    };
    // 创建线性采样器
    let sampler = {
        let sampler_config = SamplerDescriptor::default();
        Arc::new(resouce.create_sampler(
            "linear_sampler",
            &sampler_config
        ))
    };
    let bind_group = resouce.bind_group_builder()
        .texture(0, texture)
        .sampler(1,sampler)
        .build(Some("texture_label"));

    //  4.Pipeline
    let pipeline = resouce.render_pipeline_builder_2d(&shader)
        .build(&[&bind_group], &mesh, Some("pipeline"));
    let mut running = true;
    window.set_visible(true);
    while running {
        for event in sdl.event_pump().unwrap().poll_iter() {
            match event {
                Event::Quit { .. } => running = false,
                _ => {}
            }
        }
        surface.begin_frame(Color { r: 0.2, g: 0.3, b: 0.3, a: 1.0 },1.0);//内部创建encoder_draw进行清空内容
        let color_attachment = surface.get_current_color_attachment();
        let _depth_attachment = surface.get_current_depth_attachment();
        let mut encoder_draw = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut pass = encoder_draw.begin_render_pass(
                "render_pass",
                &color_atts,
                None,
                None,
                None,
                None,
            );
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0,&bind_group);
            pass.draw_mesh(&mesh);
            pass.end();
        let cmd_draw = encoder_draw.finish();
        surface.submit([cmd_draw]);
        surface.present();
    }
}
