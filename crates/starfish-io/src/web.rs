//! Web 实现（wasm32）：fetch 直实现（GET 读 / POST 保存；`path` 即 URL，
//! 相对 URL 相对页面地址）——由 lib.rs 按 `#[cfg(wasm32)] #[path]` 路由
//! 为 `mod imp`。

use super::IoError;
use std::path::Path;
use std::sync::Mutex;
use wasm_bindgen::JsCast;

async fn fetch_bytes(
    path: &str,
    method: &str,
    body: Option<Vec<u8>>,
) -> Result<Vec<u8>, IoError> {
    let window = web_sys::window().ok_or(IoError::Backend("无 window".into()))?;

    let mut init = web_sys::RequestInit::new();
    init.method(method);
    // 读恒 bypass 缓存（游戏载荷更新即取新——无缓存头的静态服务下
    // 浏览器启发式缓存会拿旧件，实测坑 2026-11-03）。无类型 Reflect
    // 设置（免 web-sys RequestCache feature）
    js_sys::Reflect::set(
        init.as_ref(),
        &"cache".into(),
        &wasm_bindgen::JsValue::from_str("no-store"),
    )
    .ok();
    if let Some(data) = body {
        let arr = js_sys::Uint8Array::from(data.as_slice());
        init.body(Some(arr.as_ref()));
        let headers = js_sys::Object::new();
        js_sys::Reflect::set(&headers, &"Content-Type".into(), &"application/octet-stream".into())
            .ok();
        init.headers(&headers);
    }
    let req = web_sys::Request::new_with_str_and_init(path, &init)
        .map_err(|e| IoError::Backend(format!("Request 构造失败: {e:?}")))?;

    let resp_val = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| IoError::Backend(format!("fetch 失败: {e:?}")))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| IoError::Backend("Response 类型异常".into()))?;

    if !resp.ok() {
        return Err(IoError::Backend(format!("HTTP {}", resp.status())));
    }
    let buf_val = wasm_bindgen_futures::JsFuture::from(resp.array_buffer().map_err(
        |e| IoError::Backend(format!("array_buffer: {e:?}")),
    )?)
    .await
    .map_err(|e| IoError::Backend(format!("读取响应体失败: {e:?}")))?;
    Ok(js_sys::Uint8Array::new(&buf_val).to_vec())
}

/// 相对 URL 的基准前缀：与原生 BASE_DIR 对偶（如 `"assets/v3"` 把
/// `read("a.bin")` 指到 `页面origin/assets/v3/a.bin`；也可给跨源 CDN
/// 完整 URL）。未设置 = 浏览器相对语义（相对页面地址）。
static BASE_URL: Mutex<Option<String>> = Mutex::new(None);

pub(super) fn set_base_dir(dir: &Path) {
    *BASE_URL.lock().unwrap() = Some(dir.to_string_lossy().into_owned());
}

/// 相对 URL → 基准前缀拼接；绝对 URL / origin 绝对路径原样
/// （对齐原生"绝对路径不受影响"：`/` 开头 = origin 绝对，视为绝对）
fn resolve_url(path: &str) -> String {
    if path.starts_with('/')
        || path.starts_with("http://")
        || path.starts_with("https://")
        || path.starts_with("//")
    {
        return path.to_string();
    }
    match BASE_URL.lock().unwrap().as_deref() {
        Some(base) => format!("{}/{}", base.trim_end_matches('/'), path),
        None => path.to_string(),
    }
}

pub(super) async fn read(path: &str) -> Result<Vec<u8>, IoError> {
    fetch_bytes(&resolve_url(path), "GET", None).await
}

pub(super) async fn write(path: &str, data: Vec<u8>) -> Result<(), IoError> {
    fetch_bytes(&resolve_url(path), "POST", Some(data))
        .await
        .map(|_| ())
}

pub(super) async fn exists(path: &str) -> Result<bool, IoError> {
    let url = resolve_url(path);
    let window = web_sys::window().ok_or(IoError::Backend("无 window".into()))?;
    let mut init = web_sys::RequestInit::new();
    init.method("HEAD");
    js_sys::Reflect::set(
        init.as_ref(),
        &"cache".into(),
        &wasm_bindgen::JsValue::from_str("no-store"),
    )
    .ok();
    let req = web_sys::Request::new_with_str_and_init(&url, &init)
        .map_err(|e| IoError::Backend(format!("Request 构造失败: {e:?}")))?;
    let resp_val = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&req))
        .await
        .map_err(|e| IoError::Backend(format!("fetch 失败: {e:?}")))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| IoError::Backend("Response 类型异常".into()))?;
    Ok(resp.ok())
}
