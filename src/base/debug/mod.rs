//! 开发者诊断设施：日志输出 / 崩溃转发 / 线程契约守卫
//!
//! 组织方式（对齐 video 的按平台分文件模式）：
//! - [`mod@super`]（本文件）：平台中立的**统一入口**（console_log /
//!   assert_main_thread）与线程契约
//! - [`web`]：wasm 实现（console.log / panic hook 落浏览器控制台）
//! - [`native`]：原生实现（stdout；panic 天然走 stderr）
//!
//! 定位：本模块是所有"调试期辅助"的唯一家——**底层调试，严格不做
//! 跨平台抽象的强行归一**：平台专属能力保持平台门控语义（如
//! install_panic_hook 仅 wasm 有意义），统一只统一"两边都有的部分"
//! （如日志输出）。开发者后续的调试工具（帧率统计、GPU 标签、性能
//! 打点等）按平台落对应文件。

use std::sync::OnceLock;
use std::thread::{current, ThreadId};

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(not(target_arch = "wasm32"))]
mod native;

// ── 日志输出（统一入口；平台实现在子模块）───────────────────────────

/// 跨平台日志：wasm → 浏览器控制台（console.log）；桌面 → stdout。
///
/// wasm 没有 stdout/stderr，`println!` 在浏览器无处可去——Web 应用的
/// 诊断输出一律走本函数。
pub fn console_log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    web::log(msg);
    #[cfg(not(target_arch = "wasm32"))]
    native::log(msg);
}

/// 安装 panic 转发：wasm 上把 panic 信息（含位置）落到浏览器控制台；
/// 桌面 panic 天然走 stderr，无此需求——**平台门控，不强行归一**。
///
/// 由 [`app_entry!`](crate::app_entry)
/// 宏自动调用，一般无需手动使用。
#[cfg(target_arch = "wasm32")]
pub fn install_panic_hook() {
    web::install_panic_hook();
}

// ── 线程契约守卫（调试期保障；release 构建零成本）──────────────────
//
// 契约内容（设计定稿见 reference/SDL退役与平台迁移设计笔记.md 第五节）：
// - **仅主线程**：`run()`、窗口操作、事件派发、`RenderSurface` 的
//   begin_frame/present/resize
// - **任意线程**：音频全家（AudioMixer/MusicPlayer/SFX）、输入快照读
//
// 实现方式：引擎入口（inner_run）把当前线程钉为"主线程锚点"（OnceLock，
// 只钉一次），之后各主线程 API 以 [`assert_main_thread`] 比对——跨线程
// 误用在调试构建直接 panic 定位，release 构建零开销。wasm 单线程天然
// 满足，断言恒过。

static MAIN_THREAD: OnceLock<ThreadId> = OnceLock::new();

/// 在程序入口（inner_run）把当前线程标记为主线程锚点
pub(crate) fn mark_main_thread() {
    let _ = MAIN_THREAD.set(current().id());
}

/// 调试断言：当前处于主线程（锚点未设置时放行——便于纯逻辑测试直调）。
///
/// 公开给开发者的自保工具：自己的主线程专属 API 也可在入口处调用，
/// 跨线程误用在 debug 构建立即 panic 指明现场，release 零成本。
pub fn assert_main_thread(site: &'static str) {
    if let Some(main) = MAIN_THREAD.get() {
        debug_assert_eq!(
            *main,
            current().id(),
            "线程契约违例：{site} 必须在主线程调用"
        );
    }
}
