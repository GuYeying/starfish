//! 渲染入口（RenderEntry）
//!
//! **多窗口支持（2026-09-08 起）**：base 的窗口层已支持运行时多窗口
//! （`ctx.create_window` + `window_created`/`window_closed`/按窗事件路由）。
//! 多窗口的渲染形态 = 每窗一个渲染目标：
//!
//! - 首窗：[`RenderEntry::new`] / [`RenderEntry::async_new`]（建立 wgpu 上下文）
//! - 其余窗：[`RenderEntry::surface_from_context`] /
//!   [`RenderEntry::async_surface_from_context`]（共享同一设备/队列，
//!   各窗独立表面与独立 present）
//!
//! 平台注记：桌面全后端验证路径；Web（多 canvas）与移动端表面语义
//! 未经验证，不构成支持承诺。

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use pollster::block_on;
use crate::base::window::Window;
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

    /// 阻塞式创建（桌面与 Emscripten 便利层；裸 wasm 无阻塞模型，不参与编译）
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        window: &Window,
        surface_settings: Option<SurfaceSettings>,
        gpu_settings:Option<GpuSettings>,
    ) -> Result<(RenderContext,RenderResourceAccess,RenderSurface), RenderContextError>{
        crate::base::rt::debug_assert_main_thread("RenderEntry::new");
        block_on(
            RenderEntry::async_new(
                window,
                surface_settings.unwrap_or_default(),
                gpu_settings.unwrap_or_default(),
            )
        )
    }

    // 异步创建渲染器 + 三角形网格
    pub async fn async_new(
        window: &Window,
        surface_settings: SurfaceSettings,
        gpu_settings:GpuSettings,
    ) -> Result<(RenderContext,RenderResourceAccess,RenderSurface), RenderContextError>{


        // 获取窗口大小
        let size: (u32, u32) = window.size();
        // ==============================================
        // 创建wgpu实例
        // ==============================================
        let instance = wgpu::Instance::new(gpu_settings.to_instance());

        // 只创建一次 surface！
        // 只提取纯数字raw句柄，不再绑定&window生命周期
        let raw_display = window.display_handle()?.as_raw();
        let raw_window = window.window_handle()?.as_raw();
        // 'static 生命周期，不绑定窗口引用（winit 窗口句柄经 HasWindowHandle 提取）
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
        // 许愿 → 掩码：愿望清单经 adapter 能力掩码后进入设备请求
        let granted_features = gpu_settings.resolve_features(&adapter);
        let granted_limits = gpu_settings.resolve_limits(&adapter);
        let adapter_info = adapter.get_info();

        let (device, queue) = adapter
            .request_device(&gpu_settings.to_device(&adapter))
            .await?;
        let (device, queue)  = (Arc::new(device),Arc::new(queue));

        // Web：未捕获错误不直接吞（WebGL2 上校验细节默认被顶层 Display 吞掉）——
        // 沿 source 链展开完整原因后再抛出，经 console_error_panic_hook 落控制台
        #[cfg(target_arch = "wasm32")]
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let mut msg = format!("wgpu uncaptured error: {e}");
            let mut src = std::error::Error::source(&e);
            while let Some(s) = src {
                msg.push_str(&format!("\n  caused by: {s}"));
                src = s.source();
            }
            panic!("{msg}");
        }));


        let render_context = RenderContext::new(
            instance,
            adapter,
            &surface,
            &device,
            &queue,
            granted_features,
            granted_limits,
            adapter_info,
        );

        super::features::report(gpu_settings.desired_features, granted_features);
        

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

    /// ⚠️【多窗口接口 · 仅开放，不具备开箱即用能力】
    ///
    /// 复用已有渲染上下文的 GPU 资源，为另一个窗口创建渲染表面（多窗口共享设备）
    ///
    /// **定位：桌面平台进阶用法，非跨平台承诺。** 本引擎官方支持形态是单窗口；
    /// 移动端/鸿蒙的表面语义不同（单 Activity/单 Surface），此入口在这些平台
    /// 上未经验证。单窗口开发请直接使用 [`RenderEntry::new`](Self::new)。
    ///
    /// 返回共享同一 instance/adapter/device/queue 的新 `(RenderContext, RenderSurface)`。
    /// `RenderResourceAccess` 与设备绑定，可直接复用已有的；若新表面颜色格式
    /// 与首窗口不同，再以新格式创建一份即可。
    ///
    /// ```ignore
    /// let (ctx, access, surface1) = RenderEntry::new(&window1, None, None)?;
    /// let (ctx2, surface2) = RenderEntry::surface_from_context(&ctx, &window2, SurfaceSettings::default())?;
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    pub fn surface_from_context(
        context: &RenderContext,
        window: &Window,
        surface_settings: SurfaceSettings,
    ) -> Result<(RenderContext, RenderSurface), RenderContextError> {
        crate::base::rt::debug_assert_main_thread("RenderEntry::surface_from_context");
        block_on(Self::async_surface_from_context(context, window, surface_settings))
    }

    /// ⚠️【多窗口接口 · 仅开放，不具备开箱即用能力】
    ///
    /// [`surface_from_context`](Self::surface_from_context) 的异步版本。
    /// 定位与限制同上：桌面平台进阶用法，非跨平台承诺。
    pub async fn async_surface_from_context(
        context: &RenderContext,
        window: &Window,
        surface_settings: SurfaceSettings,
    ) -> Result<(RenderContext, RenderSurface), RenderContextError> {
        let instance = context.instance().clone();
        let device = context.device().clone();
        let queue = context.queue().clone();
        let features = context.features();
        let limits = context.limits();
        let adapter_info = context.adapter_info().clone();

        // 与首窗口一致的 surface 创建路径：raw 句柄 + 'static surface
        let raw_display = window.display_handle()?.as_raw();
        let raw_window = window.window_handle()?.as_raw();
        let surface = Arc::new(unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display),
                raw_window_handle: raw_window,
            })
        }?);

        let adapter = context.adapter();
        let caps = surface.get_capabilities(adapter);
        let size: (u32, u32) = window.size();

        let render_frame = RenderSurface::new(&surface, &device, &queue, &size, surface_settings, &caps);

        let render_context = RenderContext::new(
            instance,
            adapter.clone(),
            &surface,
            &device,
            &queue,
            features,
            limits,
            adapter_info,
        );

        Ok((render_context, render_frame))
    }
}