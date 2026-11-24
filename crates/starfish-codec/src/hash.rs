//! sha256 摘要（sha2 crate 直通——存档校验/防篡改）。

use sha2::{Digest, Sha256};

/// 字节摘要 → 32B
pub fn sha256_bytes(data: &[u8]) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().to_vec()
}

/// 字节摘要 → 64 字符小写 hex 文本
pub fn sha256_hex(data: &[u8]) -> String {
    sha256_bytes(data).iter().map(|b| format!("{b:02x}")).collect()
}
