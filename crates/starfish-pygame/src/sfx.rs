//! 资源加密面（**starfish 全家唯一加密归属**——加密红线定约
//! 2026-11-03：SFXE1 封装/密钥/逻辑路径哈希/解密只活在本 crate；
//! starfish-io 等其余引擎 crate 零加密语义，只做字节搬运）。
//!
//! 形态：松散加密散文件（哈希名 .sfx）+ 双密文清单（`_payload.sfx`
//! 逻辑路径表 / `_manifest.sfx` Android 展开表）。读取链：
//! 逻辑路径 → 哈希定位（嵌套目录位）→ io 裸读 → 解封装（明文透传，
//! 混合无缝；钥不配显式报错——密文错误不该静默出垃圾数据）。
//!
//! 嵌入态数据（runtime 壳 include 进二进制的入口脚本/壳内密文）经
//! [`decrypt_envelope`] 解出。Android 的清单解密+展开编排也在此——
//! io 只暴露 `android_read_asset`（裸读）/`android_expand`（裸复制）
//! 原语。

use std::path::Path;
use std::sync::OnceLock;

static SFX_KEY: OnceLock<Option<[u8; 32]>> = OnceLock::new();
static SFX_MAGIC: OnceLock<[u8; 5]> = OnceLock::new();

/// 资源加密 init（宿主创建时调用一次；未调 = 无解密，密文资源读出即
/// 原始封装字节）：key = 32B 解密密钥；magic = 项目自定义封装魔数
///（缺省 "SFXE1"——按项目换魔数可让通用扫描器失效，零成本加防护）
pub fn init_crypto(key: [u8; 32], magic: [u8; 5]) {
    let _ = SFX_KEY.set(Some(key));
    let _ = SFX_MAGIC.set(magic);
}

pub fn io_key() -> Option<[u8; 32]> {
    SFX_KEY.get().cloned().flatten()
}

pub fn io_magic() -> [u8; 5] {
    SFX_MAGIC.get().copied().unwrap_or(*b"SFXE1")
}

/// 路径哈希 → .sfx 文件名（与 Python bundle.py sfx_name 同公式：
/// SHA-256(rel_path + key_hex 字符串)[:16 字节 hex]——双端确定性一致。
/// 注意追加密钥的 **hex 字符串**（64 字节 ASCII），不是原始 32 字节——
/// 此前误写原始字节，散文件链首次实跑暴露过）
pub fn sfx_name(path: &str, key_hex: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update(key_hex.as_bytes());
    let result = hasher.finalize();
    format!("{}.sfx", result[..16].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// 解密封装（SFXE1 格式：magic5 | nonce12 | 密文+tag16）：
/// AAD = 域常量（完整性与域绑定——**不含路径**：松散文件读路径随
/// 基准目录变化，路径 AAD 会在换目录/换机时假性失败，2026-11-01 实测）；
/// magic 探测不匹配 = Ok(透传原字节)——明文/密文混合无缝；
/// magic 命中但解密失败 = Err（密钥错/被篡改，显式暴露）
pub fn unwrap_envelope(
    raw: &[u8],
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

/// 嵌入态密文解包（runtime 壳 include 进二进制的入口脚本等；明文
/// 透传，密钥未接线时原样返回）
pub fn decrypt_envelope(raw: &[u8]) -> Result<Vec<u8>, String> {
    let Some(key) = io_key() else {
        return Ok(raw.to_vec());
    };
    unwrap_envelope(raw, &key, &io_magic())
}

/// 哈希散文件定位（**目录结构保留**——哈希文件写在其逻辑路径的原目录
/// 位：`modules/<hash>.sfx`、`assets/<hash>.sfx`；基准目录来自 io 层。
/// native-only：wasm 基准是 URL 相对语义（fetch 按原名寻址），无哈希链）
#[cfg(not(target_arch = "wasm32"))]
fn sfx_path(path: &str, key_hex: &str) -> std::path::PathBuf {
    let dir = starfish_io::base_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let rel = Path::new(path);
    match rel.parent() {
        Some(p) if !p.as_os_str().is_empty() => dir.join(p).join(sfx_name(path, key_hex)),
        _ => dir.join(sfx_name(path, key_hex)),
    }
}

/// 哈希定位裸读（native 命中返回 Some(解密字节)；未命中/无钥/wasm =
/// None 走调用方回退）
#[cfg(not(target_arch = "wasm32"))]
fn hashed_lookup(path: &str) -> Option<Vec<u8>> {
    let key = io_key()?;
    let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let sp = sfx_path(path, &key_hex);
    if !sp.is_file() {
        return None;
    }
    let raw = std::fs::read(&sp).ok()?;
    unwrap_envelope(&raw, &key, &io_magic()).ok()
}

/// 路径恒相对归一（**限制 + base_dir 模型定约** 2026-11-09）：sfx 面
/// 的任何输入都按 base_dir 相对路径处理——剥离 Windows 盘符、前导分隔
/// 符，反斜杠归一；绝对语义不生效（原始 io 层不受影响，保持自由形态）。
/// 例外：`http(s)://` 完整 URL 原样透传（跨源 CDN 资产，base_dir 前缀
/// 对其无意义）。
pub(crate) fn normalize_relative(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    let mut s = path.replace('\\', "/");
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        s = s[2..].to_string(); // 剥盘符（"X:"）
    }
    s.trim_start_matches('/').to_string()
}

/// 资源同步读（image.load / mixer / font 内部调用——**全链路同步**）：
/// 逻辑路径 → 哈希定位 → 解封装（未命中 → 路径裸读，明文透传）。
/// wasm 无同步文件系统恒 Err（v1 定约：Web 走异步 read）。
pub fn read_sync(path: &str) -> Result<Vec<u8>, String> {
    let path = &normalize_relative(path);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(b) = hashed_lookup(path) {
        return Ok(b);
    }
    let raw = starfish_io::read_sync(path).map_err(|e| e.to_string())?;
    let Some(key) = io_key() else {
        return Ok(raw);
    };
    unwrap_envelope(&raw, &key, &io_magic())
}

/// 资源异步读（pygame.io.fetch 底层——三平台同语义）：native 先哈希
/// 定位（嵌套散文件）；未命中/无钥 → io 裸读 → 解封装。wasm = fetch
///（no-store，按原名寻址）→ 解封装。
pub async fn read(path: &str) -> Result<Vec<u8>, String> {
    let path = &normalize_relative(path);
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(b) = hashed_lookup(path) {
        return Ok(b);
    }
    let raw = starfish_io::read(path).await.map_err(|e| e.to_string())?;
    let Some(key) = io_key() else {
        return Ok(raw);
    };
    unwrap_envelope(&raw, &key, &io_magic())
}

/// 阻塞式资源读（**绑定层同步路径装载专用口——平台分流唯一事实源**，
/// spine/ozz/live2d 路径装载器走此口）：native = block_on(异步 read)
///（哈希定位→io→解封装全链同 async）；wasm = 同步 XHR（web 无阻塞
/// 模型——异步 executor 在同步 Rust 栈内不可能驱动真 fetch，block_on
/// 系于 wasm 必然 Condvar::wait panic，三端测试 2026-11-07 实测定约；
/// 主线程一次性装载期阻塞可接受）。
pub fn read_block(path: &str) -> Result<Vec<u8>, String> {
    let path = &normalize_relative(path);
    #[cfg(target_arch = "wasm32")]
    {
        read_sync(path)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        starfish_std::app::block_on(read(path))
    }
}

/// Android 资产展开编排（**清单解密在本层**——加密红线）：读
/// `_manifest.sfx`（APK assets 根，固定名）→ 解包 → io 原始展开原语
/// （裸复制到 data_dir 根——展开后目录镜像源项目结构，与二进制同级）。
/// 返回展开文件数（0 = 未接线/解密失败/增量完成）。
#[cfg(target_os = "android")]
pub fn android_expand_assets() -> usize {
    let Some(m) = starfish_io::android_read_asset("_manifest.sfx") else {
        return 0;
    };
    let Some(key) = io_key() else {
        return 0; // 密钥未接线（壳层 init_crypto 顺序错误的信号）
    };
    // 解密失败 = 密文被篡改或钥不配——静默零展开（无展开目录 = 后续
    // 读显式报错，不半展开）
    let Ok(plain) = unwrap_envelope(&m, &key, &io_magic()) else {
        return 0;
    };
    let names: Vec<String> = String::from_utf8_lossy(&plain)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    starfish_io::android_expand(&names)
}
