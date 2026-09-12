
pub mod base;
pub mod pygame;

/// Web 入口宏：生成 `#[wasm_bindgen(start)]` 标注的入口（wasm 实例化后自动
/// 执行你的 `main`，并安装 panic → 浏览器控制台转发）；桌面目标展开为空。
///
/// 放在 `fn main` 之后调用一次即可，**应用代码零 cfg**：
///
/// ```ignore
/// fn main() {
///     starfish::base::app::run(App::default(), WindowConfig::new("demo", 800, 600));
/// }
///
/// starfish::web_entry!();
/// ```
///
/// 要求：Web 构建时用户 crate 的 dev-dependencies 需含 `wasm-bindgen`
/// （宏展开的属性路径在用户 crate 解析；panic 转发由 starfish 内置，无需
/// 额外依赖）。
#[macro_export]
macro_rules! web_entry {
    () => {
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen(start)]
        fn __starfish_web_main() -> Result<(), wasm_bindgen::prelude::JsValue> {
            $crate::base::web::install_panic_hook();
            main();
            Ok(())
        }
    };
}
