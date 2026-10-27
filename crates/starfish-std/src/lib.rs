//! starfish-std：starfish 引擎的**核心共用组件** crate。
//!
//! 收拢跨模块共享的部分（原 `base` 恒编译件）：
//! - [`error`]：引擎统一错误类型（`Error(String)`）
//! - [`debug`]：跨平台诊断输出（[`debug::console_log`]）/ panic 转发 /
//!   线程契约守卫——wasm 上 `println!` 无处可去，诊断一律走 `console_log`
//! - [`app`]：平台驱动器垫片（桌面 `block_on` / Web `spawn_local` 再导出，
//!   [`app_entry!`] 宏的基础设施）
//!
//! 以及三平台统一的入口宏 [`app_entry!`]：运行循环归调用方（SDL 风格
//! poll 模型），驱动器内化——桌面 `block_on`+exit / Web
//! `#[wasm_bindgen(start)]`+`spawn_local`+panic hook / Android
//! `android_main` 桥接（需启用 `we` 特性）。

pub mod app;
pub mod debug;
pub mod error;
pub mod run_control;

/// （可选，feature `we`）starfish-we 窗口/事件 crate 再导出——
/// [`app_entry!`] 的 Android 分支经 `$crate::we` 取 `AndroidApp`/`android_init`。
#[cfg(feature = "we")]
pub use starfish_we as we;

/// 平台入口宏：统一 async 应用体，三平台驱动器内化。
///
/// # 形态（二选一，按首个片段自动分派）
///
/// - **块形态** `app_entry!({ 语句... })`：语句块，无错误出口——
///   初始化失败用 `expect`/panic（Web 经 panic hook 落控制台）。
/// - **表达式形态** `app_entry!(app_body())`：async 表达式，其
///   `Output = Result<(), E>`（`E: Display`）——退出错误由驱动器统一
///   [`console_log`](debug::console_log) 后干净退出。
///
/// # 展开结果（按平台）
///
/// - **桌面**：`fn main` → [`app::block_on`] 阻塞驱动 →
///   `process::exit(0)` 收尾（音频线程不 join，走进程清理）。
/// - **Android**：`android_main`（NativeActivity dlsym 此符号，缺失即
///   启动闪退）→ `android_init` 装配系统回调 → **复用同一 `fn main`**
///   （需启用 `we` 特性：宏经 `$crate::we` 取宿主装配）。
/// - **Web**：`#[wasm_bindgen(start)]` 入口 → panic hook →
///   [`app::spawn_local`] 驱动（页面期常驻）；`fn main` 仅是
///   wasm bin 目标的 rustc 存根，永不被调用。
///
/// # 约束
///
/// - 宏内驱动器全部走 `$crate::` 路径（本 crate 再导出），用户
///   crate 无需依赖 pollster / wasm-bindgen-futures。
/// - **Web 构建要求用户 crate 可见 `wasm-bindgen` 依赖**（`start` 属性
///   路径在用户 crate 解析）。
/// - `block` 规则在前：`{ … }` 同时匹配两种片段，按序块形态优先；
///   `app_body()` 等调用表达式自动落表达式形态。
#[macro_export]
macro_rules! app_entry {
    // ── 块形态：无错误出口，错误走 panic ──────────────────────
    ($body:block) => {
        // 全平台统一 main：桌面是 OS bin 入口；Android 由 android_main
        // 装配后调用（cdylib 内普通函数）；Web 是 bin 存根，永不执行。
        #[cfg(not(target_arch = "wasm32"))]
        fn main() {
            $crate::app::block_on(async move { $body });
            std::process::exit(0);
        }

        // Android：OS dlopen 入口——装配后复用同一 main。
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::we::AndroidApp) {
            $crate::we::android_init(app);
            main();
        }

        // Web：实例化即执行；panic → 浏览器控制台。
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn __starfish_app_entry() -> Result<(), wasm_bindgen::prelude::JsValue> {
            $crate::debug::install_panic_hook();
            $crate::app::spawn_local(async move { $body });
            Ok(())
        }

        // wasm bin 目标的 rustc main 存根（实际入口 = start）。
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        fn main() {}
    };

    // ── 表达式形态：async expr，Output = Result<(), E>（E: Display）──
    ($body:expr) => {
        #[cfg(not(target_arch = "wasm32"))]
        fn main() {
            match $crate::app::block_on($body) {
                Ok(()) => {}
                Err(e) => {
                    $crate::debug::console_log(&format!("[starfish] 退出: {e}"))
                }
            }
            // 收尾：音频线程（cpal Stream）不 join，走进程清理。
            std::process::exit(0);
        }

        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::we::AndroidApp) {
            $crate::we::android_init(app);
            main();
        }

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn __starfish_app_entry() -> Result<(), wasm_bindgen::prelude::JsValue> {
            $crate::debug::install_panic_hook();
            $crate::app::spawn_local(async move {
                match $body.await {
                    Ok(()) => {}
                    Err(e) => {
                        $crate::debug::console_log(&format!("[starfish] 退出: {e}"))
                    }
                }
            });
            Ok(())
        }

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        fn main() {}
    };
}
