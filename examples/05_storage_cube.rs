use std::{fs, sync::Arc};
use bytemuck::{cast_slice, Pod, Zeroable};
use sdl3::{
    event::Event, keyboard::Scancode, timer, video::WindowFlags, keyboard::Keycode
};
use image::ImageReader;
use glam::{Mat4, Vec3, Vec2};
use wgpu::{
    AddressMode, Color, FilterMode, InstanceFlags, MemoryHints,
    MipmapFilterMode, PowerPreference, TextureUsages
};
use starfish::base::{
    render::{bind_group::{
            field_type::StructType, field_value::StructValue,struct_layout::StructLayout,
        }, render_entry::RenderEntry,sampler_desc::SamplerDescriptor, settings::{GpuSettings, SurfaceSettings}, texture::{
            TextureDescriptor, TextureDim, TextureSemantic, TextureUsage,
        },
    }, resources::{image::ImageData, shader::Shader}, subsystem::{EventSubsystem, VideoSubsystem}, window::Window,
};

// ===================== StorageBuffer 物体数据（仅Storage用，无需StructLayout） =====================
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct ObjectData {
    pub model: [[f32; 4]; 4],
}

use glam::camera::rh::view::look_at_mat4;
use glam::camera::rh::proj::directx::perspective;

// ===================== 立方体24唯一顶点 + 索引（完全沿用） =====================
const CUBE_UNIQUE_VERTS: &[f32] = &[
    // Front (+Z)
    -0.5,-0.5, 0.5, 0.0,0.0,
     0.5,-0.5, 0.5, 1.0,0.0,
     0.5, 0.5, 0.5, 1.0,1.0,
    -0.5, 0.5, 0.5, 0.0,1.0,
    // Back (-Z)
     0.5,-0.5,-0.5, 0.0,0.0,
    -0.5,-0.5,-0.5, 1.0,0.0,
    -0.5, 0.5,-0.5, 1.0,1.0,
     0.5, 0.5,-0.5, 0.0,1.0,
    // Left (-X)
    -0.5,-0.5,-0.5, 0.0,0.0,
    -0.5,-0.5, 0.5, 1.0,0.0,
    -0.5, 0.5, 0.5, 1.0,1.0,
    -0.5, 0.5,-0.5, 0.0,1.0,
    // Right (+X)
     0.5,-0.5, 0.5, 0.0,0.0,
     0.5,-0.5,-0.5, 1.0,0.0,
     0.5, 0.5,-0.5, 1.0,1.0,
     0.5, 0.5, 0.5, 0.0,1.0,
    // Top (+Y)
    -0.5, 0.5, 0.5, 0.0,0.0,
     0.5, 0.5, 0.5, 1.0,0.0,
     0.5, 0.5,-0.5, 1.0,1.0,
    -0.5, 0.5,-0.5, 0.0,1.0,
    // Bottom (-Y)
    -0.5,-0.5,-0.5, 0.0,0.0,
     0.5,-0.5,-0.5, 1.0,0.0,
     0.5,-0.5, 0.5, 1.0,1.0,
    -0.5,-0.5, 0.5, 0.0,1.0,
];

const CUBE_INDICES: &[u16] = &[
    // Front
    0, 1, 2, 0, 2, 3,
    // Back
    4, 5, 6, 4, 6, 7,
    // Left
    8, 9,10, 8,10,11,
    // Right
    12,13,14, 12,14,15,
    // Top
    16,17,18, 16,18,19,
    // Bottom
    20,21,22, 20,22,23,
];

// ===================== 多立方体世界坐标 =====================
const CUBE_POSITIONS: [Vec3; 10] = [
    Vec3::new(0.0, 0.0, 0.0),
    Vec3::new(2.0, 5.0, -15.0),
    Vec3::new(-1.5, -2.2, -2.5),
    Vec3::new(-3.8, -2.0, -12.3),
    Vec3::new(2.4, -0.4, -3.5),
    Vec3::new(-1.7, 3.0, -7.5),
    Vec3::new(1.3, -2.0, -2.5),
    Vec3::new(1.5, 2.0, -2.5),
    Vec3::new(1.5, 0.2, -1.5),
    Vec3::new(-1.3, 1.0, -1.5),
];
const CUBE_COUNT: usize = CUBE_POSITIONS.len();

// ===================== 相机状态 =====================
struct CameraState {
    pos: Vec3,
    front: Vec3,
    up: Vec3,
    yaw: f32,
    pitch: f32,
    fov: f32,
    last_mouse: Vec2,
    first_mouse: bool,
}

impl CameraState {
    fn new() -> Self {
        Self {
            pos: Vec3::new(0.0, 0.0, 3.0),
            front: Vec3::new(0.0, 0.0, -1.0),
            up: Vec3::Y,
            yaw: -90.0,
            pitch: 0.0,
            fov: 45.0,
            last_mouse: Vec2::new(400.0, 300.0),
            first_mouse: true,
        }
    }

    fn update_front(&mut self) {
        let rad_yaw = self.yaw.to_radians();
        let rad_pitch = self.pitch.to_radians();
        let x = rad_yaw.cos() * rad_pitch.cos();
        let y = rad_pitch.sin();
        let z = rad_yaw.sin() * rad_pitch.cos();
        self.front = Vec3::new(x, y, z).normalize();
    }
}

fn main() {
    // SDL 初始化
    let sdl = sdl3::init().expect("SDL init failed.");
    let video_subsys = VideoSubsystem::new(&sdl);
    let _event_subsys = EventSubsystem::new(&sdl);

    // 窗口
    let win_size = (800, 600);
    let mut window = Window::new(&video_subsys, "StorageBuffer Instanced Cube Demo", win_size, WindowFlags::default())
        .expect("Window creation failed");
    window.set_mouse_relative(true);
    window.set_mouse_grabbed(true);

    // 渲染上下文
    let surface_settings = SurfaceSettings::default()
        .with_usage(TextureUsages::RENDER_ATTACHMENT)
        .with_frame_latency(2);
    let gpu_settings = GpuSettings::default()
        .with_power_preference(PowerPreference::LowPower)
        .with_flags(InstanceFlags::empty())
        .with_memory_hints(MemoryHints::MemoryUsage)
        .with_depth(true);

    let (context,resouce,mut surface) = 
        RenderEntry::new(&window, Some(surface_settings), Some(gpu_settings))
            .expect("RenderContext 初始化失败");

    // ===================== 1. 着色器模块 =====================
    let shader_source = fs::read_to_string("resources/shaders/storage_cube.wgsl").unwrap();
    let shader = resouce.shader_module_builder(Shader::new(shader_source))
        .build(Some("storage_cube_shader"));

    // ===================== 2. 索引立方体网格 =====================
    let vertex_layout = vec![
        wgpu::VertexFormat::Float32x3,
        wgpu::VertexFormat::Float32x2,
    ];
    let cube_mesh = resouce.mesh_builder(vertex_layout, cast_slice(CUBE_UNIQUE_VERTS).to_vec())
        .with_short_indices(cast_slice(CUBE_INDICES).to_vec())
        .build(Some("cube_indexed_mesh"), None);

    // ===================== 3. Group2：纹理+采样器 BindGroup =====================
    let texture_desc = TextureDescriptor::new(
        TextureSemantic::Color,
        TextureUsage::Sampled,
        TextureDim::D2,
        None,
        None,
    );
    let img = ImageReader::open("resources/textures/container.jpg")
        .unwrap()
        .decode()
        .unwrap()
        .into_rgba8();
    let image = ImageData::Rgba8(img);
    let texture = Arc::new(resouce.create_texture("cube_container_tex", &image, texture_desc));

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

    let tex_bind_group = resouce.bind_group_builder()
        .texture(0, texture)
        .sampler(1, sampler)
        .build(Some("tex_sampler_bind_group"));

    // ===================== 4. Group0：相机 UniformBuffer（沿用你的StructValue接口） =====================
    let camera_layout = Arc::new(StructLayout::new(&[StructType::Mat4,StructType::Mat4,]));
    let mut camera_buffer = resouce.create_uniform_buffer(&camera_layout);
    // 初始化默认矩阵
    camera_buffer.set(0, StructValue::Mat4(Mat4::IDENTITY));
    camera_buffer.set(1, StructValue::Mat4(Mat4::IDENTITY));
    let camera_bind_group = resouce.bind_group_builder()
        .uniform(0, &camera_buffer)
        .build(Some("camera_bind_group"));

    // ===================== 5. Group1：物体 StorageBuffer =====================
    // 单个元素仅一个Mat4，对应WGSL ObjectData { model: mat4x4<f32> }
    let object_storage_layout = Arc::new(StructLayout::new(&[StructType::Mat4]));
    // 容量CUBE_COUNT个实例
    let mut object_storage_buffer = resouce.create_storage_buffer(&object_storage_layout, CUBE_COUNT, None);
    // 构建Storage绑定组，只读存储
    let object_bind_group = resouce.bind_group_builder()
        .storage(0, &object_storage_buffer, true)
        .build(Some("object_storage_bind_group"));

    // ===================== 6. 渲染管线（Group0相机 / Group1物体存储 / Group2纹理） =====================
    let pipeline = resouce.render_pipeline_builder_3d(&shader)
        .build(
            &[&camera_bind_group, &object_bind_group, &tex_bind_group],
            &cube_mesh,
            Some("storage_instanced_cube_pipeline")
        );

    // 运行时变量
    let mut camera = CameraState::new();
    let mut last_time = timer::ticks() as f64 / 1000.0;
    let mut rotate_time: f32 = 0.0;
    let mut running = true;
    let mut mouse_pump = sdl.event_pump().unwrap();

    // 主渲染循环
    while running {
        // 帧时间计算
        let now = timer::ticks() as f64 / 1000.0;
        let delta_time = (now - last_time) as f32;
        last_time = now;
        rotate_time += delta_time * 25.0;

        // 事件处理
        for event in mouse_pump.poll_iter() {
            match event {
                Event::Quit { .. } => {
                    running = false;
                    window.set_mouse_relative(false);
                    window.set_mouse_grabbed(false);
                }
                Event::KeyDown { keycode, .. } => {
                    if keycode == Some(Keycode::Escape) {
                        running = false;
                        window.set_mouse_relative(false);
                        window.set_mouse_grabbed(false);
                    }
                }
                Event::MouseMotion { x, y, .. } => {
                    let mouse_pos = Vec2::new(x as f32, y as f32);
                    if camera.first_mouse {
                        camera.last_mouse = mouse_pos;
                        camera.first_mouse = false;
                    }
                    let offset_x = mouse_pos.x - camera.last_mouse.x;
                    let offset_y = camera.last_mouse.y - mouse_pos.y;
                    camera.last_mouse = mouse_pos;

                    let sens = 0.1;
                    camera.yaw += offset_x * sens;
                    camera.pitch += offset_y * sens;
                    camera.pitch = camera.pitch.clamp(-89.0, 89.0);
                    camera.update_front();
                }
                Event::MouseWheel { y, .. } => {
                    camera.fov -= y as f32;
                    camera.fov = camera.fov.clamp(1.0, 45.0);
                }
                _ => {}
            }
        }

        // WASD 相机移动
        let speed = 2.5 * delta_time;
        let keys = mouse_pump.keyboard_state();
        let right = camera.front.cross(camera.up).normalize();
        if keys.is_scancode_pressed(Scancode::W) { camera.pos += speed * camera.front; }
        if keys.is_scancode_pressed(Scancode::S) { camera.pos -= speed * camera.front; }
        if keys.is_scancode_pressed(Scancode::A) { camera.pos -= speed * right; }
        if keys.is_scancode_pressed(Scancode::D) { camera.pos += speed * right; }

        // 更新相机矩阵，写入UniformBuffer
        let (w, h) = window.size();
        let aspect = if h <= 0 { 1.0 } else { w as f32 / h as f32 };
        let view = look_at_mat4(camera.pos, camera.pos + camera.front, camera.up);
        let proj = perspective(camera.fov.to_radians(), aspect, 0.1, 100.0);
        camera_buffer.set(0, StructValue::Mat4(view));
        camera_buffer.set(1, StructValue::Mat4(proj));
        resouce.update_uniform_buffer(&mut camera_buffer);

        // 批量生成所有物体矩阵，直接写入StorageBuffer内部缓存，无中间临时数组
        let rot_axis = Vec3::new(1.0, 0.3, 0.5).normalize();
        for (idx, cube_pos) in CUBE_POSITIONS.iter().enumerate() {
            let rot_rad = (rotate_time + idx as f32 * 20.0).to_radians();
            let model_mat = Mat4::from_translation(*cube_pos) * Mat4::from_axis_angle(rot_axis, rot_rad);

            // 直接设置第idx个实例的字段，底层自动计算偏移、写入内部data、标记dirty
            object_storage_buffer.set_element(idx, &[StructValue::Mat4(model_mat)]);
        }

        // 统一上传：内部自动判断dirty、重排字节、调用queue.write_buffer
        resouce.update_storage_buffer(&mut object_storage_buffer);

        // 渲染通道
        surface.begin_frame(Color { r: 0.2, g: 0.3, b: 0.3, a: 1.0 },1.0);
        let depth_attachment = surface.get_current_depth_attachment();
        let color_attachment = surface.get_current_color_attachment();

        let mut encoder_draw = resouce.create_command_encoder();
            let color_atts = [&color_attachment];
            let mut render_pass = encoder_draw.begin_render_pass(
                "storage_instanced_cube_pass",
                &color_atts,
                depth_attachment,
                None,
                None,
                None,
            );
            render_pass.set_pipeline(&pipeline);
            // 一次性绑定三组资源，无循环、无动态偏移
            render_pass.set_bind_groups(&[
                (0, &camera_bind_group),
                (1, &object_bind_group),
                (2, &tex_bind_group),
            ]);
            // 单次实例化绘制全部立方体，GPU内部通过instance_index读取Storage数组
            render_pass.draw_mesh_instanced(&cube_mesh,  0..CUBE_COUNT as u32);
            render_pass.end();
        let cmd = encoder_draw.finish();
        surface.submit([cmd]);
        surface.present();
    }
}