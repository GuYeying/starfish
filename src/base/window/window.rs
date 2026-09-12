//! 窗口（winit 后端）
//!
//! [`Window`] 由 [`run`](crate::base::app::run) 在平台回调里创建并注入
//! [`Ctx`](crate::base::app::Ctx)，用户不直接构造。渲染层经
//! `HasWindowHandle`/`HasDisplayHandle` 提取 raw 句柄（winit 原生实现），
//! 不再经过任何第三方转发。

use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window as WinitWindow;

/// 引擎窗口句柄（winit 后端，Clone 廉价）
#[derive(Clone)]
pub struct Window {
    pub(crate) inner: Arc<WinitWindow>,
}

impl Window {
    pub(crate) fn from_winit(inner: WinitWindow) -> Self {
        Self { inner: Arc::new(inner) }
    }

    /// 窗口唯一 id（多窗口区分用）
    pub fn id(&self) -> u64 {
        self.inner.id().into()
    }

    /// 窗口客户区尺寸（物理像素）——渲染表面 configure 基准
    pub fn size(&self) -> (u32, u32) {
        let s = self.inner.inner_size();
        (s.width, s.height)
    }

    /// DPI 缩放因子（1.0 = 无缩放）
    pub fn scale_factor(&self) -> f64 {
        self.inner.scale_factor()
    }

    /// 请求系统合成器安排重绘（引擎循环内部使用）
    pub(crate) fn request_redraw(&self) {
        self.inner.request_redraw();
    }

    /// 设置标题
    pub fn set_title(&self, title: &str) {
        self.inner.set_title(title);
    }

    /// 是否可调整大小
    pub fn set_resizable(&self, resizable: bool) {
        self.inner.set_resizable(resizable);
    }

    /// 显示/隐藏窗口
    pub fn set_visible(&self, visible: bool) {
        self.inner.set_visible(visible);
    }

    /// 置顶
    pub fn set_always_on_top(&self, on_top: bool) {
        self.inner.set_window_level(if on_top {
            winit::window::WindowLevel::AlwaysOnTop
        } else {
            winit::window::WindowLevel::Normal
        });
    }

    /// 显示/隐藏光标
    pub fn set_cursor_visible(&self, visible: bool) {
        self.inner.set_cursor_visible(visible);
    }

    /// 相对鼠标模式（对齐 SDL `RelativeMouseMode`）：锁定光标 + 隐藏，
    /// `MouseMoved` 事件转为无界相对运动（FPS 相机用）
    ///
    /// 注意：锁定失败时（部分平台/时机）静默降级为仅隐藏光标。
    pub fn set_relative_mouse(&self, relative: bool) {
        use winit::window::CursorGrabMode;
        self.set_cursor_visible(!relative);
        let mode = if relative { CursorGrabMode::Locked } else { CursorGrabMode::None };
        // Locked 在部分平台需先 Confine 过渡，两次尝试取其一定成功
        if self.inner.set_cursor_grab(mode).is_err() {
            let _ = self.inner.set_cursor_grab(CursorGrabMode::Confined);
        }
    }

    /// 底层 winit 窗口（crate 内部用；不进入公共 API）
    pub(crate) fn winit(&self) -> &WinitWindow {
        &self.inner
    }
}

// raw 句柄转发：wgpu 建表面直接吃本类型（render_entry 边界）
impl HasWindowHandle for Window {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.inner.window_handle()
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.inner.display_handle()
    }
}

impl std::fmt::Debug for Window {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Window")
            .field("id", &self.id())
            .field("size", &self.size())
            .field("scale_factor", &self.scale_factor())
            .finish()
    }
}
