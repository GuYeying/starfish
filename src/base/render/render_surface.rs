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
    color_view:  Option<Arc<TextureView>>,   //当前活动渲染目标视图（MSAA 开启时为多重采样视图）
    color_attachment : Option<ColorAttachment>,
    // 交换链视图（每帧换新；MSAA 关闭时与 color_view 相同）
    swapchain_view: Option<Arc<TextureView>>,

    // 深度缓冲
    depth_format:  TextureFormat,
    depth_texture: wgpu::Texture,
    depth_view:    Arc<TextureView>,
    depth_attachment : DepthAttachment,

    // MSAA
    sample_count: u32,
    msaa_texture: Option<wgpu::Texture>,
    msaa_view: Option<Arc<TextureView>>,

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
        // 先读出 MSAA 采样数（to_wgpu 按值消费 settings）
        let sample_count = surface_settings.sample_count.max(1);
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
        // 创建深度纹理（默认开启；采样数与颜色附件一致）
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
            sample_count,
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

        // ==============================================
        // 创建 MSAA 颜色纹理（sample_count > 1 时；present 时自动 resolve 到交换链）
        // ==============================================
        let (msaa_texture, msaa_view) = if sample_count > 1 {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("msaa_color"),
                size: wgpu::Extent3d {
                    width: config.width,
                    height: config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: color_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = Arc::new(texture.create_view(&Default::default()));
            (Some(texture), Some(view))
        } else {
            (None, None)
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
            swapchain_view: None,

            depth_format:depth_format,
            depth_texture:depth_texture,
            depth_view:depth_view,
            depth_attachment:depth_attachment,

            sample_count,
            msaa_texture,
            msaa_view,

            occlusion_query_set:occlusion_query_set,

            pending_cmds: Vec::new(),


        }
    }



    pub fn begin_frame(&mut self, clear_color: wgpu::Color,clear_depth:f32) {
        // 获取当前交换链纹理（自愈式）：
        // Outdated/Lost → 重建交换链配置与深度/MSAA 纹理后重试（拖动窗口/最小化的常见情况）
        // Timeout/Occluded → 直接重试
        // Validation → 真实错误，panic
        if self.color_view.is_none() {
            let mut frame = None;
            for _ in 0..3 {
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                        frame = Some(f);
                        break;
                    }
                    wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                        self.resize(self.config.width, self.config.height);
                    }
                    wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {}
                    wgpu::CurrentSurfaceTexture::Validation => panic!("渲染验证错误"),
                }
            }
            let Some(frame) = frame else {
                panic!("多次尝试后仍无法获取窗口帧（窗口可能已销毁）");
            };

            let swapchain_view = Arc::new(frame.texture.create_view(&Default::default()));
            // 活动渲染目标：MSAA 开启时是多重采样纹理，否则直接交换链视图
            let target = self.msaa_view.clone().unwrap_or(swapchain_view.clone());
            self.swapchain_view = Some(swapchain_view);
            self.color_attachment = Some(ColorAttachment{
                view: target.clone(),
                resolve_target: None,
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
                depth_slice: None
            });
            self.color_frame = Some(frame);
            self.color_view = Some(target);
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

        // 2. MSAA：空 resolve 通道把多重采样结果解析到交换链
        if let (Some(msaa), Some(swapchain)) = (self.msaa_view.clone(), self.swapchain_view.take()) {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("msaa_resolve"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &msaa,
                    resolve_target: Some(&swapchain),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });
            drop(_pass);
            self.pending_cmds.push(encoder.finish());
        }
        self.swapchain_view = None;

        // 3. 仅当存在命令时才提交（空帧避免无意义submit）
        if !all_commands.is_empty() {
            let _submission_idx = self.queue.submit(all_commands);
        }
        // 4. 上屏
        self.queue.present(frame);
        // 重置帧状态
        self.color_view = None;
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
            sample_count: self.sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_view = Arc::new(self.depth_texture.create_view(&Default::default()));
        self.depth_attachment = DepthAttachment{
            view: self.depth_view.clone(),
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
            stencil_ops: None,
            depth_slice: None,
        };

        // 5. 重建 MSAA 颜色纹理
        if self.sample_count > 1 {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("msaa_color"),
                size: wgpu::Extent3d {
                    width: self.config.width,
                    height: self.config.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: self.sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: self.color_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            self.msaa_view = Some(Arc::new(texture.create_view(&Default::default())));
            self.msaa_texture = Some(texture);
        }
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

    /// 表面多重采样数（1 = MSAA 关闭）
    pub fn sample_count(&self) -> u32 {
        self.sample_count
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
