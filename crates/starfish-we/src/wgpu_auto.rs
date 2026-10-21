//! `wgpu-auto` feature: the library takes over the resize→configure cycle.
//! `wgpu-auto` feature：由库接管 resize→configure 循环。
//!
//! Consumers create an [`AutoSurface`] once and never hand-write
//! `Event::Resized` handling again: every [`acquire`](AutoSurface::acquire)
//! reconciles the window size against the surface configuration and
//! reconfigures transparently (including the `Suboptimal`/`Outdated`
//! hints from wgpu).
//! 消费者只建一次 [`AutoSurface`]，从此不再手写 `Event::Resized` 处理：
//! 每次 [`acquire`](AutoSurface::acquire) 都会把窗口尺寸与 surface 配置
//! 对账并透明重配置（连 wgpu 的 `Suboptimal`/`Outdated` 提示也一并消化）。
//!
//! This is opt-in (`wgpu-auto` pulls in wgpu itself) — the base `wgpu`
//! feature stays a version-decoupled rwh passthrough.
//! 这是可选项（`wgpu-auto` 会引入 wgpu 本体）——基础 `wgpu` feature 仍是
//! 版本解耦的 rwh 直通层。

use crate::Window;

/// A wgpu surface whose size is reconciled against the window
/// automatically.
/// 尺寸自动与窗口对账的 wgpu surface。
///
/// Typical loop (no `Resized` handling anywhere):
/// 典型循环（全程不需要处理 `Resized`）：
///
/// ```ignore
/// let mut surface = AutoSurface::new(&window, &instance)?;
/// loop {
///     while let Some(event) = window.poll_event() { /* … */ }
///     if let Some(frame) = surface.acquire() {
///         let view = frame.texture.create_view(&Default::default());
///         // render → frame.present()
///     }
///     crate::next_frame().await;
/// }
/// ```
pub struct AutoSurface<'w> {
    window: &'w Window,
    surface: wgpu::Surface<'w>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
    /// The size the surface is currently configured for.
    /// surface 当前配置对应的尺寸。
    configured: (u32, u32),
}

/// `AutoSurface` construction failures.
/// `AutoSurface` 构建失败。
#[derive(Debug)]
pub enum AutoSurfaceError {
    /// `instance.create_surface(window)` rejected the window.
    /// `instance.create_surface(window)` 拒绝了窗口。
    CreateSurface(wgpu::CreateSurfaceError),
    /// No usable GPU adapter for this surface.
    /// 没有可用于该 surface 的 GPU adapter。
    NoAdapter,
    /// Logical device creation failed.
    /// 逻辑 device 创建失败。
    RequestDevice(wgpu::RequestDeviceError),
}

impl std::fmt::Display for AutoSurfaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoSurfaceError::CreateSurface(e) => {
                write!(f, "create_surface failed: {e}")
            }
            AutoSurfaceError::NoAdapter => {
                write!(f, "no usable GPU adapter / 未找到可用 GPU adapter")
            }
            AutoSurfaceError::RequestDevice(e) => {
                write!(f, "request_device failed: {e}")
            }
        }
    }
}

impl std::error::Error for AutoSurfaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AutoSurfaceError::CreateSurface(e) => Some(e),
            AutoSurfaceError::RequestDevice(e) => Some(e),
            AutoSurfaceError::NoAdapter => None,
        }
    }
}

impl<'w> AutoSurface<'w> {
    /// Creates the surface, picks an adapter/device and configures for the
    /// window's current size — one call instead of the whole boilerplate.
    /// 创建 surface、选择 adapter/device 并按窗口当前尺寸 configure——
    /// 一次调用替代整套样板。
    pub fn new(window: &'w Window, instance: &wgpu::Instance) -> Result<Self, AutoSurfaceError> {
        let surface = instance
            .create_surface(window)
            .map_err(AutoSurfaceError::CreateSurface)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .map_err(|_| AutoSurfaceError::NoAdapter)?;
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
                .map_err(AutoSurfaceError::RequestDevice)?;
        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities.formats[0];
        let alpha_mode = capabilities.alpha_modes[0];
        let configured = window.size();
        let mut this = Self {
            window,
            surface,
            device,
            queue,
            format,
            alpha_mode,
            configured,
        };
        this.reconfigure();
        Ok(this)
    }

    /// The surface texture format (for building render pipelines).
    /// surface 纹理格式（构建渲染管线用）。
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// A handle to the device (for creating buffers/pipelines).
    /// device 句柄（创建 buffer/管线用）。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// A handle to the queue (for submits).
    /// queue 句柄（提交用）。
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Acquires the next frame, transparently reconciling the size first.
    ///
    /// - `Some(frame)`: render it and call `frame.present()`.
    /// - `None`: skip this frame — `Timeout`/`Occluded` (minimized), or a
    ///   one-shot `Outdated` that was already reconfigured internally.
    ///
    /// 获取下一帧，先透明完成尺寸对账。
    ///
    /// - `Some(frame)`：渲染后调用 `frame.present()`。
    /// - `None`：跳过本帧——`Timeout`/`Occluded`（最小化），或已在内部
    ///   重配置过的一次性 `Outdated`。
    ///
    /// A `Lost` surface (rare) returns `None` persistently; recreate the
    /// AutoSurface in that case.
    /// `Lost` surface（罕见）会持续返回 `None`；此时重建 AutoSurface。
    pub fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        self.reconcile();
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => Some(frame),
            // Usable, but the surface no longer matches: reconfigure for
            // the next frame while still presenting this one.
            // 可用但已不匹配：本帧照常上屏，下一帧前重配置。
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                self.reconfigure();
                Some(frame)
            }
            // One-shot stale config: reconfigure once; the caller skips a
            // frame.
            // 一次性过期配置：重配置一次，调用方跳过本帧。
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                None
            }
            // Skip-and-retry semantics / 跳过稍后再取。
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Lost => None,
            _ => None,
        }
    }

    /// Reconciles the configured size with the window's current size;
    /// reconfigures only on change.
    /// 将已配置尺寸与窗口当前尺寸对账；有变化才重配置。
    fn reconcile(&mut self) {
        let size = self.window.size();
        if size != self.configured {
            self.reconfigure();
        }
    }

    /// Forces a reconfigure at the window's current size.
    /// 强制按窗口当前尺寸重配置。
    pub fn reconfigure(&mut self) {
        let (width, height) = self.window.size();
        self.configured = (width, height);
        self.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                width,
                height,
                present_mode: wgpu::PresentMode::Fifo,
                alpha_mode: self.alpha_mode,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
                color_space: wgpu::SurfaceColorSpace::Auto,
            },
        );
    }
}
