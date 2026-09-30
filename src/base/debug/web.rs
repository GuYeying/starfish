//! debug 的 wasm 实现：console.log 输出 / panic 落浏览器控制台

/// console.log 直写（浏览器控制台）
pub fn log(msg: &str) {
    use wasm_bindgen::prelude::wasm_bindgen;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: &str);
    }
    log(msg);
}

/// panic 信息（含位置）转发到浏览器控制台
///
/// 由 [`app_entry!`](crate::app_entry)
/// 宏自动调用，一般无需手动使用。
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
}
