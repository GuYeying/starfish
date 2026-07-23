use std::sync::Arc;
use pollster::block_on;
use super::super::window::Window;
use sdl3::video::Window as SdlWindow;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};
use crate::base::render::RenderContext;
use crate::base::render::render_resource_access::RenderResourceAccess;
use crate::base::render::settings::{GpuSettings, SurfaceSettings};
use crate::base::render::render_surface::RenderSurface;



#[derive(thiserror::Error, Debug)]
pub enum RenderContextError {
    #[error("获取窗口DisplayHandle失败: {0}")]
    DisplayHandle(HandleError),
    #[error("获取窗口WindowHandle失败: {0}")]
    WindowHandle(HandleError),
    #[error("创建WGPU Surface失败: {0}")]
    CreateSurface(wgpu::CreateSurfaceError),
    #[error("请求GPU适配器失败，无兼容显卡")]
    RequestAdapter,
    #[error("创建设备/队列失败: {0}")]
    RequestDevice(wgpu::RequestDeviceError),
}

// 自动转换对应错误
impl From<HandleError> for RenderContextError {
    fn from(e: HandleError) -> Self {
        RenderContextError::DisplayHandle(e)
    }
}
impl From<wgpu::CreateSurfaceError> for RenderContextError {
    fn from(e: wgpu::CreateSurfaceError) -> Self {
        RenderContextError::CreateSurface(e)
    }
}
impl From<wgpu::RequestDeviceError> for RenderContextError {
    fn from(e: wgpu::RequestDeviceError) -> Self {
        RenderContextError::RequestDevice(e)
    }
}


pub struct RenderEntry;

impl RenderEntry{

    pub fn new(
        window: &Window, 
        surface_settings: Option<SurfaceSettings>,
        gpu_settings:Option<GpuSettings>,
    ) -> Result<(RenderContext,RenderResourceAccess,RenderSurface), RenderContextError>{
        block_on(
            RenderEntry::async_new(
                window.inner(), 
                surface_settings.unwrap_or_default(),
                gpu_settings.unwrap_or_default(),
            )
        )
    }

    // 异步创建渲染器 + 三角形网格
    pub async fn async_new(
        window: &SdlWindow, 
        surface_settings: SurfaceSettings,
        gpu_settings:GpuSettings,
    ) -> Result<(RenderContext,RenderResourceAccess,RenderSurface), RenderContextError>{


        // 获取窗口大小
        let size: (u32, u32) = window.size();
        // ==============================================
        // 创建wgpu实例（指定Vulkan后端，关闭调试）
        // ==============================================
        let instance = wgpu::Instance::new(gpu_settings.to_instance());

        // 只创建一次 surface！
        // 只提取纯数字raw句柄，不再绑定&window生命周期
        let raw_display = window.display_handle()?.as_raw();
        let raw_window = window.window_handle()?.as_raw();
        // 'static 生命周期，不再依赖 SdlWindow
        let surface = Arc::new(unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display),
                raw_window_handle: raw_window
            })
        }?);

        // ==============================================
        // 请求GPU适配器
        // ==============================================
        let maybe_adapter: Result<wgpu::Adapter, wgpu::RequestAdapterError> = instance
            .request_adapter(&gpu_settings.to_adapter(&surface))
            .await;

        let adapter = match maybe_adapter {
            Ok(a) => a,
            Err(_) => return Err(RenderContextError::RequestAdapter),
        };
        println!("{:#?}", adapter.get_info());

        // ==============================================
        // 获取表面能力，选择纹理格式
        // ==============================================
        let caps = surface.get_capabilities(&adapter);
        println!("Supported formats: {:?}", caps.formats);

        // ==============================================
        // 请求逻辑设备 + 命令队列
        // ==============================================
        let (device, queue) = adapter
            .request_device(&gpu_settings.to_device())
            .await?;
        let (device, queue)  = (Arc::new(device),Arc::new(queue));


        let render_context = RenderContext::new(instance, &surface, &device, &queue);
        

        let render_frame: RenderSurface = 
                RenderSurface::new(
                &surface,
                &device,
                &queue,
                &size,
                surface_settings,
                &caps,
        );
        
        let render_resource_access: RenderResourceAccess = 
                RenderResourceAccess::new(
                    device,
                    queue,
                    render_frame.color_format(),
                    render_frame.depth_format()
                );

        Ok((render_context,render_resource_access,render_frame))
    }
}