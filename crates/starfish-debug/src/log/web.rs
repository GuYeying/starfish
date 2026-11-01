//! wasm 出口：console.log（退化/直出）+ fetch POST（远程批量上行——
//! 失败静默：诊断通道不得影响运行）。

/// 控制台直出（console.log——wasm 上 println! 无处可去）
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

pub fn console(line: &str) {
    log(line);
}

/// 远程上行（fire-and-forget POST text/plain——Promise 即弃，请求照发）
pub fn post(endpoint: &str, body: &str) {
    let Some(window) = web_sys::window() else { return };
    let opts = web_sys::RequestInit::new();
    opts.set_method("POST");
    opts.set_body(&wasm_bindgen::JsValue::from_str(body));
    let Ok(req) = web_sys::Request::new_with_str_and_init(endpoint, &opts) else { return };
    let _ = req.headers().set("Content-Type", "text/plain; charset=utf-8");
    let _ = window.fetch_with_request(&req);   // Promise 即弃 = 后台发送
}
