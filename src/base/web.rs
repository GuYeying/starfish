//! Web 辅助设施（跨平台封装：桌面目标下为无害直通实现）
//!
//! 平台差异只允许出现在本模块与 [`app`](super::app) 的调度分支——
//! 应用代码经由这里的封装，**零 cfg**。

/// 跨平台日志：wasm → 浏览器控制台（console.log）；桌面 → stdout。
///
/// wasm 没有 stdout/stderr，`println!` 在浏览器无处可去——Web 应用的
/// 诊断输出一律走本函数。
pub fn console_log(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::prelude::wasm_bindgen;
        #[wasm_bindgen]
        extern "C" {
            #[wasm_bindgen(js_namespace = console)]
            fn log(s: &str);
        }
        log(msg);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        println!("{msg}");
    }
}

/// 安装 panic 转发：wasm 上把 panic 信息（含位置）经
/// `console_error_panic_hook` 落到浏览器控制台；桌面 panic 本就走
/// stderr，无需处理。
///
/// 由 [`web_entry!`](crate::web_entry) 宏自动调用，一般无需手动使用。
#[cfg(target_arch = "wasm32")]
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
}
