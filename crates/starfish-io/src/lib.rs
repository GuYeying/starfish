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
//!
//! **模块布局（2026-11-03 分平台拆分）**：统一接口在本文件导出；平台
//! 实现按 cfg 路由——`native.rs`（std::fs）/ `web.rs`（fetch）二选一
//! 装载为 `mod imp`；Android 资产原语独立 `android.rs`（`pub use` 导出）。

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

// ── 平台实现路由（统一 `mod imp` 面——lib.rs 公共 API 只对 imp 说话）──

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod imp;

#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod imp;

// ── Android：APK 资产展开后端（独立模块，pub use 统一导出）────────
// AssetManager 句柄由 starfish-we 的 android_init **移交**至此（窗口
// crate 只做句柄移交，不承载资源读取）；零解密——加密层在
// starfish-pygame::sfx。

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{attach as android_attach, expand as android_expand, read_asset as android_read_asset};

// ── 公开 API（统一异步；原生阻塞实现 + Web 真异步）──────────────────

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

/// 读取全部字节（raw）
///
/// - 原生：std::fs::read（path 为文件路径，相对基准目录/CWD 或绝对）
/// - Web：`fetch(path)` GET（path 为 URL，相对页面地址或绝对）
///
// ── 纯裸读（**零加密语义**——加密红线定约 2026-11-03：SFXE1/密钥/
// 哈希/解密全归 starfish-pygame 资源面；io 只做字节搬运）───────────

/// 异步读（raw）：native = 基准目录 fs；wasm = fetch（no-store）。
/// 返回原始字节——调用方自行判封装（加密层在 starfish-pygame::sfx）。
pub async fn read(path: &str) -> Result<Vec<u8>, IoError> {
    imp::read(path).await
}

/// 同步读（raw）：native = 基准目录 fs 直读；wasm = 无同步文件系统，
/// 恒 Err（v1 定约：Web 资源走异步 read）。
#[cfg(not(target_arch = "wasm32"))]
pub fn read_sync(path: &str) -> Result<Vec<u8>, IoError> {
    std::fs::read(resolve(path)).map_err(|e| {
        IoError::Backend(format!("read {path}: {e}"))
    })
}

#[cfg(target_arch = "wasm32")]
pub fn read_sync(path: &str) -> Result<Vec<u8>, IoError> {
    let _ = path;
    Err(IoError::Backend(
        "Web 端无同步 io——用 await pygame.io.fetch(path) + load_from_bytes(bytes)".into(),
    ))
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
