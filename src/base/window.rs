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
#[cfg(not(target_arch = "wasm32"))]
pub async fn next_frame() {}

/// 等待下一帧（Web 帧拍原语；requestAnimationFrame 的 Promise 桥）。
/// 桌面无需此原语——循环体后 `Clock::tick` / vsync present 自然节流。
#[cfg(target_arch = "wasm32")]
pub async fn next_frame() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NF: AtomicU32 = AtomicU32::new(0);
    let n = NF.fetch_add(1, Ordering::Relaxed);
    if n % 60 == 0 {
        crate::base::debug::console_log(&format!("[window] next_frame 进入 {n}"));
    }
    let promise = js_sys::Promise::new(&mut |resolve: js_sys::Function,
                                             _reject: js_sys::Function| {
        web_sys::window()
            .expect("web: 无浏览器窗口")
            .request_animation_frame(&resolve)
            .expect("requestAnimationFrame 失败");
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    if n % 60 == 0 {
        crate::base::debug::console_log(&format!("[window] next_frame 完成 {n}"));
    }
}
