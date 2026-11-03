//! 原生实现（桌面 + 移动沙箱）：std::fs 直实现（阻塞，包在 async 体外
//! 壳内——由 lib.rs 按 `#[cfg(not(wasm32))] #[path]` 路由为 `mod imp`）。

use super::IoError;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 相对路径的基准目录：Android/iOS 沙箱 = 应用私有目录（引擎入口注入）；
/// 未设置 = CWD 相对（桌面默认）。绝对路径不受影响。
static BASE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

pub(super) fn set_base_dir(dir: &Path) {
    *BASE_DIR.lock().unwrap() = Some(dir.to_path_buf());
}

/// 相对路径 → 基准目录拼接；绝对路径原样
pub(super) fn resolve(path: &str) -> PathBuf {
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
pub(super) fn clear_base_dir() {
    *BASE_DIR.lock().unwrap() = None;
}

pub(super) fn base_dir() -> Option<PathBuf> {
    BASE_DIR.lock().unwrap().clone()
}
