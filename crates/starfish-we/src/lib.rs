//! starfish-we: a cross-platform **single-window + events** crate
//! (SDL-style poll model).
//! starfish-we：跨平台**单窗口 + 事件** crate（SDL 风格 poll 模型）。
//!
//! Built to replace winit where the inversion of control hurts: the caller
//! (e.g. a scripting-language interpreter) owns the main loop and pulls
//! events, instead of handing control flow to event-loop callbacks.
//! 用于替换 winit：调用者（脚本语言解释器）拥有主循环，主动轮询事件，
//! 而不是把控制流交给事件循环的回调。
//!
//! ```ignore
//! use crate::{Event, Window, WindowFlags};
//!
//! let mut window = Window::new("demo", (800, 600), WindowFlags::default())?;
//! loop {
//!     while let Some(event) = window.poll_event() {
//!         if let Event::CloseRequested = event {
//!             return Ok(());
//!         }
//!         // ... handle the rest / 处理其余事件
//!     }
//!     // The interpreter advances its own tick here (bytecode / scheduling).
//!     // 解释器在这里推进自己的 tick（执行字节码 / 调度）
//! }
//! ```
//!
//! # Layering and features / 分层与 feature
//!
//! - **Base layer (default)**: window + events with zero third-party
//!   dependencies; [`Window::raw_handle`] exposes raw platform handles so
//!   any rendering backend can be bridged via unsafe.
//!   **基础层（默认）**：窗口 + 事件，零第三方依赖；[`Window::raw_handle`]
//!   暴露裸平台句柄，可用 unsafe 桥接任何渲染后端。
//! - **`wgpu` feature**: implements the raw-window-handle traits so
//!   `instance.create_surface(&window)` works with zero unsafe; wgpu itself
//!   is deliberately not a dependency.
//!   **`wgpu` feature**：实现 raw-window-handle trait，
//!   `instance.create_surface(&window)` 零 unsafe 直通；不引入 wgpu 本体。
//!
//! | feature | meaning / 含义 |
//! |---|---|
//! | `wgpu` | raw-window-handle interop / raw-window-handle 对接 |
//! | `android-demo` | built-in Android demo host / 内置 Android 演示宿主 |
//!
//! Architecture: each target platform is one parallel module under
//! [`platform`] (horizontal cfg); all implement the same `WindowInner`
//! interface while the public API stays platform-independent.
//! 架构：[`platform`] 下各目标平台一个平行模块（横向 cfg），
//! 各自实现同一套 `WindowInner` 接口，公共 API 平台无关。

#![deny(missing_docs)]

mod error;
mod event;
pub mod flags;
mod handle;
mod platform;
#[cfg(feature = "we-wgpu")]
mod rwh_impl;
#[cfg(feature = "wgpu-auto")]
mod wgpu_auto;
mod window;

pub use error::WindowError;
pub use event::{Event, KeyCode, Modifiers, MouseButton, TouchPhase, UserEvent};
pub use flags::WindowFlags;
pub use handle::RawWindowHandle;
#[cfg(feature = "wgpu-auto")]
pub use wgpu_auto::{AutoSurface, AutoSurfaceError};
pub use window::Window;
pub use window::{EventProxy, EventProxyClosed};

/// (Android builds only) Call from the host's `android_main` to wire up
/// the system callbacks.
/// （仅 Android 构建）宿主在 `android_main` 中调用，装配系统回调。
///
/// Platform-specific API by explicit exception: it only exists on Android
/// builds.
/// 平台特有 API 的显式例外：只存在于 Android 构建中。
#[cfg(target_os = "android")]
pub use platform::android::android_init;

/// (Android builds only) The system app handle — the parameter type of
/// [`android_init`], needed by the host's `android_main` signature.
/// （仅 Android 构建）系统应用句柄——[`android_init`] 的参数类型，
/// 宿主 `android_main` 签名所需。
#[cfg(target_os = "android")]
pub use android_activity::AndroidApp;

/// (iOS builds only) Test-only event injection: feeds a synthetic event
/// into the same buffer the OS path uses. Enables deterministic event
/// testing on the simulator (real touch delivery is a follow-up).
/// （仅 iOS 构建）测试用事件注入：将合成事件写入与 OS 路径相同的缓冲，
/// 支撑模拟器上的确定性事件测试（真实触摸投递属后续项）。
#[cfg(target_os = "ios")]
#[doc(hidden)]
pub use platform::ios::inject_test_event as ios_inject_test_event;

/// (iOS builds only) Host entry: install the tick and hand the main
/// thread to UIApplicationMain (never returns).
/// （仅 iOS 构建）宿主入口：安装 tick 并把主线程交给 UIApplicationMain
/// （正常情况下永不返回）。
#[cfg(target_os = "ios")]
pub use platform::ios::entry::ios_start;

/// (iOS builds only) Host entry macro — a statement form, called inside
/// `main`: hands the main thread to UIApplicationMain (never returns).
/// Unlike `android_main!` (which must define `android_main` at module
/// top level for the Android glue ABI), iOS has no such ABI contract —
/// the entry is a plain Rust `main`, so a statement is the right shape.
/// Defining a `fn main` here instead would silently become a dead nested
/// function when invoked inside another function body.
/// （仅 iOS 构建）宿主入口宏——**语句式**，在 `main` 体内调用：把主
/// 线程交给 UIApplicationMain（永不返回）。与 `android_main!` 不同
/// （Android glue 的 ABI 要求在模块顶层定义 `android_main`），iOS 没有
/// 这种 ABI 约定——入口就是普通 Rust `main`，语句才是正确形状。若在
/// 此定义 `fn main`，被放进另一函数体时会静默变成永不执行的嵌套函数。
///
/// ```ignore
/// fn main() {
///     crate::ios_main!(my_tick); // never returns / 永不返回
/// }
///
/// fn my_tick(window: &crate::Window) -> Option<i32> {
///     // Per-frame: poll events + render + interpreter step.
///     // 每帧：poll 事件 + 渲染 + 解释器步进。
///     None // Some(code) exits the app / Some(code) 退出应用
/// }
/// ```
#[cfg(target_os = "ios")]
#[macro_export]
macro_rules! ios_main {
    ($tick:path) => {
        $crate::ios_start($tick)
    };
}

/// Wait for the next frame — the per-frame yield primitive of the poll
/// model. Call it unconditionally at the end of the application loop body.
///
/// 等待下一帧——poll 模型的**逐帧让出原语**，在应用循环体末尾无条件调用。
///
/// - **Desktop / Android**: no-op async. The frame cadence is naturally
///   bounded by vsync'd present (or `Clock`-style throttling).
///   桌面/Android：no-op async——帧节奏由 vsync present（或 Clock 类节流）
///   自然承担。
/// - **Web**: the `requestAnimationFrame` promise bridge. The browser
///   requires the loop to yield every frame — a tight `loop {}` without
///   this call freezes the page's renderer. **This primitive is what makes
///   the poll model complete on the web**; without it a consumer would have
///   to hand-write the wasm-bindgen rAF bridge (non-trivial, easy to get
///   wrong).
///   Web：`requestAnimationFrame` 的 Promise 桥。浏览器强制循环逐帧让出
///   ——没有本调用，紧密 `loop {}` 会冻结页面渲染进程。**本原语是 poll
///   模型在 Web 上成立的一半**；没有它，消费者就得手写 wasm-bindgen 的
///   rAF 桥（非平凡且易错）。
///
/// ```ignore
/// let mut window = Window::new(...)?;
/// loop {
///     while let Some(event) = window.poll_event() { /* ... */ }
///     // draw ...
///     crate::next_frame().await;   // ← unconditional, all platforms
/// }
/// ```
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub async fn next_frame() {}

/// Web implementation: the rAF promise bridge（Web 实现 = rAF 的 Promise
/// 桥；语义与注意事项见上方桌面孪生分支的文档）。
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub async fn next_frame() {
    let promise = js_sys::Promise::new(
        &mut |resolve: js_sys::Function, _reject: js_sys::Function| {
            web_sys::window()
                .expect("web: no global window / 无全局 window")
                .request_animation_frame(&resolve)
                .expect("requestAnimationFrame failed / rAF 调用失败");
        },
    );
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}