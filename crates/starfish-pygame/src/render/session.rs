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
    /// Screen 会话槽（2026-11-16 多窗批次 per-id 化——"main" = 主窗；
    /// 旧单槽语义 = "main"（begin_screen()/with_screen() 全绑定面零
    /// 翻新）。约定互斥不嵌套：同帧各窗先后 with 块）
    static SCREEN_SESSIONS: RefCell<std::collections::HashMap<String, RenderPass<'static>>>
        = RefCell::new(std::collections::HashMap::new());
    /// 当前活跃窗口 id（with_screen/bridge::with_session 的路由基准——
    /// begin_screen_id 置位、end_screen_id 恢复 "main"；37 处绘制臂
    /// 零改的关键）
    static CURRENT_SCREEN: RefCell<String> = RefCell::new("main".to_string());
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
    begin_screen_id("main")
}

/// 开启指定窗口 Screen 会话（多窗批次——id 查 display 注册表；**置
/// 当前活跃 id**——with_screen/绘制臂路由随之切窗）
pub fn begin_screen_id(id: &str) {
    let screen = crate::display::screen(Some(id))
        .unwrap_or_else(|| panic!("session.begin_screen_id: 窗口 {id:?} 未装配"));
    let pass: RenderPass<'static> = screen.render();
    SCREEN_SESSIONS.with(|s| {
        s.borrow_mut()
            .insert(id.to_string(), pass);
    });
    CURRENT_SCREEN.with(|c| *c.borrow_mut() = id.to_string());
}

/// 开启 Screen **深度会话**（`render_depth`：绘制走深度测试管线，深度
/// 缓冲随 base 帧清屏清 1.0——3D 内容 / 2.5D 遮挡；相机经
/// `RenderPass::set_default_camera` 换透视）。槽位与普通 Screen 会话共用
/// （同一时间一个活跃 Screen 会话——开深度会话即替换普通会话）。
pub fn begin_screen_depth() {
    begin_screen_depth_id("main")
}

/// 深度会话 per-id 口（多窗批次）。
pub fn begin_screen_depth_id(id: &str) {
    let screen = crate::display::screen(Some(id))
        .unwrap_or_else(|| panic!("session.begin_screen_depth_id: 窗口 {id:?} 未装配"));
    let pass: RenderPass<'static> = screen.render_depth();
    SCREEN_SESSIONS.with(|s| {
        s.borrow_mut()
            .insert(id.to_string(), pass);
    });
}

/// 结束 Screen 会话（finish 幂等打包；Drop 兜底双保险）
pub fn end_screen() {
    end_screen_id("main")
}

/// 结束指定窗口 Screen 会话（多窗批次——恢复当前活跃 id 为 "main"）。
pub fn end_screen_id(id: &str) {
    SCREEN_SESSIONS.with(|s| {
        if let Some(pass) = s.borrow_mut().remove(id) {
            pass.finish();
        }
    });
    CURRENT_SCREEN.with(|c| *c.borrow_mut() = "main".to_string());
}

/// 在活跃 Screen 会话上执行绘制（无会话 = [`NoActiveSession`]；活跃
/// = CURRENT_SCREEN——begin_screen_id 切窗后路由随之）
pub fn with_screen<T>(f: impl FnOnce(&RenderPass<'static>) -> T) -> Result<T, NoActiveSession> {
    let cur = CURRENT_SCREEN.with(|c| c.borrow().clone());
    with_screen_id(&cur, f)
}

/// 指定窗口会话绘制（多窗批次）。
pub fn with_screen_id<T>(
    id: &str,
    f: impl FnOnce(&RenderPass<'static>) -> T,
) -> Result<T, NoActiveSession> {
    SCREEN_SESSIONS.with(|s| match s.borrow().get(id) {
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
