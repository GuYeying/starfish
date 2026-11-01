//! 资源包 SFIX1 读取 + XChaCha20-Poly1305 解密封装层（2026-11-01 定约，
//! 设计全文见 docs/资源包与io分层设计.md——根仓 docs/）。
//!
//! 两个解耦关注点：
//! - **打包**：SFIX1 容器（索引尾置 + AEAD 索引）——native runtime 读
//! - **加密**：SFXE1 资源封装（magic|nonce24|密文+tag，AAD=路径）——
//!   包内条目与 wasm 加密散文件**同格式**（打包与加密解耦的兑现）

use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    ChaCha20Poly1305, Key, Nonce,   // IETF 12B nonce——与 Python cryptography 互操作
};
use std::collections::BTreeMap;

pub(crate) const PACK_MAGIC: &[u8; 5] = b"SFIX1";
pub(crate) const ENV_MAGIC: &[u8; 5] = b"SFXE1";
const HEADER_LEN: usize = 41; // magic5+ver4+flags4+nonce12+off8+len8 // magic5 + ver4 + flags4 + nonce24 + off8 + len8
const INDEX_AAD: &[u8] = b"sf-index";

/// 解密封装：magic 探测 → XChaCha20-Poly1305 open（AAD = 路径）。
/// 无封装（非 SFXE1）= 原样透传（明文资源无缝混合）。
pub(crate) fn unwrap_envelope(raw: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, String> {
    if raw.len() < 17 + 16 || &raw[..5] != ENV_MAGIC {
        return Ok(raw.to_vec()); // 无封装 = 透传
    }
    let nonce = Nonce::from_slice(&raw[5..17]); // IETF 12B
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &raw[17..],
                aad: b"starfish-asset-v1",
            },
        )
        .map_err(|e| format!("解密失败（密钥不匹配或资源被篡改）: {e:?}"))
}

/// 封装（打包器为 Rust 侧备用口；主打包器 = Python scripts/pack_assets.py）
#[cfg(any(test, feature = "pack-write"))]
pub(crate) fn wrap_envelope(plain: &[u8], aad_path: &str, key: &[u8; 32], nonce24: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let ct = cipher.encrypt(
        Nonce::from_slice(nonce24),
        Payload { msg: plain, aad: aad_path.as_bytes() },
    ).expect("XChaCha 加密不应失败");
    let mut out = ENV_MAGIC.to_vec();
    out.extend_from_slice(nonce12);
    out.extend_from_slice(&ct);
    out
}

/// 资源包（SFIX1）只读视图：open 时解析（解密）索引，按路径取封装再解密
#[derive(Clone)]
pub struct Pack {
    map: BTreeMap<String, Entry>,
    key: [u8; 32],
    raw: Vec<u8>, // 整包驻内存（v1——io.read 本就是整文件读语义）
}

#[derive(Clone)]
struct Entry {
    offset: u64,
    len: u64,
}

impl Pack {
    /// 从字节打开（native = fs 读全量；未来 wasm 整包 fetch 同口）
    pub fn open(raw: Vec<u8>, key: &[u8; 32]) -> Result<Self, String> {
        if raw.len() < HEADER_LEN || &raw[..5] != PACK_MAGIC {
            return Err("非 SFIX1 资源包".into());
        }
        let rd = |off: usize, n: usize| -> Vec<u8> { raw[off..off + n].to_vec() };
        let index_nonce: [u8; 12] = rd(13, 12).try_into().unwrap();
        let index_offset = u64::from_le_bytes(rd(25, 8).try_into().unwrap()) as usize;
        let index_len = u64::from_le_bytes(rd(33, 8).try_into().unwrap()) as usize;
        if index_offset + index_len > raw.len() {
            return Err("资源包索引越界（文件损坏）".into());
        }
        // 索引解密（AAD = b"sf-index"）
        let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
        let index_plain = cipher
            .decrypt(
                Nonce::from_slice(&index_nonce),
                Payload { msg: &raw[index_offset..index_offset + index_len], aad: INDEX_AAD },
            )
            .map_err(|_| "资源包索引解密失败（密钥不匹配）")?;
        let index: serde_json::Value = serde_json::from_slice(&index_plain)
            .map_err(|e| format!("资源包索引解析失败: {e}"))?;
        let mut map = BTreeMap::new();
        if let Some(entries) = index.get("entries").and_then(|v| v.as_array()) {
            for e in entries {
                let path = e.get("p").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let offset = e.get("o").and_then(|v| v.as_u64()).unwrap_or(0);
                let len = e.get("l").and_then(|v| v.as_u64()).unwrap_or(0);
                if !path.is_empty() {
                    map.insert(path, Entry { offset, len });
                }
            }
        }
        Ok(Self { map, key: *key, raw })
    }

    /// 条目数
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// 条目路径清单（runtime 装表遍历用）
    pub fn entry_paths(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// 按路径取资源（封装解密 + AAD=路径）
    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let Some(e) = self.map.get(path) else {
            return Err(format!("资源包内无此资源: {path}"));
        };
        let off = e.offset as usize;
        let end = off + e.len as usize;
        if end > self.raw.len() {
            return Err(format!("资源条目越界: {path}"));
        }
        unwrap_envelope(&self.raw[off..end], &self.key)
    }
}

/// wasm 告警（console——packs 忽略等流程提示）
#[cfg(target_arch = "wasm32")]
pub(crate) fn console_note(msg: &str) {
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen]
    extern "C" {
        #[wasm_bindgen(js_namespace = console)]
        fn log(s: &str);
    }
    log(&format!("[starfish-io] {msg}"));
}
