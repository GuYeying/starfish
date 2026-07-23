use std::sync::Arc;
use wgpu::{Device, Queue, Surface, SurfaceCapabilities, SurfaceTexture, TextureFormat};
use crate::base::render::render_pass::attachments::{ColorAttachment, DepthAttachment};
use crate::base::render::settings::SurfaceSettings;
use wgpu::{CommandBuffer};
use wgpu::{TextureView};


//管理每帧的数据的
pub struct RenderSurface{
    surface: Arc<wgpu::Surface<'static>>,          // 渲染表面（对应窗口）
    device: Arc<wgpu::Device>,                // GPU逻辑设备（核心）
    queue: Arc<wgpu::Queue>,                  // GPU命令队列

    config: wgpu::SurfaceConfiguration,

    // 帧
    color_format:TextureFormat,
    color_frame: Option<SurfaceTexture>,//当前帧
    color_view:  Option<Arc<wgpu::TextureView>>,   //当前帧视图
    color_attachment : Option<ColorAttachment>,

    // 深度缓冲
    depth_format:  TextureFormat,
    depth_texture: wgpu::Texture,
    depth_view:    Arc<wgpu::TextureView>,
    depth_attachment : DepthAttachment,
    // 遮挡查询
    occlusion_query_set: Arc<wgpu::QuerySet>,

    pending_cmds: Vec<CommandBuffer>,


}

impl RenderSurface{

    pub(crate) fn new(
        surface: &Arc<Surface<'static>>,
        device:&Arc<Device>,
        queue:&Arc<Queue>,
        size:&(u32, u32),
        surface_settings: SurfaceSettings,
        caps: &SurfaceCapabilities,

    )->Self{

        let depth_format = surface_settings.depth_format.unwrap_or_else( || wgpu::TextureFormat::Depth24Plus);
        let config = surface_settings.to_wgpu(caps, size);
        

        // ==============================================
        // 创建遮挡查询（默认开启）
        // ==============================================
        let occlusion_query_set = Arc::new(device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("occlusion_query"),
            ty: wgpu::QueryType::Occlusion,
            count: 1,
        }));


        // ==============================================
        // 创建深度纹理（默认开启）
        // ==============================================
        let color_format = config.format;
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth_texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = Arc::new(depth_texture.create_view(&Default::default()));
        let depth_attachment = DepthAttachment{ 
            view: depth_view.clone(), 
            load: wgpu::LoadOp::Load, 
            store: wgpu::StoreOp::Store,
            stencil_ops: None, 
            depth_slice: None,

        };



        surface.configure(&device, &config);



        Self{
            surface:surface.clone(),
            device: device.clone(),
            queue: queue.clone(),
            
            config: config,

            color_format: color_format,
            color_frame: None,
            color_view: None,
            color_attachment: None,

            depth_format:depth_format,
            depth_texture:depth_texture,
            depth_view:depth_view,
            depth_attachment:depth_attachment,

            occlusion_query_set:occlusion_query_set,

            pending_cmds: Vec::new(),

           
        }
    }



    pub fn begin_frame(&mut self, clear_color: wgpu::Color,clear_depth:f32) {
        // 获取当前交换链纹理
        if self.color_view.is_none() {
            let frame = match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(f) => f,
                wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
                wgpu::CurrentSurfaceTexture::Timeout => panic!("获取窗口帧超时"),
                wgpu::CurrentSurfaceTexture::Outdated => panic!("帧已过期，需要重建交换链"),
                wgpu::CurrentSurfaceTexture::Lost => panic!("窗口已丢失，无法渲染"),
                wgpu::CurrentSurfaceTexture::Occluded => panic!("窗口被遮挡，无法获取帧"),
                wgpu::CurrentSurfaceTexture::Validation => panic!("渲染验证错误"),
            };

            let view = Arc::new(frame.texture.create_view(&Default::default()));
            self.color_attachment = Some(ColorAttachment{ 
                view: view.clone(),
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
                resolve_target: None,
                depth_slice: None 
            });
            self.color_frame = Some(frame);
            self.color_view = Some(view);
        }


        let cur_view = self.color_view.as_ref().expect("bbegin_frame: color_view is empty, swap chain texture not acquired");
        let mut encoder = self.device.create_command_encoder(&Default::default());

        // 清屏 + 清深度
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear_screen"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: cur_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_depth),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: Some(&self.occlusion_query_set),
            timestamp_writes: None,
            multiview_mask: None,
        });

        drop(_pass);
        let cmd = encoder.finish();
        self.pending_cmds.push(cmd);
    }

    /// 仅缓存命令，延后到present统一提交
    pub fn submit<I: IntoIterator<Item = CommandBuffer>>(&mut self, command_buffers: I) {
        self.pending_cmds.extend(command_buffers.into_iter());
    }
    pub fn submit_single(
        &mut self,
        cmd: CommandBuffer
    ){
        self.pending_cmds.push(cmd);
    }

    pub fn present(&mut self) {
        // 取出当前帧交换链纹理
        let frame = self.color_frame.take().expect("present() failed: No valid frames");
        // 1. 收集所有待提交命令
        let all_commands = std::mem::take(&mut self.pending_cmds);
        // 3. 仅当存在命令时才提交（空帧避免无意义submit）
        if !all_commands.is_empty() {
            let _submission_idx = self.queue.submit(all_commands);
        }
        // 4. 上屏
        self.queue.present(frame);
        // 重置帧状态
        self.color_view = None;
        self.color_frame = None;
    }


    pub fn resize(&mut self, width: u32, height: u32) {
        // 0. 防 0 尺寸
        let width = width.max(1);
        let height = height.max(1);

        // 2. 直接修改你原来存的 config
        self.config.width = width;
        self.config.height = height;

        // 3. 用修改后的 config 重新配置 surface
        self.surface.configure(&self.device, &self.config);

        // 4. 重新创建匹配新尺寸的深度纹理
        self.depth_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth_texture"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_view = Arc::new(self.depth_texture.create_view(&Default::default()));
    }


    

    pub fn get_current_color_texture_view(&self) -> Option<Arc<TextureView>>{
        self.color_view.clone()
    }
    pub fn get_current_depth_texture_view(&self)-> Option<Arc<TextureView>>{
        Some(self.depth_view.clone())
    }
    pub fn get_current_color_attachment(&self)->Option<ColorAttachment>{
        self.color_attachment.clone()
    }
    pub fn get_current_depth_attachment(&self)->Option<DepthAttachment>{
        Some(self.depth_attachment.clone())
    }


    pub fn color_format(&self)->TextureFormat{
        self.color_format.clone()
    }
    pub fn depth_format(&self)->TextureFormat{
        self.depth_format.clone()
    }

    /// 判断传入纹理视图是否等于当前帧颜色视图
    pub fn is_same_color_view(&self, other: &Arc<TextureView>) -> bool {
        let Some(cur) = &self.color_view else {
            return false;
        };
        Arc::as_ptr(cur) == Arc::as_ptr(other)
    }

    /// 校验传入的 ColorAttachment 是否是当前帧有效附件
    pub fn is_valid_color_attachment(&self, attach: &ColorAttachment) -> bool {
        self.is_same_color_view(&attach.view)
    }

    /// 判断传入纹理视图是否等于当前深度视图
    pub fn is_same_depth_view(&self, other: &Arc<TextureView>) -> bool {
        Arc::as_ptr(&self.depth_view) == Arc::as_ptr(other)
    }

    /// 校验传入的 DepthAttachment 是否是当前有效深度附件
    pub fn is_valid_depth_attachment(&self, attach: &DepthAttachment) -> bool {
        self.is_same_depth_view(&attach.view)
    }

    
}

