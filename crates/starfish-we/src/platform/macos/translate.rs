//! macOS event translation helpers.
//! macOS 事件翻译辅助（键码表 / 修饰键 / 按钮与坐标换算）。
//!
//! Key codes are the Carbon-era virtual key codes carried by `NSEvent
//! keyCode` — hardware positions, layout independent, same spirit as the
//! W3C codes we map to.
//! 键码即 `NSEvent keyCode` 携带的 Carbon 时代虚拟键码——硬件位置、
//! 与布局无关，与我们映射目标的 W3C 命名同一精神。

use objc2_app_kit::NSEventModifierFlags;

use crate::event::{KeyCode, Modifiers};

/// Carbon virtual key code → [`KeyCode`]: item-by-item mapping (Carbon
/// codes are scattered by hardware history, not contiguous ranges);
/// unrecognized keys become [`KeyCode::Other`] (carrying the native code)
/// so no key is lost.
/// Carbon 虚拟键码 → [`KeyCode`]：Carbon 码按硬件历史分散、不构成连续
/// 区间，故逐项映射；未识别的键包成 [`KeyCode::Other`]（携带原生键码），
/// 不丢按键。
pub(crate) fn keycode_to_keycode(keycode: u16) -> KeyCode {
    use KeyCode as K;
    match keycode {
        // Letters (scattered across 0x00–0x20 by hardware history).
        // 字母（因硬件历史散布在 0x00–0x20）。
        0x00 => K::KeyA,
        0x01 => K::KeyS,
        0x02 => K::KeyD,
        0x03 => K::KeyF,
        0x04 => K::KeyH,
        0x05 => K::KeyG,
        0x06 => K::KeyZ,
        0x07 => K::KeyX,
        0x08 => K::KeyC,
        0x09 => K::KeyV,
        0x0B => K::KeyB,
        0x0C => K::KeyQ,
        0x0D => K::KeyW,
        0x0E => K::KeyE,
        0x0F => K::KeyR,
        0x10 => K::KeyY,
        0x11 => K::KeyT,
        0x12 => K::Digit1,
        0x13 => K::Digit2,
        0x14 => K::Digit3,
        0x15 => K::Digit4,
        0x16 => K::Digit6,
        0x17 => K::Digit5,
        0x18 => K::Equal,
        0x19 => K::Digit9,
        0x1A => K::Digit7,
        0x1B => K::Minus,
        0x1C => K::Digit8,
        0x1D => K::Digit0,
        0x1E => K::BracketRight,
        0x1F => K::KeyO,
        0x20 => K::KeyU,
        0x21 => K::BracketLeft,
        0x22 => K::KeyI,
        0x23 => K::KeyP,
        0x25 => K::KeyL,
        0x26 => K::KeyJ,
        0x27 => K::Quote,
        0x28 => K::KeyK,
        0x29 => K::Semicolon,
        0x2A => K::Backslash,
        0x2B => K::Comma,
        0x2C => K::Slash,
        0x2D => K::KeyN,
        0x2E => K::KeyM,
        0x2F => K::Period,
        // Editing / whitespace / navigation.
        // 编辑 / 空白 / 导航。
        0x24 => K::Enter,
        0x30 => K::Tab,
        0x31 => K::Space,
        0x33 => K::Backspace,
        0x35 => K::Escape,
        0x73 => K::Home,
        0x74 => K::PageUp,
        0x75 => K::Delete,
        0x77 => K::End,
        0x79 => K::PageDown,
        0x7B => K::ArrowLeft,
        0x7C => K::ArrowRight,
        0x7D => K::ArrowDown,
        0x7E => K::ArrowUp,
        // Function keys.
        // 功能键。
        0x7A => K::F1,
        0x78 => K::F2,
        0x63 => K::F3,
        0x76 => K::F4,
        0x60 => K::F5,
        0x61 => K::F6,
        0x62 => K::F7,
        0x64 => K::F8,
        0x65 => K::F9,
        0x6D => K::F10,
        0x67 => K::F11,
        0x6F => K::F12,
        // Modifiers (left / right distinguished).
        // 修饰键（区分左右）。
        0x38 => K::ShiftLeft,
        0x3C => K::ShiftRight,
        0x3B => K::ControlLeft,
        0x3E => K::ControlRight,
        0x3A => K::AltLeft,
        0x3D => K::AltRight,
        0x37 => K::MetaLeft,
        0x36 => K::MetaRight,
        0x39 => K::CapsLock,
        // Numpad.
        // 小键盘。
        0x52 => K::Numpad0,
        0x53 => K::Numpad1,
        0x54 => K::Numpad2,
        0x55 => K::Numpad3,
        0x56 => K::Numpad4,
        0x57 => K::Numpad5,
        0x58 => K::Numpad6,
        0x59 => K::Numpad7,
        0x5B => K::Numpad8,
        0x5C => K::Numpad9,
        0x41 => K::NumpadDecimal,
        0x43 => K::NumpadMultiply,
        0x45 => K::NumpadAdd,
        0x47 => K::NumLock,
        0x4B => K::NumpadDivide,
        0x4C => K::NumpadEnter,
        0x4E => K::NumpadSubtract,
        // PC PrintScreen lives at F13 on Mac keyboards.
        // PC 的 PrintScreen 在 Mac 键盘上是 F13。
        0x69 => K::PrintScreen,
        0x71 => K::ScrollLock,
        0x6B => K::Pause,
        other => K::Other(other as u32),
    }
}

/// Modifier flags from the event (`NSEventModifierFlags`) → [`Modifiers`].
/// Taken from the event itself — always consistent with the event's moment.
/// 事件自带的修饰键标志（`NSEventModifierFlags`）→ [`Modifiers`]。
/// 直接取自事件——与事件时刻严格一致。
pub(crate) fn modifiers_from_flags(flags: NSEventModifierFlags) -> Modifiers {
    Modifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        ctrl: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        meta: flags.contains(NSEventModifierFlags::Command),
    }
}

/// W3C-style button index (0 left / 1 right / 2 middle) → [`MouseButton`].
/// W3C 风格按钮序号（0 左 / 1 右 / 2 中）→ [`MouseButton`]。
pub(crate) fn button_from_index(index: i64) -> crate::event::MouseButton {
    use crate::event::MouseButton as M;
    match index {
        0 => M::Left,
        1 => M::Right,
        2 => M::Middle,
        other => M::Other(other as u8),
    }
}
