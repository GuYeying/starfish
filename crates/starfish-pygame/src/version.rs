//! pygame.version 对应物（薄模块）

/// starfish pygame 层版本（= crate 版本；对位 `pygame.version.ver`）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 版本三元组（对位 `pygame.version.vernum`）
pub fn get_version() -> (u8, u8, u8) {
    let mut it = VERSION.split('.');
    (
        it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_parses() {
        let (a, b, _) = super::get_version();
        assert_eq!((a, b), (0, 9));
    }
}
