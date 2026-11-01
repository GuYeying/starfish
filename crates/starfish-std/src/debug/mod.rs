//! 线程契约守卫 + 停表（日志输出已收拢到 **starfish-debug** crate 的
//! `log` 模块——级别/文件/wasm 远程/第三方 log 桥接全在彼处；本模块
//! 只剩线程契约与计时设施）。
//!
//! 历史注：`console_log`/`devlog`/`set_diag_sink` 已随日志统一批撤除
//! （2026-10-31）——调用点全部迁往 `starfish_debug::log_*!` 宏。
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

// ── 跨平台毫秒停表（诊断打点用）────────────────────────────────────

/// 桌面 = Instant 真计时；wasm = 恒 0（std::time 在 wasm32-unknown-unknown
/// 未实现——`Instant::now` 直接 panic "time not implemented"，2026-10-26
/// web 宿主实测）。打点保留、耗时位归零；将来接 performance.now 补真值。
pub struct Stopwatch {
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
}

impl Stopwatch {
    pub fn now() -> Self {
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            start: std::time::Instant::now(),
        }
    }

    /// 自 [`Stopwatch::now`] 起的毫秒数
    pub fn ms(&self) -> u128 {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.start.elapsed().as_millis()
        }
        #[cfg(target_arch = "wasm32")]
        {
            0
        }
    }
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
