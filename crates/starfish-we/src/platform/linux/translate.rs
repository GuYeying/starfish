//! Linux/Wayland event translation: evdev keycodes → [`KeyCode`],
//! pointer buttons → [`MouseButton`], scroll axes → wheel deltas.
//! Linux/Wayland 事件翻译：evdev 键码 → [`KeyCode`]，指针按键 →
//! [`MouseButton`]，滚动轴 → 滚轮增量。
//!
//! sctk hands us RAW evdev codes (it applies the `+8` xkb offset
//! internally for keysym lookup only) — this table maps them straight to
//! the W3C `KeyboardEvent.code`-named [`KeyCode`] variants, mirroring
//! `windows/translate.rs`'s VK table. Unknown codes fall back to
//! `KeyCode::Other(raw)`.
//! sctk 交给我们的是**裸 evdev 码**（`+8` 的 xkb 偏移只在它内部查
//! keysym 时用）——本表将其直接映射为 W3C `KeyboardEvent.code` 命名的
//! [`KeyCode`]，镜像 `windows/translate.rs` 的 VK 表。未知码回退
//! `KeyCode::Other(raw)`。

use crate::event::{KeyCode as K, MouseButton};

/// evdev → [`KeyCode`] (raw codes, no xkb offset).
/// evdev → [`KeyCode`]（裸码，无 xkb 偏移）。
pub(super) fn evdev_to_keycode(raw: u32) -> K {
    // Letters: KEY_A=30 … KEY_Z=44 (unordered!)
    // 字母：KEY_A=30 … KEY_Z=44（乱序！）
    const LETTERS: [K; 26] = [
        K::KeyA,
        K::KeyB,
        K::KeyC,
        K::KeyD,
        K::KeyE,
        K::KeyF,
        K::KeyG,
        K::KeyH,
        K::KeyI,
        K::KeyJ,
        K::KeyK,
        K::KeyL,
        K::KeyM,
        K::KeyN,
        K::KeyO,
        K::KeyP,
        K::KeyQ,
        K::KeyR,
        K::KeyS,
        K::KeyT,
        K::KeyU,
        K::KeyV,
        K::KeyW,
        K::KeyX,
        K::KeyY,
        K::KeyZ,
    ];
    const LETTER_CODES: [u32; 26] = [
        30, // A
        48, // B
        46, // C
        32, // D
        18, // E
        33, // F
        34, // G
        35, // H
        23, // I
        36, // J
        37, // K
        38, // L
        50, // M
        49, // N
        24, // O
        25, // P
        16, // Q
        19, // R
        31, // S
        20, // T
        22, // U
        47, // V
        17, // W
        45, // X
        21, // Y
        44, // Z
    ];
    // Digits: KEY_1=2 … KEY_0=11.
    // 数字：KEY_1=2 … KEY_0=11。
    const DIGITS: [K; 10] = [
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
        K::Digit0,
    ];
    // F1..F10 are contiguous (59..=68); F11/F12 sit elsewhere.
    // F1..F10 连续（59..=68）；F11/F12 在别处。
    const FKEYS: [K; 12] = [
        K::F1,
        K::F2,
        K::F3,
        K::F4,
        K::F5,
        K::F6,
        K::F7,
        K::F8,
        K::F9,
        K::F10,
        K::F11,
        K::F12,
    ];
    // Numpad digits are scattered across three rows (71..=73, 75..=77,
    // 79..=82).
    // 小键盘数字分散在三行（71..=73、75..=77、79..=82）。
    const NUMPAD: [(u32, K); 10] = [
        (82, K::Numpad0),
        (79, K::Numpad1),
        (80, K::Numpad2),
        (81, K::Numpad3),
        (75, K::Numpad4),
        (76, K::Numpad5),
        (77, K::Numpad6),
        (71, K::Numpad7),
        (72, K::Numpad8),
        (73, K::Numpad9),
    ];

    if let Some(i) = LETTER_CODES.iter().position(|&c| c == raw) {
        return LETTERS[i];
    }
    if (2..=11).contains(&raw) {
        return DIGITS[(raw - 2) as usize];
    }
    if (59..=68).contains(&raw) {
        return FKEYS[(raw - 59) as usize];
    }
    if raw == 87 {
        return K::F11;
    }
    if raw == 88 {
        return K::F12;
    }
    for &(code, key) in &NUMPAD {
        if raw == code {
            return key;
        }
    }
    match raw {
        1 => K::Escape,
        15 => K::Tab,
        14 => K::Backspace,
        28 => K::Enter,
        57 => K::Space,
        58 => K::CapsLock,
        12 => K::Minus,
        13 => K::Equal,
        26 => K::BracketLeft,
        27 => K::BracketRight,
        43 => K::Backslash,
        39 => K::Semicolon,
        40 => K::Quote,
        41 => K::Backquote,
        51 => K::Comma,
        52 => K::Period,
        53 => K::Slash,
        42 => K::ShiftLeft,
        54 => K::ShiftRight,
        29 => K::ControlLeft,
        97 => K::ControlRight,
        56 => K::AltLeft,
        100 => K::AltRight,
        125 => K::MetaLeft,
        126 => K::MetaRight,
        127 => K::ContextMenu,
        105 => K::ArrowLeft,
        106 => K::ArrowRight,
        103 => K::ArrowUp,
        108 => K::ArrowDown,
        102 => K::Home,
        107 => K::End,
        104 => K::PageUp,
        109 => K::PageDown,
        110 => K::Insert,
        111 => K::Delete,
        69 => K::NumLock,
        70 => K::ScrollLock,
        119 => K::Pause,
        99 => K::PrintScreen,
        // evdev has no NumpadAdd-style keys beyond the table above that
        // map to our W3C set — everything else keeps its native code.
        // 上表之外没有能映射进我们 W3C 集的键——其余保留原生码。
        _ => K::Other(raw),
    }
}

/// evdev button code → [`MouseButton`] (standard values: BTN_LEFT=0x110,
/// BTN_RIGHT=0x111, BTN_MIDDLE=0x112, BTN_SIDE=0x113, BTN_EXTRA=0x114).
/// evdev 按键码 → [`MouseButton`]（标准值：BTN_LEFT=0x110、BTN_RIGHT=0x111、
/// BTN_MIDDLE=0x112、BTN_SIDE=0x113、BTN_EXTRA=0x114）。
pub(super) fn mouse_button(code: u32) -> MouseButton {
    match code {
        0x110 => MouseButton::Left,
        0x111 => MouseButton::Right,
        0x112 => MouseButton::Middle,
        0x113 => MouseButton::Back,
        0x114 => MouseButton::Forward,
        other => MouseButton::Other(other as u8),
    }
}

/// Scroll axis → wheel delta in detents (our convention: positive =
/// up/right). `value120` is the modern high-resolution unit (120 = one
/// detent); compositors without it send `discrete` detents directly.
/// 滚动轴 → 棘轮格数增量（本库约定：正 = 上/右）。`value120` 是现代
/// 高分辨率单位（120 = 一格）；老合成器直接发 `discrete` 格数。
pub(super) fn wheel_delta(value120: i32, discrete: i32) -> f64 {
    if value120 != 0 {
        f64::from(value120) / 120.0
    } else {
        f64::from(discrete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_map_to_w3c_names() {
        assert!(matches!(evdev_to_keycode(30), K::KeyA));
        assert!(matches!(evdev_to_keycode(44), K::KeyZ));
        assert!(matches!(evdev_to_keycode(17), K::KeyW));
    }

    #[test]
    fn digits_and_fkeys() {
        assert!(matches!(evdev_to_keycode(2), K::Digit1));
        assert!(matches!(evdev_to_keycode(11), K::Digit0));
        assert!(matches!(evdev_to_keycode(59), K::F1));
        assert!(matches!(evdev_to_keycode(68), K::F10));
        assert!(matches!(evdev_to_keycode(87), K::F11));
        assert!(matches!(evdev_to_keycode(88), K::F12));
    }

    #[test]
    fn named_keys_and_fallback() {
        assert!(matches!(evdev_to_keycode(1), K::Escape));
        assert!(matches!(evdev_to_keycode(28), K::Enter));
        assert!(matches!(evdev_to_keycode(105), K::ArrowLeft));
        assert!(matches!(evdev_to_keycode(82), K::Numpad0));
        // Unknown code keeps its native value.
        // 未知码保留原生值。
        assert!(matches!(evdev_to_keycode(999), K::Other(999)));
    }

    #[test]
    fn mouse_buttons() {
        assert!(matches!(mouse_button(0x110), MouseButton::Left));
        assert!(matches!(mouse_button(0x111), MouseButton::Right));
        assert!(matches!(mouse_button(0x114), MouseButton::Forward));
        // 0x105 does not fit u8 — MouseButton::Other truncates (as u8).
        // 0x153 超出 u8 —— Other 按 `as u8` 截断。
        assert!(matches!(mouse_button(0x105), MouseButton::Other(5)));
    }

    #[test]
    fn wheel_prefers_value120() {
        assert_eq!(wheel_delta(120, 1), 1.0);
        assert_eq!(wheel_delta(240, 2), 2.0);
        assert_eq!(wheel_delta(-120, -1), -1.0);
        // Legacy compositors: fall back to discrete detents.
        // 老合成器：回退 discrete 格数。
        assert_eq!(wheel_delta(0, 3), 3.0);
    }
}
