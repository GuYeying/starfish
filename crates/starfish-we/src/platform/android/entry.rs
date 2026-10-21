//! Android entry-point glue and the built-in demo host.
//! Android 入口 glue 与内置演示宿主。
//!
//! Hosts integrate with one line via the
//! [`android_main!`](crate::android_main) macro (see the macro's docs);
//! the `android-demo` feature provides the built-in demo host (a logcat
//! event loop). Build the APK with
//! `cargo apk build --features android-demo`.
//! 宿主用 [`android_main!`](crate::android_main) 宏一行接入（见宏文档）；
//! `android-demo` feature 提供内置演示宿主（logcat 事件循环），
//! 打 APK：`cargo apk build --features android-demo`。

/// The Android host entry point (NativeActivity).
/// Android 宿主入口（NativeActivity）。
///
/// Expands to an exported `android_main`: first wires up this crate's
/// callbacks ([`android_init`]), then hands control to `$main(app)`. Hosts
/// integrate with one line at their crate root:
/// 展开为导出的 `android_main`：先装配本库回调（[`android_init`]），
/// 再把控制权交给 `$main(app)`。宿主在 crate 根一行接入：
///
/// ```ignore
/// crate::android_main!(my_app_main);
///
/// fn my_app_main(app: crate::AndroidApp) {
///     // Your main loop: create a window, drive poll_event.
///     // 你的主循环：建窗 + poll_event 驱动
/// }
/// ```
///
/// Note: after using the macro, do NOT hand-write `android_main` (symbol
/// conflict); the host crate must be a cdylib (the APK's load unit).
/// 注意：使用宏后不要再手写 `android_main`（符号冲突）；
/// 宿主 crate 需是 cdylib（APK 的加载单元）。
#[macro_export]
macro_rules! android_main {
    ($main:path) => {
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::AndroidApp) {
            $crate::android_init(app.clone());
            $main(app);
        }
    };
}
