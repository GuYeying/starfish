//! iOS 输入翻译：HID 用法码 → [`KeyCode`]、修饰键位 → [`Modifiers`]、
//! 软键盘插入文本 → 事件序列。
//! iOS input translation: HID usage → [`KeyCode`], modifier flags →
//! [`Modifiers`], soft-keyboard inserted text → event sequences.

use objc2_ui_kit::{
    UIKeyInputDownArrow, UIKeyInputEscape, UIKeyInputLeftArrow, UIKeyInputRightArrow,
    UIKeyInputUpArrow, UIKeyboardHIDUsage, UIKeyModifierFlags,
};

use crate::event::{Event, KeyCode, Modifiers};

/// `UIKeyboardHIDUsage`（即标准 USB HID 用法码，与 SDL/winit 的 HID 表同源）
/// → [`KeyCode`]。未映射的系统/国际键返回 `None`（事件丢弃，文本仍可经
/// `characters` 送达）。
/// `UIKeyboardHIDUsage` (the standard USB HID usage IDs, same table as
/// SDL/winit) → [`KeyCode`]. Unmapped system/international keys return
/// `None` (the event is dropped; text still arrives via `characters`).
pub(super) fn hid_usage_to_key_code(usage: UIKeyboardHIDUsage) -> Option<KeyCode> {
    use KeyCode as K;

    let key = if usage == UIKeyboardHIDUsage::KeyboardA {
        K::KeyA
    } else if usage == UIKeyboardHIDUsage::KeyboardB {
        K::KeyB
    } else if usage == UIKeyboardHIDUsage::KeyboardC {
        K::KeyC
    } else if usage == UIKeyboardHIDUsage::KeyboardD {
        K::KeyD
    } else if usage == UIKeyboardHIDUsage::KeyboardE {
        K::KeyE
    } else if usage == UIKeyboardHIDUsage::KeyboardF {
        K::KeyF
    } else if usage == UIKeyboardHIDUsage::KeyboardG {
        K::KeyG
    } else if usage == UIKeyboardHIDUsage::KeyboardH {
        K::KeyH
    } else if usage == UIKeyboardHIDUsage::KeyboardI {
        K::KeyI
    } else if usage == UIKeyboardHIDUsage::KeyboardJ {
        K::KeyJ
    } else if usage == UIKeyboardHIDUsage::KeyboardK {
        K::KeyK
    } else if usage == UIKeyboardHIDUsage::KeyboardL {
        K::KeyL
    } else if usage == UIKeyboardHIDUsage::KeyboardM {
        K::KeyM
    } else if usage == UIKeyboardHIDUsage::KeyboardN {
        K::KeyN
    } else if usage == UIKeyboardHIDUsage::KeyboardO {
        K::KeyO
    } else if usage == UIKeyboardHIDUsage::KeyboardP {
        K::KeyP
    } else if usage == UIKeyboardHIDUsage::KeyboardQ {
        K::KeyQ
    } else if usage == UIKeyboardHIDUsage::KeyboardR {
        K::KeyR
    } else if usage == UIKeyboardHIDUsage::KeyboardS {
        K::KeyS
    } else if usage == UIKeyboardHIDUsage::KeyboardT {
        K::KeyT
    } else if usage == UIKeyboardHIDUsage::KeyboardU {
        K::KeyU
    } else if usage == UIKeyboardHIDUsage::KeyboardV {
        K::KeyV
    } else if usage == UIKeyboardHIDUsage::KeyboardW {
        K::KeyW
    } else if usage == UIKeyboardHIDUsage::KeyboardX {
        K::KeyX
    } else if usage == UIKeyboardHIDUsage::KeyboardY {
        K::KeyY
    } else if usage == UIKeyboardHIDUsage::KeyboardZ {
        K::KeyZ
    } else if usage == UIKeyboardHIDUsage::Keyboard1 {
        K::Digit1
    } else if usage == UIKeyboardHIDUsage::Keyboard2 {
        K::Digit2
    } else if usage == UIKeyboardHIDUsage::Keyboard3 {
        K::Digit3
    } else if usage == UIKeyboardHIDUsage::Keyboard4 {
        K::Digit4
    } else if usage == UIKeyboardHIDUsage::Keyboard5 {
        K::Digit5
    } else if usage == UIKeyboardHIDUsage::Keyboard6 {
        K::Digit6
    } else if usage == UIKeyboardHIDUsage::Keyboard7 {
        K::Digit7
    } else if usage == UIKeyboardHIDUsage::Keyboard8 {
        K::Digit8
    } else if usage == UIKeyboardHIDUsage::Keyboard9 {
        K::Digit9
    } else if usage == UIKeyboardHIDUsage::Keyboard0 {
        K::Digit0
    } else if usage == UIKeyboardHIDUsage::KeyboardReturnOrEnter {
        K::Enter
    } else if usage == UIKeyboardHIDUsage::KeyboardEscape {
        K::Escape
    } else if usage == UIKeyboardHIDUsage::KeyboardDeleteOrBackspace {
        K::Backspace
    } else if usage == UIKeyboardHIDUsage::KeyboardTab {
        K::Tab
    } else if usage == UIKeyboardHIDUsage::KeyboardSpacebar {
        K::Space
    } else if usage == UIKeyboardHIDUsage::KeyboardHyphen {
        K::Minus
    } else if usage == UIKeyboardHIDUsage::KeyboardEqualSign {
        K::Equal
    } else if usage == UIKeyboardHIDUsage::KeyboardOpenBracket {
        K::BracketLeft
    } else if usage == UIKeyboardHIDUsage::KeyboardCloseBracket {
        K::BracketRight
    } else if usage == UIKeyboardHIDUsage::KeyboardBackslash {
        K::Backslash
    } else if usage == UIKeyboardHIDUsage::KeyboardSemicolon {
        K::Semicolon
    } else if usage == UIKeyboardHIDUsage::KeyboardQuote {
        K::Quote
    } else if usage == UIKeyboardHIDUsage::KeyboardGraveAccentAndTilde {
        K::Backquote
    } else if usage == UIKeyboardHIDUsage::KeyboardComma {
        K::Comma
    } else if usage == UIKeyboardHIDUsage::KeyboardPeriod {
        K::Period
    } else if usage == UIKeyboardHIDUsage::KeyboardSlash {
        K::Slash
    } else if usage == UIKeyboardHIDUsage::KeyboardCapsLock {
        K::CapsLock
    } else if usage == UIKeyboardHIDUsage::KeyboardF1 {
        K::F1
    } else if usage == UIKeyboardHIDUsage::KeyboardF2 {
        K::F2
    } else if usage == UIKeyboardHIDUsage::KeyboardF3 {
        K::F3
    } else if usage == UIKeyboardHIDUsage::KeyboardF4 {
        K::F4
    } else if usage == UIKeyboardHIDUsage::KeyboardF5 {
        K::F5
    } else if usage == UIKeyboardHIDUsage::KeyboardF6 {
        K::F6
    } else if usage == UIKeyboardHIDUsage::KeyboardF7 {
        K::F7
    } else if usage == UIKeyboardHIDUsage::KeyboardF8 {
        K::F8
    } else if usage == UIKeyboardHIDUsage::KeyboardF9 {
        K::F9
    } else if usage == UIKeyboardHIDUsage::KeyboardF10 {
        K::F10
    } else if usage == UIKeyboardHIDUsage::KeyboardF11 {
        K::F11
    } else if usage == UIKeyboardHIDUsage::KeyboardF12 {
        K::F12
    } else if usage == UIKeyboardHIDUsage::KeyboardPrintScreen {
        K::PrintScreen
    } else if usage == UIKeyboardHIDUsage::KeyboardScrollLock {
        K::ScrollLock
    } else if usage == UIKeyboardHIDUsage::KeyboardPause {
        K::Pause
    } else if usage == UIKeyboardHIDUsage::KeyboardInsert {
        K::Insert
    } else if usage == UIKeyboardHIDUsage::KeyboardHome {
        K::Home
    } else if usage == UIKeyboardHIDUsage::KeyboardPageUp {
        K::PageUp
    } else if usage == UIKeyboardHIDUsage::KeyboardDeleteForward {
        K::Delete
    } else if usage == UIKeyboardHIDUsage::KeyboardEnd {
        K::End
    } else if usage == UIKeyboardHIDUsage::KeyboardPageDown {
        K::PageDown
    } else if usage == UIKeyboardHIDUsage::KeyboardRightArrow {
        K::ArrowRight
    } else if usage == UIKeyboardHIDUsage::KeyboardLeftArrow {
        K::ArrowLeft
    } else if usage == UIKeyboardHIDUsage::KeyboardDownArrow {
        K::ArrowDown
    } else if usage == UIKeyboardHIDUsage::KeyboardUpArrow {
        K::ArrowUp
    } else if usage == UIKeyboardHIDUsage::KeypadNumLock {
        K::NumLock
    } else if usage == UIKeyboardHIDUsage::KeypadSlash {
        K::NumpadDivide
    } else if usage == UIKeyboardHIDUsage::KeypadAsterisk {
        K::NumpadMultiply
    } else if usage == UIKeyboardHIDUsage::KeypadHyphen {
        K::NumpadSubtract
    } else if usage == UIKeyboardHIDUsage::KeypadPlus {
        K::NumpadAdd
    } else if usage == UIKeyboardHIDUsage::KeypadEnter {
        K::NumpadEnter
    } else if usage == UIKeyboardHIDUsage::Keypad1 {
        K::Numpad1
    } else if usage == UIKeyboardHIDUsage::Keypad2 {
        K::Numpad2
    } else if usage == UIKeyboardHIDUsage::Keypad3 {
        K::Numpad3
    } else if usage == UIKeyboardHIDUsage::Keypad4 {
        K::Numpad4
    } else if usage == UIKeyboardHIDUsage::Keypad5 {
        K::Numpad5
    } else if usage == UIKeyboardHIDUsage::Keypad6 {
        K::Numpad6
    } else if usage == UIKeyboardHIDUsage::Keypad7 {
        K::Numpad7
    } else if usage == UIKeyboardHIDUsage::Keypad8 {
        K::Numpad8
    } else if usage == UIKeyboardHIDUsage::Keypad9 {
        K::Numpad9
    } else if usage == UIKeyboardHIDUsage::Keypad0 {
        K::Numpad0
    } else if usage == UIKeyboardHIDUsage::KeypadPeriod {
        K::NumpadDecimal
    } else if usage == UIKeyboardHIDUsage::KeyboardApplication {
        K::ContextMenu
    } else if usage == UIKeyboardHIDUsage::KeyboardLeftControl {
        K::ControlLeft
    } else if usage == UIKeyboardHIDUsage::KeyboardLeftShift {
        K::ShiftLeft
    } else if usage == UIKeyboardHIDUsage::KeyboardLeftAlt {
        K::AltLeft
    } else if usage == UIKeyboardHIDUsage::KeyboardLeftGUI {
        K::MetaLeft
    } else if usage == UIKeyboardHIDUsage::KeyboardRightControl {
        K::ControlRight
    } else if usage == UIKeyboardHIDUsage::KeyboardRightShift {
        K::ShiftRight
    } else if usage == UIKeyboardHIDUsage::KeyboardRightAlt {
        K::AltRight
    } else if usage == UIKeyboardHIDUsage::KeyboardRightGUI {
        K::MetaRight
    } else {
        return None;
    };
    Some(key)
}

/// `UIKeyModifierFlags` → [`Modifiers`]（macOS Command 归 `meta`，与
/// crate 的事件词汇一致）。
/// `UIKeyModifierFlags` → [`Modifiers`] (macOS Command maps to `meta`,
/// matching the crate's event vocabulary).
pub(super) fn modifiers_from_flags(flags: UIKeyModifierFlags) -> Modifiers {
    Modifiers {
        shift: flags.contains(UIKeyModifierFlags::Shift),
        ctrl: flags.contains(UIKeyModifierFlags::Control),
        alt: flags.contains(UIKeyModifierFlags::Alternate),
        meta: flags.contains(UIKeyModifierFlags::Command),
    }
}

/// 软键盘 `insertText:` 的字符串 → 事件序列。系统把方向键/Escape 等
/// 特殊键以 `UIKeyInput*` 常量字符串送入（见 UIResponder 生成码导出的
/// 同名常量），翻译为 Key 按下/抬起对；其余文本（含多字符，如自动
/// 更正/联想）原样一条 [`Event::TextInput`]，由消费方逐字符过滤。
/// The soft-keyboard `insertText:` string → event sequence. The system
/// delivers arrow keys/Escape etc. as the `UIKeyInput*` constant strings
/// (exported by the same names in the generated UIResponder bindings);
/// those become Key down/up pairs. Everything else (possibly multi-char,
/// e.g. autocorrect) is delivered as one [`Event::TextInput`] for the
/// consumer to filter per character.
pub(super) fn insert_text_to_events(text: &str) -> Vec<Event> {
    // SAFETY: 这些 extern static 由 ObjC 运行时在进程启动时初始化、
    // 进程级常量（UIKit 导出的字符串符号），读取恒安全。
    // SAFETY: these extern statics are initialized by the ObjC runtime at
    // process start and never change (UIKit-exported string symbols);
    // reading them is always sound.
    #[expect(unsafe_code)]
    let special = unsafe {
        if text == UIKeyInputUpArrow.to_string() {
            Some(KeyCode::ArrowUp)
        } else if text == UIKeyInputDownArrow.to_string() {
            Some(KeyCode::ArrowDown)
        } else if text == UIKeyInputLeftArrow.to_string() {
            Some(KeyCode::ArrowLeft)
        } else if text == UIKeyInputRightArrow.to_string() {
            Some(KeyCode::ArrowRight)
        } else if text == UIKeyInputEscape.to_string() {
            Some(KeyCode::Escape)
        } else {
            None
        }
    };

    match special {
        Some(key) => vec![
            Event::KeyDown {
                key,
                modifiers: Modifiers::default(),
                repeat: false,
            },
            Event::KeyUp {
                key,
                modifiers: Modifiers::default(),
            },
        ],
        None => {
            if text.is_empty() {
                Vec::new()
            } else {
                vec![Event::TextInput {
                    text: text.to_owned(),
                }]
            }
        }
    }
}
