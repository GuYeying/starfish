//! pygame.surface 对应物（契约 ADR-1：离屏绘制面；架构 §九.2 绑定定约）。
//!
//! 本体 = [`Surface`]（[`crate::pygame::render`] 实现：会话/DrawTarget/
//! blit/transform 语义所在地）。本模块为 pygame 名发现性的正式入口。
//!
//! ## 绑定定约（架构 §九.2，2026-09-29）
//!
//! - **创建 = color + 可选 depth 整合体**：`Surface::new(size)` 仅 color；
//!   `Surface::with_depth(size)` 附 Depth24Plus 深度缓冲（Python 侧映射
//!   `pygame.Surface((w,h), depth_buffer=True)`）。⚠️ 真实 pygame 的
//!   `depth` 参数是每像素位数（bpp）——绑定层**禁用 `depth=` 表达深度
//!   缓冲**，用 `depth_buffer=`。
//! - **`has_depth` 创建时定格**：`render_depth()` 深度会话构造须
//!   `with_depth`（native 校验，无深度明确报错）。
//! - **Screen 无需本模块**：`display::set_mode` 的 Screen 深度缓冲默认
//!   创建（Depth24Plus 无条件），`screen.render_depth()` 开箱即用。

pub use crate::pygame::render::Surface;

/// 创建离屏绘制面（仅 color）
pub fn new(size: (u32, u32)) -> Surface {
    Surface::new(size)
}

/// 创建离屏绘制面 + 深度缓冲（Depth24Plus；[`Surface::render_depth`] 用）
pub fn with_depth(size: (u32, u32)) -> Surface {
    Surface::with_depth(size)
}
