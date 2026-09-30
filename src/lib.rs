//! starfish（rewrite 版）：SDL 风格 pull 运行路线的引擎本体。
//!
//! 运行周期归调用方所有：桌面 `main` 里普通 while 循环 + `poll_event`；
//! Web 由调用方的 rAF 链驱动；无回调控制反转、无 control-flow 异常。
//! 窗口/事件层 = `starfish-window`（再导出垫片见 [`base::window`]）。

pub mod base;

/// pygame 兼容层（契约：`architecture/pygame层设计.md`）。
/// 依赖单向：本层可直用 base 类型；base 永不感知本层。
pub mod pygame;

/// 平台入口宏：统一 async 应用体，三平台驱动器内化。
///
/// # 形态（二选一，按首个片段自动分派）
///
/// - **块形态** `app_entry!({ 语句... })`：语句块，无错误出口——
///   初始化失败用 `expect`/panic（Web 经 panic hook 落控制台）。
/// - **表达式形态** `app_entry!(app_body())`：async 表达式，其
///   `Output = Result<(), E>`（`E: Display`）——退出错误由驱动器统一
///   [`console_log`](base::debug::console_log) 后干净退出。
///
/// # 展开结果（按平台）
///
/// - **桌面**：`fn main` → [`base::app::block_on`] 阻塞驱动 →
///   `process::exit(0)` 收尾（音频线程不 join，走进程清理）。
/// - **Android**：`android_main`（NativeActivity dlsym 此符号，缺失即
///   启动闪退）→ `android_init` 装配系统回调 → **复用同一 `fn main`**
///   （与桌面同一条驱动路径；桥接形态同旧引擎 `set_android_app` 方案，
///   但无需全局句柄槽——`android_init` 本身就是全局装配）。
/// - **Web**：`#[wasm_bindgen(start)]` 入口 → panic hook →
///   [`base::app::spawn_local`] 驱动（页面期常驻）；`fn main` 仅是
///   wasm bin 目标的 rustc 存根，永不被调用。
///
/// # 约束
///
/// - 宏内驱动器全部走 `$crate::` 路径（[`base::app`] 再导出），用户
///   crate 无需依赖 pollster / wasm-bindgen-futures。
/// - **Web 构建要求用户 crate 可见 `wasm-bindgen` 依赖**（`start` 属性
///   路径在用户 crate 解析）；本仓库 examples 由 dev-dependencies 提供。
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
            $crate::base::app::block_on(async move { $body });
            std::process::exit(0);
        }

        // Android：OS dlopen 入口——装配后复用同一 main。
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::base::window::AndroidApp) {
            $crate::base::window::android_init(app);
            main();
        }

        // Web：实例化即执行；panic → 浏览器控制台。
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn __starfish_app_entry() -> Result<(), wasm_bindgen::prelude::JsValue> {
            $crate::base::debug::install_panic_hook();
            $crate::base::app::spawn_local(async move { $body });
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
            match $crate::base::app::block_on($body) {
                Ok(()) => {}
                Err(e) => {
                    $crate::base::debug::console_log(&format!("[starfish] 退出: {e}"))
                }
            }
            // 收尾：音频线程（cpal Stream）不 join，走进程清理。
            std::process::exit(0);
        }

        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::base::window::AndroidApp) {
            $crate::base::window::android_init(app);
            main();
        }

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn __starfish_app_entry() -> Result<(), wasm_bindgen::prelude::JsValue> {
            $crate::base::debug::install_panic_hook();
            $crate::base::app::spawn_local(async move {
                match $body.await {
                    Ok(()) => {}
                    Err(e) => {
                        $crate::base::debug::console_log(&format!("[starfish] 退出: {e}"))
                    }
                }
            });
            Ok(())
        }

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        fn main() {}
    };
}
