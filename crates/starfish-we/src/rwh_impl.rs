//! The `wgpu` feature: plug [`Window`] into the raw-window-handle
//! ecosystem.
//! `wgpu` feature：把 [`Window`] 接入 raw-window-handle 生态。
//!
//! With these impls, `instance.create_surface(&window)` yields a surface
//! with zero unsafe. This is a mechanical mapping from our raw handles to
//! rwh types; wgpu itself is intentionally not a dependency.
//! 实现后，`instance.create_surface(&window)` 即可零 unsafe 拿到 surface。
//! 只做我们裸句柄 → rwh 类型的机械映射，不引入 wgpu 本体。

use std::num::NonZeroIsize;
use std::ptr::NonNull;

use raw_window_handle::{
    AndroidDisplayHandle, AndroidNdkWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
    Win32WindowHandle, WindowHandle, WindowsDisplayHandle,
};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use raw_window_handle::{WebCanvasWindowHandle, WebDisplayHandle};

use crate::Window;
use crate::handle::RawWindowHandle as SfHandle;

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = match self.raw_handle() {
            SfHandle::Win32 { hwnd, hinstance } => {
                // rwh handle types are #[non_exhaustive]: create via the
                // official constructor, then fill fields by assignment.
                // rwh 的句柄类型是 #[non_exhaustive]，用官方构造器创建、字段赋值补全。
                let hwnd = NonZeroIsize::new(hwnd as isize).ok_or(HandleError::Unavailable)?;
                let mut handle = Win32WindowHandle::new(hwnd);
                handle.hinstance = NonZeroIsize::new(hinstance as isize);
                RawWindowHandle::Win32(handle)
            }
            SfHandle::Android { a_native_window } => {
                let ptr = NonNull::new(a_native_window).ok_or(HandleError::Unavailable)?;
                RawWindowHandle::AndroidNdk(AndroidNdkWindowHandle::new(ptr))
            }
            // Linux (Wayland): real libwayland pointers (the backend
            // builds with the `system` wayland backend for exactly this).
            // Linux（Wayland）：真实 libwayland 指针（后端为此特意用
            // wayland 的 `system` 后端构建）。
            SfHandle::Wayland { surface, .. } => {
                let ptr = NonNull::new(surface).ok_or(HandleError::Unavailable)?;
                RawWindowHandle::Wayland(WaylandWindowHandle::new(ptr))
            }
            // Web: the canvas element's JsValue object pointer →
            // WebCanvasWindowHandle (consumed by wgpu's WebGPU/WebGL
            // backends; the variant exists only on wasm builds).
            // Web：canvas 元素的 JsValue 对象指针 → WebCanvasWindowHandle
            // （wgpu 的 WebGPU/WebGL 后端同源消费；仅 wasm 构建存在该变体）
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            SfHandle::Web { canvas } => {
                let ptr = NonNull::new(canvas).ok_or(HandleError::Unavailable)?;
                RawWindowHandle::WebCanvas(WebCanvasWindowHandle::new(ptr))
            }
            // Backends not landed yet (macOS / Linux / iOS, and the Web arm
            // on non-wasm builds).
            // 未落地平台（macOS/Linux/iOS 及非 wasm 的 Web 臂）
            _ => return Err(HandleError::Unavailable),
        };
        // SAFETY: the pointer comes from a live window; the 'a borrow is
        // tied to &self.
        // SAFETY: 指针来自存活的窗口，'a 借用期与 &self 绑定。
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = match self.raw_handle() {
            // rwh 0.6's Windows display handle carries no data.
            // rwh 0.6 的 Windows display 句柄不携带数据。
            SfHandle::Win32 { .. } => RawDisplayHandle::Windows(WindowsDisplayHandle::new()),
            SfHandle::Android { .. } => RawDisplayHandle::Android(AndroidDisplayHandle::new()),
            // Linux (Wayland): the display side carries the wl_display
            // pointer.
            // Linux（Wayland）：display 侧携带 wl_display 指针。
            SfHandle::Wayland { display, .. } => {
                let ptr = NonNull::new(display).ok_or(HandleError::Unavailable)?;
                RawDisplayHandle::Wayland(WaylandDisplayHandle::new(ptr))
            }
            // Web: an empty display handle (the canvas carries its own
            // display semantics).
            // Web：空 display 句柄（canvas 自带显示语义）
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            SfHandle::Web { .. } => RawDisplayHandle::Web(WebDisplayHandle::new()),
            // Unimplemented backends must not fall back to a fake Windows
            // display — report unavailable like the window side does.
            // 未实现后端不得伪造 Windows display，与窗口侧一致报不可用。
            _ => return Err(HandleError::Unavailable),
        };
        // SAFETY: these display handles are empty structs with no borrowed
        // data.
        // SAFETY: 这些 display 句柄为空结构，无借用数据。
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}
