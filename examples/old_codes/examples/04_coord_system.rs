use std::{fs, sync::{Arc}};
use bytemuck::{cast_slice};
use sdl3::{event::Event, timer, video::WindowFlags};
use image::ImageReader;
use glam::{Mat4, Vec3};
use wgpu::{
    AddressMode, Color, FilterMode, InstanceFlags, MemoryHints, MipmapFilterMode,
    PowerPreference, TextureUsages,
};
use starfish::base::{
    render::{
        bind_group::{
            field_type::StructType, field_value::StructValue, struct_layout::StructLayout, uniform_buffer::UniformBuffer, 
        }, render_entry::RenderEntry, sampler_desc::SamplerDescriptor, settings::{GpuSettings, SurfaceSettings}, texture::{TextureDescriptor, TextureDim, TextureSemantic, TextureUsage},
    }, resources::{image::ImageData, shader::Shader}, subsystem::{EventSubsystem, VideoSubsystem}, window::Window,
};
use glam::camera::rh::view::look_at_mat4;
use glam::camera::rh::proj::directx::perspective;

// ===================== 立方体顶点（唯一24个顶点：xyz uv）=====================
// 每个面4个唯一顶点，6面合计24个，通过索引复用
const CUBE_UNIQUE_VERTS: &[f32] = &[

    // ---------- Front (+Z)
    -0.5,-0.5, 0.5, 0.0,0.0,
     0.5,-0.5, 0.5, 1.0,0.0,
     0.5, 0.5, 0.5, 1.0,1.0,
    -0.5, 0.5, 0.5, 0.0,1.0,

    // ---------- Back (-Z)
     0.5,-0.5,-0.5, 0.0,0.0,
    -0.5,-0.5,-0.5, 1.0,0.0,
    -0.5, 0.5,-0.5, 1.0,1.0,
     0.5, 0.5,-0.5, 0.0,1.0,

    // ---------- Left (-X)
    -0.5,-0.5,-0.5, 0.0,0.0,
    -0.5,-0.5, 0.5, 1.0,0.0,
    -0.5, 0.5, 0.5, 1.0,1.0,
    -0.5, 0.5,-0.5, 0.0,1.0,

    // ---------- Right (+X)
     0.5,-0.5, 0.5, 0.0,0.0,
     0.5,-0.5,-0.5, 1.0,0.0,
     0.5, 0.5,-0.5, 1.0,1.0,
     0.5, 0.5, 0.5, 0.0,1.0,

    // ---------- Top (+Y)
    -0.5, 0.5, 0.5, 0.0,0.0,
     0.5, 0.5, 0.5, 1.0,0.0,
     0.5, 0.5,-0.5, 1.0,1.0,
    -0.5, 0.5,-0.5, 0.0,1.0,

    // ---------- Bottom (-Y)
    -0.5,-0.5,-0.5, 0.0,0.0,
     0.5,-0.5,-0.5, 1.0,0.0,
     0.5,-0.5, 0.5, 1.0,1.0,
    -0.5,-0.5, 0.5, 0.0,1.0,

];
// ===================== 立方体索引数组 u16 =====================
// 每个面2个三角形(6索引)，6个面合计36索引，对应上面24个唯一顶点
const CUBE_INDICES: &[u16] = &[
    // Front
    0, 1, 2,
    0, 2, 3,
    // Back
    4, 5, 6,
    4, 6, 7,
    // Left
    8, 9,10,
    8,10,11,
    // Right
    12,13,14,
    12,14,15,
    // Top
    16,17,18,
    16,18,19,
    // Bottom
    20,21,22,
    20,22,23,
];//98-32 = 60
fn main() {
    // SDL 初始化
    let sdl = sdl3::init().expect("SDL init failed.");
    let video_subsys = VideoSubsystem::new(&sdl);
    let _event_subsys = EventSubsystem::new(&sdl);
    // 窗口参数
    let win_size = (800, 600);
    let window = Window::new(&video_subsys, "Indexed Cube 3D Demo", win_size, WindowFlags::default())
        .expect("Window creation failed: abnormal window parameters/graphics card/system permissions");
    // 渲染上下文配置
    let surface_settings = SurfaceSettings::default()
        .with_usage(TextureUsages::RENDER_ATTACHMENT)
        .with_frame_latency(2);
    let gpu_settings = GpuSettings::default()
        .with_power_preference(PowerPreference::LowPower)
        .with_flags(InstanceFlags::empty())
        .with_memory_hints(MemoryHints::MemoryUsage)
        .with_depth(true); // 3D深度缓冲开启

    let (context,resouce,mut surface) = RenderEntry::new(&window, Some(surface_settings), Some(gpu_settings))
        .expect("RenderContext 初始化失败");
    // ===================== 1. 着色器模块 =====================
    let shader_source = fs::read_to_string("resources/shaders/coord_system.wgsl").unwrap();
    let shader = resouce.shader_module_builder(Shader::new(shader_source))
        .build(Some("cube_transform_shader"));
    // ===================== 2. 网格构建【核心修改：传入索引缓冲】 =====================
    // 顶点布局：Position(f32x3) + UV(f32x2)
    let vertex_layout = vec![
        wgpu::VertexFormat::Float32x3,
        wgpu::VertexFormat::Float32x2,
    ];
    // 关键改动：第三个参数为顶点数据，第四个参数传入索引字节切片（不再是None）
    let cube_mesh = resouce.mesh_builder(vertex_layout, cast_slice(CUBE_UNIQUE_VERTS).to_vec())
        .with_short_indices(cast_slice(CUBE_INDICES).to_vec())
        .build(Some("cube_indexed_mesh"), None);
    // ===================== 3. 纹理 + 采样器 + 纹理BindGroup =====================
    let texture_desc = TextureDescriptor::new(
        TextureSemantic::Color,
        TextureUsage::Sampled,
        TextureDim::D2,
        None,
        None,
    );
    // 加载纹理
    let img = ImageReader::open("resources/textures/container.jpg")
        .unwrap()
        .decode()
        .unwrap()
        .into_rgba8();
    let image = ImageData::Rgba8(img);
    let texture = Arc::new(resouce.create_texture("cube_container_tex", &image, texture_desc));
    // 线性重复采样器
    let sampler_config = SamplerDescriptor::new(
        FilterMode::Linear,
        FilterMode::Linear,
        MipmapFilterMode::Linear,
        AddressMode::Repeat,
        AddressMode::Repeat,
        AddressMode::Repeat,
        None,
    );
    let sampler = Arc::new(resouce.create_sampler("cube_linear_sampler", &sampler_config));
    // 纹理采样器绑定组
    let tex_bind_group = resouce.bind_group_builder()
        .texture(0, texture)
        .sampler(1, sampler)
        .build(Some("tex_sampler_bind_group"));
    // ===================== 4. MVP Uniform缓冲 + 矩阵BindGroup =====================
    let binding_layout = Arc::new(StructLayout::new(&[StructType::Mat4,StructType::Mat4,StructType::Mat4,]));
    let mut mvp_buffer: UniformBuffer = resouce.create_uniform_buffer(&binding_layout);
    //UniformBuffer::new(&context,&binding_layout);
    let matrix_bind_group = resouce.bind_group_builder()
        .uniform(0, &mvp_buffer)
        .build(Some("mvp_uniform_bind_group"));
    // ===================== 5. 3D渲染管线 =====================
    let pipeline = resouce.render_pipeline_builder_3d(&shader)
        .build(&[&tex_bind_group, &matrix_bind_group], &cube_mesh, Some("cube_3d_render_pipeline"));
    // ===================== 主渲染循环 =====================
    let mut running = true;
    while running {
        // 事件处理
        for event in sdl.event_pump().unwrap().poll_iter() {
            if let Event::Quit { .. } = event {
                running = false;
            }
        }
        // 相机&矩阵计算
        let time = timer::ticks() as f32 / 1000.0;
        let camera_pos = Vec3::new(0.0, 0.0, 3.0);
        let view = look_at_mat4(camera_pos, Vec3::ZERO, Vec3::Y);
        let fov = std::f32::consts::PI / 4.0;
        let aspect = win_size.0 as f32 / win_size.1 as f32;
        let proj = perspective(fov, aspect, 0.01, 500.0);

        // 模型旋转
        let mut model = Mat4::IDENTITY;
        model *= Mat4::from_rotation_y(time);
        model *= Mat4::from_rotation_x(time * 0.7);
        //设置槽位数值
        mvp_buffer.set(0, StructValue::Mat4(model));
        mvp_buffer.set(1, StructValue::Mat4(view));
        mvp_buffer.set(2, StructValue::Mat4(proj));
        //将数据上传到gpu
        resouce.update_uniform_buffer(&mut mvp_buffer);
        // 帧开始
        surface.begin_frame(Color::BLACK,1.0);
        
        let depth_attachment = surface.get_current_depth_attachment();
        let color_attachment = surface.get_current_color_attachment();
        // 绘制指令编码
        let mut encoder_draw = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut render_pass = encoder_draw.begin_render_pass(
                "cube_indexed_render_pass",
                &color_atts,
                depth_attachment,
                None,
                None,
                None,
            );
            render_pass.set_pipeline(&pipeline);
            render_pass.set_bind_groups(&[(0, &tex_bind_group), (1, &matrix_bind_group)]);
            // 框架内部自动识别索引缓冲，执行 indexed draw，无需修改draw调用
            render_pass.draw_mesh(&cube_mesh);
            render_pass.end();
        let cmd = encoder_draw.finish();
        surface.submit([cmd]);
        surface.present();
    }
}