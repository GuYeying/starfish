


1.去除了resouces里的Shader模块，并对其他模块依赖Shader进行剔除

2.
### Added
- **第三方窗口框架接入 API**：`RenderEntry` 新增面向 winit / eframe / tao / slint
  等外部框架的渲染入口，直接接受 `raw_window_handle::{RawWindowHandle,
  RawDisplayHandle}`，无需构造引擎自身的 `Window`。
  - `surface_from_raw_handles` / `async_surface_from_raw_handles`
    —— 基于已有 `RenderContext` 挂表面（设备先行，最通用）。
  - `new_from_raw_handles` / `async_new_from_raw_handles`
    —— 一步建 device + surface 的便利入口（对齐 `RenderEntry::new` 返回形状）。
- `raw_window_handle` 的 `RawDisplayHandle` / `RawWindowHandle` 现为公开
  接口的一部分（`unsafe fn`，契约见文档）。