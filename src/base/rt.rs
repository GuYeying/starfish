//! 运行线程契约（调试期保障；release 构建零成本）
//!
//! 契约内容（设计定稿见 reference/SDL退役与平台迁移设计笔记.md 第五节）：
//! - **仅主线程**：`run()`、窗口操作、事件派发、`RenderSurface` 的
//!   begin_frame/present/resize
//! - **任意线程**：音频全家（AudioMixer/MusicPlayer/SFX）、输入快照读
//!
//! 实现方式：`run()` 进入时把当前线程钉为"主线程锚点"（OnceLock，只钉一次），
//! 之后各主线程 API 以 [`debug_assert_main_thread`] 比对——跨线程误用在调试
//! 构建直接 panic 定位，release 构建零开销。wasm 单线程天然满足，断言恒过。

use std::sync::OnceLock;
use std::thread::{current, ThreadId};

static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

/// 在程序入口（run）把当前线程标记为主线程锚点
pub(crate) fn mark_main_thread() {
    let _ = MAIN_THREAD.set(current().id());
}

/// 调试断言：当前处于主线程（锚点未设置时放行——便于纯逻辑测试直调）
pub(crate) fn debug_assert_main_thread(site: &'static str) {
    if let Some(main) = MAIN_THREAD.get() {
        debug_assert_eq!(
            *main,
            current().id(),
            "线程契约违例：{site} 必须在主线程调用"
        );
    }
}
