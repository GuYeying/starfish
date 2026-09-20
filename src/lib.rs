
pub mod base;
pub mod pygame;

/// 统一入口宏：**一个调用点覆盖全平台**（桌面 / iOS / Android / Web），
/// 应用代码从第一行到最后一行零 `#[cfg]`。
///
/// 平台入口对仗结构（**三平台同名 `run`**）：
///
/// | 平台 | OS 入口 | 实际执行的引导 |
/// |---|---|---|
/// | 桌面（win/linux/mac）/ iOS | `main` | [`run`](base::app::run) → `EventLoop::new` |
/// | Android | `android_main` | 捕获 `AndroidApp` 到全局槽 → `main` → [`run`](base::app::run)（从槽取句柄，android-activity 引导 + 私有目录注入） |
/// | Web（wasm32-unknown-unknown） | `wasm_bindgen(start)` | `main` → [`run`](base::app::run)（spawn_local 调度） |
///
/// Android 分支的 `AndroidApp` 经 [`base::app`](base::app) 再导出并捕获进
/// 全局槽，用户 crate 无需依赖 winit；`run` 在 Android 上自动完成私有目录
/// 注入（io 相对路径 / 资产落盘的根），应用侧零手工注入。
///
/// # 用法
///
/// ```ignore
/// use starfish::base::app::{Application, Ctx, WindowConfig};
///
/// struct App;
/// impl Application for App { /* … */ }
///
/// starfish::app_entry!(App, WindowConfig::new("demo", 800, 600).with_fps_cap(60));
/// ```
///
/// # 约束
///
/// 两个参数必须是**纯构造表达式**（禁 `?`、语句、早退）——宏会按平台展开
/// 到一或两个位置，副作用每平台只应发生一次。Web 构建要求用户 crate 的
/// dev-dependencies 含 `wasm-bindgen`（宏展开的属性路径在用户 crate
/// 解析；桌面/Android 构建无此要求）。
#[macro_export]
macro_rules! app_entry {
    ($app:expr, $config:expr $(,)?) => {
        // Android：OS dlopen 入口——捕获系统递入的 AndroidApp 后，走与桌面
        // 完全相同的 main → run（句柄经全局槽传递，见 base::app）。
        #[cfg(target_os = "android")]
        #[unsafe(no_mangle)]
        fn android_main(app: $crate::base::app::AndroidApp) {
            $crate::base::app::set_android_app(app);
            main();
        }

        // 全平台统一的 main：桌面/iOS 是真 bin 入口；Android 由 android_main
        // 调用（cdylib 内普通函数）；Web 是 bin 目标的编译占位（浏览器实际
        // 走下方 start 入口，main 永不被调用）。
        fn main() {
            $crate::base::app::run($app, $config);
        }

        // Web：实例化后自动执行；panic → 浏览器控制台转发。
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn __starfish_app_entry() -> Result<(), wasm_bindgen::prelude::JsValue> {
            $crate::base::debug::install_panic_hook();
            main();
            Ok(())
        }
    };
}
