//! Android：APK 资产展开后端（NativeActivity 自管理形态）——由 lib.rs
//! 按 `#[cfg(target_os = "android")] mod android` 装载，公共口经 lib.rs
//! `pub use` 导出。
//!
//! AssetManager 句柄由 starfish-we 的 android_init **移交**至此（窗口
//! crate 只做句柄移交）；**零解密**——加密层在 starfish-pygame::sfx
//! （加密红线 2026-11-03），清单密文由调用方读出后自行解包。

use std::sync::Mutex;

static ANDROID_ASSETS: Mutex<Option<(ndk::asset::AssetManager, String)>> =
    Mutex::new(None);

/// 句柄移交口（starfish-we android_init 调用；app 私有数据目录 = 展开
/// 目标基准）。一次性语义——重复 attach 后到者忽略。
pub fn attach(am: ndk::asset::AssetManager, data_dir: String) {
    if let Ok(mut slot) = ANDROID_ASSETS.lock() {
        if slot.is_none() {
            *slot = Some((am, data_dir));
        }
    }
}

/// APK 原始资产读（**零解密**——加密层在 starfish-pygame::sfx；清单
/// 密文由调用方读出后自行解包）。rel 相对 APK assets/ 根。
pub fn read_asset(rel: &str) -> Option<Vec<u8>> {
    use std::ffi::CString;
    use std::io::Read;
    let guard = ANDROID_ASSETS.lock().ok()?;
    let (am, _data_dir) = guard.as_ref()?;
    let c = CString::new(rel).ok()?;
    let mut asset = am.open(&c)?;
    let mut buf = Vec::new();
    asset.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// APK 原始展开（**零解密**——调用方传入**已解密**的物理文件名表）：
/// 逐名经 AssetManager 读出写入 `data_dir/<name>`（已存在跳过 = 增量，
/// 名含子目录则建目录）→ **set_base_dir 指向 data_dir**——展开后目录
/// 镜像源项目结构（assets/ modules/ 与二进制同级，2026-11-03 定约）。
/// 清单的读取/解密在 starfish-pygame::sfx::android_expand_assets。
/// 返回本次展开文件数。
pub fn expand(names: &[String]) -> usize {
    use std::ffi::CString;
    use std::io::Read;

    // AssetManager 不实现 Clone——全部工作在锁作用域内完成，返回值
    // 只带出计数（带不出句柄借用）
    let guard = match ANDROID_ASSETS.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    let Some((am, data_dir)) = guard.as_ref() else {
        return 0;
    };
    let base = data_dir.clone();
    let mut expanded = 0usize;
    let root = std::path::Path::new(&base);
    for rel in names {
        let rel = rel.trim();
        if rel.is_empty() {
            continue;
        }
        let dst = root.join(rel);
        if dst.is_file() {
            continue; // 增量：已展开即跳过
        }
        let read = || -> Option<Vec<u8>> {
            let c = CString::new(rel).ok()?;
            let mut asset = am.open(&c)?;
            let mut buf = Vec::new();
            asset.read_to_end(&mut buf).ok()?;
            Some(buf)
        };
        if let Some(bytes) = read() {
            if let Some(p) = dst.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            if std::fs::write(&dst, &bytes).is_ok() {
                expanded += 1;
            }
        }
    }
    super::set_base_dir(&base);
    expanded
}
