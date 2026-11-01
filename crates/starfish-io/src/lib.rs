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

pub mod pack;
pub use pack::Pack;

// ── 内嵌资源包源（runtime 单文件形态——self-blob 解析后注入；创建时
// 一次性 set，io.read 解析链包优先）──
static PACK_SOURCE: std::sync::OnceLock<Option<pack::Pack>> = std::sync::OnceLock::new();

/// runtime 壳注入内嵌资源包（self-blob → Pack::open 后调此口）
pub fn set_pack_source(pack: pack::Pack) {
    let _ = PACK_SOURCE.set(Some(pack));
}

fn pack_lookup(path: &str) -> Option<Vec<u8>> {
    let pack = PACK_SOURCE.get()?.as_ref()?;
    pack.read(path).ok()
}

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

    /// 测试专用：清除基准目录（恢复 CWD 相对）
    #[cfg(test)]
    pub fn clear_base_dir() {
        *BASE_DIR.lock().unwrap() = None;
    }

    pub fn base_dir() -> Option<PathBuf> {
        BASE_DIR.lock().unwrap().clone()
    }
}

/// 当前基准目录（只读视图；与 [`set_base_dir`] 对偶）
///
/// - Android：`run` 自动注入的应用私有目录——资产落盘等需要**绝对路径**
///   的场景由此取根（`base_dir()` + 相对名自行拼接，或直接走相对 io API）
/// - 桌面：用户手动设置则返回之；未设置 = `None`（CWD 相对）
/// - Web：不存在——Web 的"基准"是 URL 前缀（由浏览器相对语义消化），
///   无读取方故不设 getter
#[cfg(not(target_arch = "wasm32"))]
pub fn base_dir() -> Option<std::path::PathBuf> {
    imp::base_dir()
}

/// 相对路径 → 基准目录拼接（绝对路径原样）——**路径规则单一事实源**。
/// 非 Web 专用：不走 [`read`](Self 的异步读) 的同步资源装载场景
/// （如纹理解码装载）由此取得与 io 一致的路径语义。
#[cfg(not(target_arch = "wasm32"))]
pub fn resolve(path: &str) -> std::path::PathBuf {
    imp::resolve(path)
}

// ── Web：fetch 直实现（GET 读 / POST 保存）─────────────────────────

#[cfg(target_arch = "wasm32")]
mod imp {
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

    pub fn set_base_dir(dir: &Path) {
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
}

// ── 公开 API（统一异步；原生阻塞实现 + Web 真异步）──────────────────

/// 设置相对路径的基准（全平台；覆盖写，可重复调用）
///
/// - 原生：`dir` 为目录，相对路径 → `dir.join(path)`。Android 由引擎 `run`
///   自动注入应用私有目录（internal_data_path）；桌面默认 CWD 相对，可手动
///   指定（如把存档目录与启动位置解耦）。绝对路径不受影响。
/// - Web：`dir` 为 **URL 前缀**，相对路径 → `{dir}/{path}`（如 `"assets/v3"`
///   把 `read("a.bin")` 指到 `页面origin/assets/v3/a.bin`；也可给跨源 CDN
///   完整 URL）。未设置 = 浏览器相对语义（相对页面地址）；`/` 开头视为
///   origin 绝对路径，不受前缀影响——对齐原生"绝对路径不受影响"。
pub fn set_base_dir(dir: impl AsRef<std::path::Path>) {
    imp::set_base_dir(dir.as_ref())
}

/// 读取全部字节
///
/// - 原生：std::fs::read（path 为文件路径，相对基准目录/CWD 或绝对）
/// - Web：`fetch(path)` GET（path 为 URL，相对页面地址或绝对）
/// - **资源策略**：init_resource 配置了包时 = 包优先查找（runtime 构
///   建），未命中回落路径；wasm fetch 散文件按密钥解封装（加密资源）

// ── 资源加密（创建时一次性——密钥/魔数 init 注入；**松散加密文件**
// 单一形态——单文件包已裁撤：io.read 整文件读语义下包无增益，逐文件
// 加密即防护本体；wasm 多文件 fetch 同格式，2026-11-01 定约）──

static IO_KEY: std::sync::OnceLock<Option<[u8; 32]>> = std::sync::OnceLock::new();
static IO_MAGIC: std::sync::OnceLock<[u8; 5]> = std::sync::OnceLock::new();

/// 资源加密 init（宿主创建时调用一次；未调 = 无解密，密文资源读出即
/// 原始封装字节）：key = 32B 解密密钥；magic = 项目自定义封装魔数
///（缺省 "SFXE1"——按项目换魔数可让通用扫描器失效，零成本加防护）
pub fn init_crypto(key: [u8; 32], magic: [u8; 5]) {
    let _ = IO_KEY.set(Some(key));
    let _ = IO_MAGIC.set(magic);
}

pub fn io_key() -> Option<[u8; 32]> {
    IO_KEY.get().cloned().flatten()
}

pub fn io_magic() -> [u8; 5] {
    IO_MAGIC.get().copied().unwrap_or(*b"SFXE1")
}

/// - **资源加密**：init_crypto 注入密钥/魔数后，读到的加密封装文件
///   自动解密（明文资源透传，混合无缝）；密钥不符 = 显式报错（密文物
///   理错误不该静默出垃圾图）
pub async fn read(path: &str) -> Result<Vec<u8>, IoError> {
    if let Some(bytes) = pack_lookup(path) {
        return Ok(bytes);
    }
    let raw = imp::read(path).await?;
    if let Some(key) = io_key() {
        return unwrap_envelope(&raw, path, &key, &io_magic())
            .map_err(IoError::Backend);
    }
    Ok(raw)
}

/// 解密封装（SFXE1 格式：magic5 | nonce12 | 密文+tag16）：
/// AAD = 域常量（完整性与域绑定——**不含路径**：松散文件读路径随
/// 基准目录变化，路径 AAD 会在换目录/换机时假性失败，2026-11-01 实测）；
/// magic 探测不匹配 = Ok(透传原字节)——明文/密文混合无缝；
/// magic 命中但解密失败 = Err（密钥错/被篡改，显式暴露）
pub(crate) fn unwrap_envelope(
    raw: &[u8],
    _aad_path: &str,
    key: &[u8; 32],
    magic: &[u8; 5],
) -> Result<Vec<u8>, String> {
    use chacha20poly1305::{
        aead::{Aead, KeyInit, Payload},
        ChaCha20Poly1305, Key, Nonce,
    };
    const DOMAIN_AAD: &[u8] = b"starfish-asset-v1";
    if raw.len() < 33 || &raw[..5] != magic {
        return Ok(raw.to_vec()); // 无封装 = 透传（明文资源）
    }
    let nonce = Nonce::from_slice(&raw[5..17]); // IETF 12B
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &raw[17..],
                aad: DOMAIN_AAD,
            },
        )
        .map_err(|e| format!("资源解密失败（密钥不匹配或已篡改）: {e:?}"))
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

    #[test]
    fn base_dir_redirects_relative_paths() {
        let base = std::env::temp_dir().join("starfish_io_test_base");
        std::fs::create_dir_all(&base).unwrap();
        set_base_dir(&base);

        // 相对路径被重定向进基准目录
        pollster::block_on(write("rel_redirect.bin", vec![9u8, 8, 7])).unwrap();
        let redirected = base.join("rel_redirect.bin");
        assert!(redirected.is_file(), "相对路径应落在基准目录内");
        assert_eq!(
            pollster::block_on(read("rel_redirect.bin")).unwrap(),
            vec![9u8, 8, 7]
        );

        // 绝对路径不受基准影响
        let abs = temp_path("abs_untouched");
        pollster::block_on(write(&abs, vec![1u8])).unwrap();
        assert!(std::path::Path::new(&abs).is_file());

        imp::clear_base_dir();
        std::fs::remove_file(&redirected).ok();
        std::fs::remove_file(&abs).ok();
        std::fs::remove_dir(&base).ok();
    }
}
