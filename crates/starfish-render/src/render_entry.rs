//! 渲染入口（RenderEntry）
//!
//! **上下文/表面分离模型**（2026-10-23 多窗口定稿）：[`RenderContext`] 持
//! instance/adapter/device/queue（设备族，进程一份）；[`RenderSurface`]
//! 对应一个窗口的交换链（每窗一份，共享设备族）——多窗口 = 一个上下文
//! + N 个表面，surface 之外一切不重建。
//!
//! 三条入口路径：
//! - 单窗口一步到位：[`RenderEntry::new`] / [`RenderEntry::async_new`]；
//! - 设备先行 + 挂表面（多窗口核心）：[`RenderEntry::new_context`] →
//!   [`RenderEntry::new_surface`]；
//! - 第三方框架/外部设备：raw handles 族与 `surface_from_foreign_device`。

use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use pollster::block_on;
#[cfg(feature = "we")]
use starfish_we::Window;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use crate::RenderContext;
use crate::render_resource_access::RenderResourceAccess;
use crate::settings::{GpuSettings, SurfaceSettings};
use crate::render_surface::RenderSurface;



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
    #[cfg(feature = "we")]
    pub fn new(
        window: &Window,
        surface_settings: Option<SurfaceSettings>,
        gpu_settings:Option<GpuSettings>,
    ) -> Result<(RenderContext,RenderResourceAccess,RenderSurface), RenderContextError>{
        starfish_std::debug::assert_main_thread("RenderEntry::new");
        block_on(
            RenderEntry::async_new(
                window,
                surface_settings.unwrap_or_default(),
                gpu_settings.unwrap_or_default(),
            )
        )
    }

    /// 无窗口独立上下文（**设备先行形态**，阻塞版）：instance/adapter/device/queue
    /// 先于窗口创建——纹理/采样器/bind group 等设备资源即刻可建（经
    /// [`RenderContext::resource_access`]），窗口就位后经
    /// [`Self::surface_from_context`](Self::surface_from_context) 挂表面。
    ///
    /// 动态宿主（如 Python 绑定）的资源构造早于事件循环启动——本入口把
    /// "Device 存在"与"窗口存在"两个时刻解绑，消除两侧生命周期的强兼容。
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_context(gpu_settings: GpuSettings) -> Result<RenderContext, RenderContextError> {
        starfish_std::debug::assert_main_thread("RenderEntry::new_context");
        block_on(Self::async_new_context(gpu_settings))
    }

    /// [`new_context`](Self::new_context) 的异步版（Web 装载任务/异步宿主用）
    pub async fn async_new_context(
        gpu_settings: GpuSettings,
    ) -> Result<RenderContext, RenderContextError> {
        let instance = wgpu::Instance::new(gpu_settings.to_instance());
        let maybe_adapter = instance.request_adapter(&gpu_settings.to_adapter_headless()).await;
        let adapter = match maybe_adapter {
            Ok(a) => a,
            Err(_) => return Err(RenderContextError::RequestAdapter),
        };

        let granted_features = gpu_settings.resolve_features(&adapter);
        let granted_limits = gpu_settings.resolve_limits(&adapter);
        let adapter_info = adapter.get_info();

        let (device, queue) = adapter
            .request_device(&gpu_settings.to_device(&adapter))
            .await?;
        let (device, queue) = (Arc::new(device), Arc::new(queue));

        // 与 async_new 同款：Web 未捕获错误不吞（WebGL2 校验细节默认被顶层吞掉）
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

        Ok(RenderContext::new(
            instance,
            adapter,
            &device,
            &queue,
            granted_features,
            granted_limits,
            adapter_info,
        ))
    }

    /// Web：canvas 直连（SurfaceTarget::Canvas，无需任何窗口层句柄）。
    /// 宿主把 starfish-window 的 `Window::canvas()` 传进来即可。
    #[cfg(target_arch = "wasm32")]
    pub async fn async_new_from_canvas(
        canvas: web_sys::HtmlCanvasElement,
        surface_settings: SurfaceSettings,
        gpu_settings: GpuSettings,
    ) -> Result<(RenderContext, RenderResourceAccess, RenderSurface), RenderContextError> {
        let instance = wgpu::Instance::new(gpu_settings.to_instance());
        // 安全 API：SurfaceTarget::Canvas（wasm32 + web feature 即存在）
        let surface = Arc::new(instance.create_surface(
            wgpu::SurfaceTarget::Canvas(canvas.clone()),
        )?);

        let maybe_adapter = instance
            .request_adapter(&gpu_settings.to_adapter(&surface))
            .await;
        let adapter = match maybe_adapter {
            Ok(a) => a,
            Err(_) => return Err(RenderContextError::RequestAdapter),
        };

        let caps = surface.get_capabilities(&adapter);
        let downlevel_caps = adapter.get_downlevel_capabilities();

        let granted_features = gpu_settings.resolve_features(&adapter);
        let granted_limits = gpu_settings.resolve_limits(&adapter);
        let adapter_info = adapter.get_info();

        let (device, queue) = adapter
            .request_device(&gpu_settings.to_device(&adapter))
            .await?;
        let (device, queue) = (Arc::new(device), Arc::new(queue));

        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            let mut msg = format!("wgpu uncaptured error: {e}");
            let mut src = std::error::Error::source(&e);
            while let Some(s) = src {
                msg.push_str(&format!("
  caused by: {s}"));
                src = s.source();
            }
            panic!("{msg}");
        }));

        let size = (canvas.width(), canvas.height());
        let render_context = RenderContext::new(
            instance,
            adapter,
            &device,
            &queue,
            granted_features,
            granted_limits,
            adapter_info,
        );

        super::features::report(gpu_settings.desired_features, granted_features);

        let render_frame = RenderSurface::new(
            &surface,
            &device,
            &queue,
            &size,
            surface_settings,
            &caps,
            &downlevel_caps,
        );

        let render_resource_access = RenderResourceAccess::new(
            device,
            queue,
            render_frame.color_format(),
            render_frame.depth_format(),
        );

        Ok((render_context, render_resource_access, render_frame))
    }

    // 异步创建渲染器 + 三角形网格
    #[cfg(feature = "we")]
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
        // 提取纯数字 raw 句柄（'static，不绑定 &window 生命周期）——
        // wgpu 的 Surface<'window> 绑定目标借用，而 surface 须存活至
        // App 结束：此处的 unsafe 是"窗口由 App 持有"契约的落点（行业
        // 标准形态，winit+wgpu 应用同款），非可随手消除项。
        let raw_display = window.display_handle()?.as_raw();
        let raw_window = window.window_handle()?.as_raw();
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

        // ==============================================
        // 获取表面能力，选择纹理格式
        // ==============================================
        let caps = surface.get_capabilities(&adapter);
        // downlevel 能力（view_formats 掩码用，见 SurfaceSettings::to_wgpu）
        let downlevel_caps = adapter.get_downlevel_capabilities();

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

        // 原生（Android 等）：接 console_log——壳的诊断 sink 落 dev_log.txt
        // （此前原生无处理器，全部校验错/创建错被静默吞掉，真机黑屏/全屏
        // 陈旧帧无从定位）
        #[cfg(not(target_arch = "wasm32"))]
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            starfish_std::debug::console_log(&format!("[wgpu-error] {e}"));
        }));


        let render_context = RenderContext::new(
            instance,
            adapter,
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
                &downlevel_caps,
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

    // =========================================================
    // 多窗口核心：设备先行 → 每窗挂表面（上下文/表面分离模型）
    // =========================================================

    /// 从已有上下文为窗口挂渲染表面（**多窗口核心入口**，阻塞桌面版）。
    ///
    /// 复用 `context` 的 instance/adapter/device/queue，仅新建该窗口的
    /// 交换链——多窗口 = 一个 [`RenderContext`] + N 个 [`RenderSurface`]，
    /// surface 之外一切不重建。调用方须保证 `window` 比回出的表面晚销毁
    /// （表面持 'static 句柄，契约同 [`Self::async_new`]）。
    ///
    /// 多窗口共享约定：各窗口 [`SurfaceSettings`] 一致（默认即一致）时，
    /// 一份 `RenderResourceAccess` + 同参数管线全窗口共享；颜色格式或
    /// MSAA 不同的窗口以 `ctx.resource_access(颜色格式, 深度格式)` 另建
    /// access，管线按 `(color_format, sample_count)` 分套（wgpu 硬约束，
    /// 不得跨键复用）。
    ///
    /// ```ignore
    /// let ctx = RenderEntry::new_context(GpuSettings::default())?;
    /// let s1 = RenderEntry::new_surface(&ctx, &window1, SurfaceSettings::default())?;
    /// let s2 = RenderEntry::new_surface(&ctx, &window2, SurfaceSettings::default())?;
    /// ```
    #[cfg(not(target_arch = "wasm32"))]
    #[cfg(feature = "we")]
    pub fn new_surface(
        context: &RenderContext,
        window: &Window,
        surface_settings: SurfaceSettings,
    ) -> Result<RenderSurface, RenderContextError> {
        starfish_std::debug::assert_main_thread("RenderEntry::new_surface");
        block_on(Self::async_new_surface(context, window, surface_settings))
    }

    /// [`new_surface`](Self::new_surface) 的异步版（Web/异步宿主用）。
    #[cfg(feature = "we")]
    pub async fn async_new_surface(
        context: &RenderContext,
        window: &Window,
        surface_settings: SurfaceSettings,
    ) -> Result<RenderSurface, RenderContextError> {
        // 与首窗口同款路径：raw 句柄 + 'static surface（契约见 async_new）
        let raw_display = window.display_handle()?.as_raw();
        let raw_window = window.window_handle()?.as_raw();
        unsafe {
            Self::async_surface_from_raw_handles(
                context,
                raw_window,
                Some(raw_display),
                window.size(),
                surface_settings,
            )
        }
        .await
    }

    /// Web：从已有上下文为 `<canvas>` 挂表面（`SurfaceTarget::Canvas`
    /// 直连，无需窗口层句柄；多 canvas 预留入口）。
    #[cfg(target_arch = "wasm32")]
    pub async fn async_new_surface_from_canvas(
        context: &RenderContext,
        canvas: web_sys::HtmlCanvasElement,
        surface_settings: SurfaceSettings,
    ) -> Result<RenderSurface, RenderContextError> {
        let instance = context.instance().clone();
        let adapter = context.adapter();
        let device = context.device().clone();
        let queue = context.queue().clone();

        // 安全 API：SurfaceTarget::Canvas（wasm32 + web feature 即存在）
        let surface = Arc::new(instance.create_surface(
            wgpu::SurfaceTarget::Canvas(canvas.clone()),
        )?);

        let caps = surface.get_capabilities(adapter);
        let downlevel_caps = adapter.get_downlevel_capabilities();
        let size = (canvas.width(), canvas.height());

        Ok(RenderSurface::new(
            &surface,
            &device,
            &queue,
            &size,
            surface_settings,
            &caps,
            &downlevel_caps,
        ))
    }




    // =========================================================
    // 第一层：设备先行 + 从 raw handle 挂表面（最通用）
    // =========================================================

    /// 基于已存在的 `RenderContext`，用任意窗口框架的 raw handle 创建渲染表面。
    ///
    /// 这是给 **eframe / winit / tao / slint** 等第三方框架的**通用入口**。
    /// 调用方只须从自己的窗口对象上取到 `RawWindowHandle`（及 `RawDisplayHandle`），
    /// 无需构造本引擎的 `Window`。
    ///
    /// # Safety
    ///
    /// 与 `create_surface_unsafe` 同款契约，调用方必须保证：
    ///
    /// 1. `raw_window` / `raw_display` 在返回的 `RenderSurface` 存活期内**始终有效**，
    ///    且指向的底层原生对象**不被销毁**（即窗口生命周期 ≥ surface 生命周期）；
    /// 2. 遵循 `raw-window-handle` 的 `HasWindowHandle`/`HasDisplayHandle` 安全约定
    ///    （句柄在 drop 前可用、跨线程规则等）；
    /// 3. 桌面平台须在主线程（Win32/macOS/iOS/Android/部分 X11）调用——
    ///    本函数内部会 `assert_main_thread` 兜底。
    ///
    /// 违反上述契约是 **UB**。实践中只要"surface 与 window 在同一作用域内、
    /// window 先 drop 得晚"就安全，这也是 winit+wgpu 应用的标准形态。
    ///
    /// # Example（winit）
    ///
    /// ```ignore
    /// let (ctx, _access, _surface) =
    ///     RenderEntry::new(&window, None, None)?;   // 或 new_context 先建设备
    ///
    /// // 之后每帧 resize 或挂第二表面时：
    /// let (ctx2, surface2) = unsafe {
    ///     RenderEntry::surface_from_raw_handles(
    ///         &ctx,
    ///         window.window_handle()?.as_raw(),
    ///         Some(window.display_handle()?.as_raw()),
    ///         window.inner_size().into(),
    ///         SurfaceSettings::default(),
    ///     )?
    /// };
    /// ```
    pub unsafe fn surface_from_raw_handles(
        context: &RenderContext,
        raw_window: RawWindowHandle,
        raw_display: Option<RawDisplayHandle>,
        size: (u32, u32),
        surface_settings: SurfaceSettings,
    ) -> Result<RenderSurface, RenderContextError> {
        starfish_std::debug::assert_main_thread("RenderEntry::surface_from_raw_handles");

        let instance = context.instance().clone();
        let adapter = context.adapter();
        let device = context.device().clone();
        let queue = context.queue().clone();

        // 与 async_new 同款路径：raw handle → 'static surface
        let surface = Arc::new(
            unsafe { instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
                .map_err(RenderContextError::CreateSurface) }?,
        );

        let caps = surface.get_capabilities(adapter);
        let downlevel_caps = adapter.get_downlevel_capabilities();

        Ok(RenderSurface::new(
            &surface,
            &device,
            &queue,
            &size,
            surface_settings,
            &caps,
            &downlevel_caps,
        ))
    }

    /// [`surface_from_raw_handles`](Self::surface_from_raw_handles) 的异步版本。
    ///
    /// 语义完全相同，仅移除 `pollster::block_on` 包裹以便在异步宿主（web 装载
    /// 任务、tokio runtime、Python 的 `asyncio`）中直接 `await`。
    ///
    /// # Safety
    /// 同 [`surface_from_raw_handles`](Self::surface_from_raw_handles)。
    pub async unsafe fn async_surface_from_raw_handles(
        context: &RenderContext,
        raw_window: RawWindowHandle,
        raw_display: Option<RawDisplayHandle>,
        size: (u32, u32),
        surface_settings: SurfaceSettings,
    ) -> Result<RenderSurface, RenderContextError> {
        // 主体与同步版完全一致——异步版之所以独立存在，是为了给宿主
        // 一个"不阻塞"的入口（有些框架在 async context 里不允许 block_on）。
        let instance = context.instance().clone();
        let adapter = context.adapter();
        let device = context.device().clone();
        let queue = context.queue().clone();

        let surface = Arc::new(
            unsafe { instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
                .map_err(RenderContextError::CreateSurface) }?,
        );

        let caps = surface.get_capabilities(adapter);
        let downlevel_caps = adapter.get_downlevel_capabilities();

        Ok(RenderSurface::new(
            &surface,
            &device,
            &queue,
            &size,
            surface_settings,
            &caps,
            &downlevel_caps,
        ))
    }

    // =========================================================
    // 第二层：一步到位（设备 + 表面），便利接口
    // =========================================================

    /// 一步完成"建实例/设备 + 建表面"，面向**不想自己管 RenderContext**
    /// 的简单宿主（winit 小例子、demo、测试）。
    ///
    /// 返回 `(RenderContext, RenderResourceAccess, RenderSurface)`——
    /// 与 `RenderEntry::new` 形状对齐，把 `Window` 换成 raw handle。
    ///
    /// # Safety
    /// 同 [`surface_from_raw_handles`](Self::surface_from_raw_handles)。
    #[cfg(not(target_arch = "wasm32"))]
    pub unsafe fn new_from_raw_handles(
        raw_window: RawWindowHandle,
        raw_display: Option<RawDisplayHandle>,
        size: (u32, u32),
        surface_settings: Option<SurfaceSettings>,
        gpu_settings: Option<GpuSettings>,
    ) -> Result<(RenderContext, RenderResourceAccess, RenderSurface), RenderContextError> {
        starfish_std::debug::assert_main_thread("RenderEntry::new_from_raw_handles");
        pollster::block_on(unsafe { Self::async_new_from_raw_handles(
            raw_window,
            raw_display,
            size,
            surface_settings.unwrap_or_default(),
            gpu_settings.unwrap_or_default(),
        ) })
    }

    /// [`new_from_raw_handles`](Self::new_from_raw_handles) 的异步版本。
    ///
    /// # Safety
    /// 同 [`surface_from_raw_handles`](Self::surface_from_raw_handles)。
    pub async unsafe fn async_new_from_raw_handles(
        raw_window: RawWindowHandle,
        raw_display: Option<RawDisplayHandle>,
        size: (u32, u32),
        surface_settings: SurfaceSettings,
        gpu_settings: GpuSettings,
    ) -> Result<(RenderContext, RenderResourceAccess, RenderSurface), RenderContextError> {
        // 1) 建 instance + surface（先有 surface 才能拿 caps 请求 adapter）
        let instance = wgpu::Instance::new(gpu_settings.to_instance());
        let surface = Arc::new(
            unsafe { instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
                .map_err(RenderContextError::CreateSurface) }?,
        );

        // 2) 请求 adapter（带 surface 兼容性过滤）
        let adapter = instance
            .request_adapter(&gpu_settings.to_adapter(&surface))
            .await
            .map_err(|_| RenderContextError::RequestAdapter)?;

        let caps = surface.get_capabilities(&adapter);
        let downlevel_caps = adapter.get_downlevel_capabilities();

        // 3) 逻辑设备/队列
        let granted_features = gpu_settings.resolve_features(&adapter);
        let granted_limits = gpu_settings.resolve_limits(&adapter);
        let adapter_info = adapter.get_info();

        let (device, queue) = adapter
            .request_device(&gpu_settings.to_device(&adapter))
            .await?;
        let (device, queue) = (Arc::new(device), Arc::new(queue));

        // 4) 未捕获错误钩子（与 async_new 保持一致）
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
            &device,
            &queue,
            granted_features,
            granted_limits,
            adapter_info,
        );

        super::features::report(gpu_settings.desired_features, granted_features);

        let render_surface = RenderSurface::new(
            &surface,
            &device,
            &queue,
            &size,
            surface_settings,
            &caps,
            &downlevel_caps,
        );

        let render_resource_access = RenderResourceAccess::new(
            device,
            queue,
            render_surface.color_format(),
            render_surface.depth_format(),
        );

        Ok((render_context, render_resource_access, render_surface))
    }

    // =========================================================
    // 第三层：接受外部 wgpu Device（egui / 多后端共享场景）
    // =========================================================

    /// 从**外部已有的 `wgpu::Device`/`Queue`** 创建渲染表面。
    ///
    /// 用于 egui-wgpu 的 `RenderState` 已经建了 device、宿主想把渲染挂到
    /// 同一设备上的场景——避免两套 device 互相拷贝资源。
    ///
    /// 注意：本引擎的 `RenderContext` 需要 instance/adapter 才能做后续
    /// surface reconfigure 等操作，而这两个通常**只能从 wgpu 拿不到**，
    /// 所以本函数只返回 `RenderSurface`，调用方须自行维护 device 生命周期。
    /// 若需要完整 `RenderContext`，请用第二层接口自己建 device。
    ///
    /// # Safety
    /// 除 [`surface_from_raw_handles`](Self::surface_from_raw_handles) 的窗口
    /// 契约外，还须保证传入的 `instance`/`adapter`/`device`/`queue` 同源
    /// （由同一 instance 创建），否则 `create_surface` 内部会 panic/报错。
    pub unsafe fn surface_from_foreign_device(
        instance: &wgpu::Instance,
        adapter: &wgpu::Adapter,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        raw_window: RawWindowHandle,
        raw_display: Option<RawDisplayHandle>,
        size: (u32, u32),
        surface_settings: SurfaceSettings,
    ) -> Result<RenderSurface, RenderContextError> {
        let surface = Arc::new(
            unsafe { instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: raw_display,
                    raw_window_handle: raw_window,
                })
                .map_err(RenderContextError::CreateSurface) }?,
        );

        let caps = surface.get_capabilities(adapter);
        let downlevel_caps = adapter.get_downlevel_capabilities();
        let (device, queue)  = (Arc::new(device.clone()),Arc::new(queue.clone()));
        
        Ok(RenderSurface::new(
            &surface,
            &device,
            &queue,
            &size,
            surface_settings,
            &caps,
            &downlevel_caps,
        ))
    }
}