//! 活跃绘制会话（ADR-5 `with` 语法的引擎侧状态承载）
//!
//! [`RenderPass`] 是借用型会话对象；本模块提供 **thread-local 活跃槽**：
//! begin 入槽、with 路由、end 打包——绑定层的 Python
//! `with screen.render():` / `with pygame.targets([...]):` 与 Rust 侧
//! 帧循环同吃这一份状态（绘制行为单源，不因语言分叉）。
//!
//! 双槽并行（调用方约定互斥不嵌套）：
//! - **Screen 槽**：`begin_screen` = `display::get_screen().render()`
//!   （get_screen 为进程唯一 &'static，`RenderPass<'static>` 天然成立，
//!   无 unsafe）；
//! - **Targets 槽**：`begin_targets` = [`render_targets_arc`]——目标
//!   `Arc` 由槽内 KEEPALIVE 保活至 end（render_targets_arc 的 SAFETY
//!   契约就地兑现，不再外推给调用方）。
//!
//! 错误面：`with_*` 无活跃会话返回 [`NoActiveSession`]（绑定层映射为
//! Python 异常；Rust 侧可直接处理）。

use std::cell::RefCell;
use std::sync::Arc;

use super::target::{render_targets_arc, RenderPass};
use super::Surface;

thread_local! {
    static SCREEN_SESSION: RefCell<Option<RenderPass<'static>>> = const { RefCell::new(None) };
    static TARGETS_SESSION: RefCell<Option<RenderPass<'static>>> = const { RefCell::new(None) };
    /// Targets 槽保活表（render_targets_arc 的 SAFETY 证据——end 时
    /// finish 先行、清表在后，顺序不可换）
    static TARGETS_KEEPALIVE: RefCell<Vec<Arc<Surface>>> = const { RefCell::new(Vec::new()) };
}

/// 无活跃会话（`with_*` 在 begin 之前调用）
#[derive(Debug, Clone, Copy)]
pub struct NoActiveSession;

// ── Screen 会话（`with screen.render():`）────────────────────────

/// 开启 Screen 会话（display 未建 = get_screen 的契约 panic，ADR-9）
pub fn begin_screen() {
    let pass: RenderPass<'static> = crate::display::get_screen().render();
    SCREEN_SESSION.with(|s| *s.borrow_mut() = Some(pass));
}

/// 开启 Screen **深度会话**（`render_depth`：绘制走深度测试管线，深度
/// 缓冲随 base 帧清屏清 1.0——3D 内容 / 2.5D 遮挡；相机经
/// `RenderPass::set_default_camera` 换透视）。槽位与普通 Screen 会话共用
/// （同一时间一个活跃 Screen 会话——开深度会话即替换普通会话）。
pub fn begin_screen_depth() {
    let pass: RenderPass<'static> = crate::display::get_screen().render_depth();
    SCREEN_SESSION.with(|s| *s.borrow_mut() = Some(pass));
}

/// 结束 Screen 会话（finish 幂等打包；Drop 兜底双保险）
pub fn end_screen() {
    SCREEN_SESSION.with(|s| {
        if let Some(pass) = s.borrow_mut().take() {
            pass.finish();
        }
    });
}

/// 在活跃 Screen 会话上执行绘制（无会话 = [`NoActiveSession`]）
pub fn with_screen<T>(f: impl FnOnce(&RenderPass<'static>) -> T) -> Result<T, NoActiveSession> {
    SCREEN_SESSION.with(|s| match s.borrow().as_ref() {
        Some(pass) => Ok(f(pass)),
        None => Err(NoActiveSession),
    })
}

// ── Targets 会话（`with pygame.targets([a, b]):`——MRT）──────────

/// 开启 Targets 会话（Arc 保活 + 'static 化；目标不足 2 个或尺寸不齐
/// = render_targets 的 assert panic）
pub fn begin_targets(targets: &[Arc<Surface>]) {
    let pass = render_targets_arc(targets);
    TARGETS_KEEPALIVE.with(|k| *k.borrow_mut() = targets.to_vec());
    TARGETS_SESSION.with(|s| *s.borrow_mut() = Some(pass));
}

/// 结束 Targets 会话（finish 先行——内部仍触碰目标；保活表后清）
pub fn end_targets() {
    TARGETS_SESSION.with(|s| {
        if let Some(pass) = s.borrow_mut().take() {
            pass.finish();
        }
    });
    TARGETS_KEEPALIVE.with(|k| k.borrow_mut().clear());
}

/// 在活跃 Targets 会话上执行绘制（无会话 = [`NoActiveSession`]）
pub fn with_targets<T>(f: impl FnOnce(&RenderPass<'static>) -> T) -> Result<T, NoActiveSession> {
    TARGETS_SESSION.with(|s| match s.borrow().as_ref() {
        Some(pass) => Ok(f(pass)),
        None => Err(NoActiveSession),
    })
}
