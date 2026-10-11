//! 窗口与事件：[`starfish_window`] 的再导出垫片（SDL 风格 pull 模型）。
//!
//! starfish-window 直接为当前模块量身实现（替代 winit 的控制反转回调模型）：
//! 调用方持循环，`poll_event` 拉事件；窗口操作都是普通方法调用。
//! 本模块只做类型再导出——**不重复定义**KeyCode/Event/Window（消灭重复层）。
//!
//! - `Window`：单窗口（进程内唯一），`poll_event` 非阻塞拉取，
//!   `set_*` 运行期方法族；raw-window-handle 直通（wgpu 建表面零 unsafe）
//! - `Event`：全平台统一词汇表（含 Touch / Suspended / Resumed 移动端变体）
//! - `KeyCode`：W3C KeyboardEvent.code 命名（~105 键 + `Other`）
//! - `WindowFlags`：创建标志（SDL 语义位组合）

pub use starfish_window::{
    Event, KeyCode, Modifiers, MouseButton, TouchPhase, Window, WindowError, WindowFlags,
};
/// Android 平台入口的系统应用句柄（仅 Android 构建）。
#[cfg(target_os = "android")]
pub use starfish_window::AndroidApp;
/// Android 生命周期装配入口（仅 Android 构建）。
#[cfg(target_os = "android")]
pub use starfish_window::android_init;

/// 等待下一帧（桌面 no-op async / Web rAF yield）。
/// 三平台统一：应用体里无条件调用，不需要 cfg。
/// 本体在 starfish-window 0.2.0 起（§poll 模型的逐帧让出原语）——
/// 这里仅再导出，保持引擎稳定路径。
pub use starfish_window::next_frame;
