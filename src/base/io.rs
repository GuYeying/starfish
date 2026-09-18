//! base/io —— 跨平台文件/数据读取与保存（统一异步 API，平台行为一致化）
//!
//! 设计定稿（用户决策，2026-09-18 重启旧 iofi 场景）：**一套通用 API +
//! 平台各自直调目标 API**——
//! - 原生（win/linux/mac + 移动）：std::fs 直实现（阻塞，包在 async 体外壳内）
//! - Web（wasm32）：**fetch** 直实现（GET 读 / POST 保存；`path` 即 URL，
//!   相对 URL 相对页面地址）——浏览器无文件系统，保存语义 = 把字节
//!   POST 到目标端点，由服务端落盘
//!
//! 与旧 iofi（批次 10 建、批次 14 撤）的差异：当时 Web 端无实现（纯 std
//! 透传包装，价值归零故撤）；现在 Web 有了 fetch 真实现，模块价值回归——
//! 一套业务代码读写数据，三端各自直调平台 API。
//!
//! 统一异步：原生为阻塞实现 + async 体外壳（"模拟异步"，await 处即阻塞处，
//! 语义与同步等价）；Web 为真异步（fetch 挂起不挡帧）。调用方统一
//! `starfish::base::io::read(...).await`。
//!
//! 平台能力矩阵：
//! - read / read_text / write / write_text / exists：三端全支持
//! - 其余文件管理操作（list_dir / create_dir / 删除等）：v1 不设——
//!   Web fetch 无对应语义；原生用户直接用 std::fs（能力更完整）
//!
//! Web 端契约：`write` 以 **POST**（body = 原始字节）发往 `path`，
//! 服务端需接受该端点的写入；非 2xx 状态 → [`IoError::Backend`]。

/// IO 错误（不可用能力显式报错，不做静默降级）
#[derive(Debug)]
pub enum IoError {
    /// 当前平台不支持该操作
    UnsupportedPlatform,
    /// 平台实现错误（含原生 IO 错误 / Web HTTP 状态等详情）
    Backend(String),
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::UnsupportedPlatform => write!(f, "io: 该平台不支持此操作"),
            IoError::Backend(s) => write!(f, "io 后端错误: {s}"),
        }
    }
}
impl std::error::Error for IoError {}

// ── 原生：std::fs 直实现（桌面 + 移动沙箱）─────────────────────────

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::IoError;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// 相对路径的基准目录：Android/iOS 沙箱 = 应用私有目录（引擎入口注入）；
    /// 未设置 = CWD 相对（桌面默认）。绝对路径不受影响。
    static BASE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

    pub fn set_base_dir(dir: &Path) {
        *BASE_DIR.lock().unwrap() = Some(dir.to_path_buf());
    }

    /// 相对路径 → 基准目录拼接；绝对路径原样
    pub fn resolve(path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(base) = BASE_DIR.lock().unwrap().as_ref() {
            base.join(p)
        } else {
            p.to_path_buf()
        }
    }

    pub(super) async fn read(path: &str) -> Result<Vec<u8>, IoError> {
        std::fs::read(resolve(path)).map_err(|e| IoError::Backend(format!("read {path}: {e}")))
    }

    pub(super) async fn write(path: &str, data: Vec<u8>) -> Result<(), IoError> {
        std::fs::write(resolve(path), data).map_err(|e| IoError::Backend(format!("write {path}: {e}")))
    }

    pub(super) async fn exists(path: &str) -> Result<bool, IoError> {
        Ok(resolve(path).exists())
    }
}

// ── Web：fetch 直实现（GET 读 / POST 保存）─────────────────────────

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::IoError;
    use wasm_bindgen::JsCast;

    async fn fetch_bytes(
        path: &str,
        method: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Vec<u8>, IoError> {
        let window = web_sys::window().ok_or(IoError::Backend("无 window".into()))?;

        let mut init = web_sys::RequestInit::new();
        init.method(method);
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

    pub(super) async fn read(path: &str) -> Result<Vec<u8>, IoError> {
        fetch_bytes(path, "GET", None).await
    }

    pub(super) async fn write(path: &str, data: Vec<u8>) -> Result<(), IoError> {
        fetch_bytes(path, "POST", Some(data)).await.map(|_| ())
    }

    pub(super) async fn exists(path: &str) -> Result<bool, IoError> {
        let window = web_sys::window().ok_or(IoError::Backend("无 window".into()))?;
        let mut init = web_sys::RequestInit::new();
        init.method("HEAD");
        let req = web_sys::Request::new_with_str_and_init(path, &init)
            .map_err(|e| IoError::Backend(format!("Request 构造失败: {e:?}")))?;
        let resp_val = wasm_bindgen_futures::JsFuture::from(window.fetch_with_request(&req))
            .await
            .map_err(|e| IoError::Backend(format!("fetch 失败: {e:?}")))?;
        let resp: web_sys::Response = resp_val
            .dyn_into()
            .map_err(|_| IoError::Backend("Response 类型异常".into()))?;
        Ok(resp.ok())
    }
}

// ── 公开 API（统一异步；原生阻塞实现 + Web 真异步）──────────────────

/// 设置相对路径的基准目录（仅原生平台）
///
/// Android/iOS 沙箱场景：在 `android_main` 注入 `internal_data_path()`，
/// 此后 `read`/`write` 的相对路径自动落到应用私有目录内——调用方无需
/// 关心沙箱绝对路径。绝对路径不受影响；未设置时 = CWD 相对。
#[cfg(not(target_arch = "wasm32"))]
pub fn set_base_dir(dir: impl AsRef<std::path::Path>) {
    imp::set_base_dir(dir.as_ref())
}

/// 读取全部字节
///
/// - 原生：std::fs::read（path 为文件路径，相对基准目录/CWD 或绝对）
/// - Web：`fetch(path)` GET（path 为 URL，相对页面地址或绝对）
pub async fn read(path: &str) -> Result<Vec<u8>, IoError> {
    imp::read(path).await
}

/// 写入全部字节（覆盖）
///
/// - 原生：std::fs::write（覆盖写，自动创建文件）
/// - Web：`fetch(path, POST)`（body = 原始字节；服务端负责落盘）
pub async fn write(path: &str, data: impl Into<Vec<u8>>) -> Result<(), IoError> {
    imp::write(path, data.into()).await
}

/// 目标是否存在
///
/// - 原生：路径存在性（文件或目录）
/// - Web：`fetch(path, HEAD)` 且响应 2xx
pub async fn exists(path: &str) -> Result<bool, IoError> {
    imp::exists(path).await
}

/// 读取全部内容为 UTF-8 文本
pub async fn read_text(path: &str) -> Result<String, IoError> {
    let bytes = read(path).await?;
    String::from_utf8(bytes)
        .map_err(|e| IoError::Backend(format!("非 UTF-8 内容: {e}")))
}

/// 写入 UTF-8 文本（覆盖）
pub async fn write_text(path: &str, text: impl AsRef<str>) -> Result<(), IoError> {
    write(path, text.as_ref().as_bytes().to_vec()).await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("starfish_io_test_{name}"))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn bytes_round_trip() {
        let path = temp_path("bytes");
        pollster::block_on(write(&path, vec![1u8, 2, 3, 4])).unwrap();
        let read_back = pollster::block_on(read(&path)).unwrap();
        assert_eq!(read_back, vec![1, 2, 3, 4]);
        assert!(pollster::block_on(exists(&path)).unwrap());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn text_round_trip() {
        let path = temp_path("text");
        pollster::block_on(write_text(&path, "hello starfish")).unwrap();
        let s = pollster::block_on(read_text(&path)).unwrap();
        assert_eq!(s, "hello starfish");
        std::fs::remove_file(&path).ok();
    }
}
