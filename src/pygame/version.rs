//! pygame.version 对应物（薄模块）

/// starfish pygame 层版本（= crate 版本；对位 `pygame.version.ver`）
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 版本三元组（对位 `pygame.version.vernum`）
pub fn vernum() -> (u8, u8, u8) {
    let mut it = VERSION.split('.');
    (
        it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
        it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
    )
}

/// 底层 SDL 版本（垫底原则：保留名字，返回安全默认 (2, 0, 20)——
/// 本层无 SDL，接口形态对齐供教程/绑定层使用）
pub fn get_sdl_version() -> (u8, u8, u8) {
    (2, 0, 20)
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_parses() {
        let (a, b, _) = super::vernum();
        assert_eq!((a, b), (0, 9));
        assert_eq!(super::get_sdl_version(), (2, 0, 20));
    }
}
