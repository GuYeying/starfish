//! 运行控制（**嵌入式宿主的停止通道**）：长驻进程（如 Android 嵌入式
//! 宿主 app）内，运行中的游戏需要被外部"杀死"——线程无法强杀，取消
//! 标志 + 帧边界检查 = 唯一安全的停止方式（2026-10-26 嵌入式 .so 接口
//! 设计定约）。
//!
//! - 宿主接口 `stop()` → [`request_stop`]；
//! - 游戏帧循环（pygame-rs testhost / 各平台宿主驱动）在帧边界检查
//!   [`stop_requested`]，命中即收口（引擎卸载序列）；
//! - 未置位 = 零开销（一次原子读/帧）。
//!
//! 桌面/Web 独立进程形态天然不受影响（从不置位）。

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

/// 宿主请求停止当前运行（帧边界生效）
pub fn request_stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// 帧边界检查（游戏帧循环每帧调用一次）
pub fn stop_requested() -> bool {
    STOP.load(Ordering::SeqCst)
}

/// 运行开始前复位（嵌入场景同进程多次运行的间隔清理）
pub fn reset() {
    STOP.store(false, Ordering::SeqCst);
}
