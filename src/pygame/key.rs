//! pygame.key 对应物（契约 ADR-7：状态表 + locals 常量）
//!
//! 状态表由 [`crate::pygame::event`] 翻译时顺带喂（引擎事件 = 唯一事实
//! 源，不引入额外维护对象）；`get_pressed()` 是表只读视图。

use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use crate::base::window::{KeyCode, Modifiers};

thread_local! {
    static PRESSED: RefCell<HashSet<KeyCode>> = RefCell::new(HashSet::new());
    /// 最近一次按键事件的修饰键位集（KMOD_* 位；get_mods 读）
    static MODS: Cell<u32> = const { Cell::new(0) };
    /// 窗口键盘聚焦态（FocusGained/Lost 事件喂）
    static FOCUSED: Cell<bool> = const { Cell::new(true) };
}

/// 修饰键位常量（位值自定，承诺名字与语义——shift/ctrl/alt/meta 组位）
pub const KMOD_SHIFT: u32 = 1 << 0;
pub const KMOD_CTRL: u32 = 1 << 1;
pub const KMOD_ALT: u32 = 1 << 2;
pub const KMOD_META: u32 = 1 << 3;


macro_rules! pygame_keys {
    ($($name:ident => $variant:ident),* $(,)?) => {
        $(
            #[doc = concat!("K_", stringify!($name))]
            pub const $name: KeyCode = KeyCode::$variant;
        )*
    };
}

pygame_keys! {
    // 字母
    K_a => KeyA, K_b => KeyB, K_c => KeyC, K_d => KeyD, K_e => KeyE,
    K_f => KeyF, K_g => KeyG, K_h => KeyH, K_i => KeyI, K_j => KeyJ,
    K_k => KeyK, K_l => KeyL, K_m => KeyM, K_n => KeyN, K_o => KeyO,
    K_p => KeyP, K_q => KeyQ, K_r => KeyR, K_s => KeyS, K_t => KeyT,
    K_u => KeyU, K_v => KeyV, K_w => KeyW, K_x => KeyX, K_y => KeyY,
    K_z => KeyZ,
    // 数字行
    K_0 => Digit0, K_1 => Digit1, K_2 => Digit2, K_3 => Digit3,
    K_4 => Digit4, K_5 => Digit5, K_6 => Digit6, K_7 => Digit7,
    K_8 => Digit8, K_9 => Digit9,
    // 功能键
    K_F1 => F1, K_F2 => F2, K_F3 => F3, K_F4 => F4, K_F5 => F5,
    K_F6 => F6, K_F7 => F7, K_F8 => F8, K_F9 => F9, K_F10 => F10,
    K_F11 => F11, K_F12 => F12,
    // 控制键
    K_ESCAPE => Escape, K_TAB => Tab, K_CAPSLOCK => CapsLock,
    K_SPACE => Space, K_RETURN => Enter, K_BACKSPACE => Backspace,
    K_DELETE => Delete, K_INSERT => Insert,
    K_SHIFT => ShiftLeft, K_LSHIFT => ShiftLeft, K_RSHIFT => ShiftRight,
    K_CTRL => ControlLeft, K_LCTRL => ControlLeft, K_RCTRL => ControlRight,
    K_ALT => AltLeft, K_LALT => AltLeft, K_RALT => AltRight,
    K_META => MetaLeft, K_LMETA => MetaLeft, K_RMETA => MetaRight,
    K_MENU => ContextMenu,
    // 方向 / 翻页
    K_UP => ArrowUp, K_DOWN => ArrowDown, K_LEFT => ArrowLeft,
    K_RIGHT => ArrowRight,
    K_HOME => Home, K_END => End,
    K_PAGEUP => PageUp, K_PAGEDOWN => PageDown,
    // 符号
    K_MINUS => Minus, K_EQUALS => Equal,
    K_LEFTBRACKET => BracketLeft, K_RIGHTBRACKET => BracketRight,
    K_BACKSLASH => Backslash, K_SEMICOLON => Semicolon, K_QUOTE => Quote,
    K_BACKQUOTE => Backquote, K_COMMA => Comma, K_PERIOD => Period,
    K_SLASH => Slash,
    // 小键盘
    K_KP0 => Numpad0, K_KP1 => Numpad1, K_KP2 => Numpad2,
    K_KP3 => Numpad3, K_KP4 => Numpad4, K_KP5 => Numpad5,
    K_KP6 => Numpad6, K_KP7 => Numpad7, K_KP8 => Numpad8,
    K_KP9 => Numpad9,
    K_KP_PLUS => NumpadAdd, K_KP_MINUS => NumpadSubtract,
    K_KP_MULTIPLY => NumpadMultiply, K_KP_DIVIDE => NumpadDivide,
    K_KP_ENTER => NumpadEnter, K_KP_PERIOD => NumpadDecimal,
    K_PRINT => PrintScreen, K_SCROLLLOCK => ScrollLock,
    K_PAUSE => Pause, K_NUMLOCK => NumLock,
}



    
pub(crate) fn press(key: KeyCode) {
    PRESSED.with(|s| s.borrow_mut().insert(key));
}

pub(crate) fn release(key: KeyCode) {
    PRESSED.with(|s| s.borrow_mut().remove(&key));
}

pub(crate) fn set_mods(m: Modifiers) {
    MODS.with(|s| {
        let mut v = 0;
        if m.shift {
            v |= KMOD_SHIFT;
        }
        if m.ctrl {
            v |= KMOD_CTRL;
        }
        if m.alt {
            v |= KMOD_ALT;
        }
        if m.meta {
            v |= KMOD_META;
        }
        s.set(v);
    });
}

pub(crate) fn set_focused(v: bool) {
    FOCUSED.with(|s| s.set(v));
}

/// 按键按下状态视图（[`get_pressed`] 产出）
#[derive(Clone, Copy)]
pub struct Pressed;

/// 全部按键的按下状态（pygame `key.get_pressed()` 的类型化形态）
pub fn get_pressed() -> Pressed {
    Pressed
}

/// 修饰键位集（pygame `key.get_mods()` 的 KMOD_* 位）
pub fn get_mods() -> u32 {
    MODS.with(|s| s.get())
}

/// 窗口键盘聚焦态（pygame `key.get_focused()`）
pub fn get_focused() -> bool {
    FOCUSED.with(|s| s.get())
}

/// 键名（W3C KeyboardEvent.code 命名——`"KeyA"`/`"ArrowLeft"`/`"Space"`；
/// pygame `key.name()` 对位，返回 typed KeyCode 的代码名）
pub fn name(key: KeyCode) -> String {
    format!("{key:?}")
}

/// 键重复软件合成（垫底：自动重复由 OS 事件流的 repeat 标记承载，
/// 引擎无软件合成——收下不生效）
pub fn set_repeat(_delay: u32, _interval: u32) {}

/// 键重复配置查询（垫底：`(0, 0)` = 未启用软件重复）
pub fn get_repeat() -> (u32, u32) {
    (0, 0)
}

impl Pressed {
    /// 指定键是否按下
    pub fn get(&self, key: KeyCode) -> bool {
        PRESSED.with(|s| s.borrow().contains(&key))
    }

    /// 方向族（方向键；教程四向移动的正典入口）
    pub fn left(&self) -> bool {
        self.get(KeyCode::ArrowLeft)
    }
    pub fn right(&self) -> bool {
        self.get(KeyCode::ArrowRight)
    }
    pub fn up(&self) -> bool {
        self.get(KeyCode::ArrowUp)
    }
    pub fn down(&self) -> bool {
        self.get(KeyCode::ArrowDown)
    }
    pub fn space(&self) -> bool {
        self.get(KeyCode::Space)
    }
    pub fn escape(&self) -> bool {
        self.get(KeyCode::Escape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_release_roundtrip() {
        press(KeyCode::KeyA);
        assert!(get_pressed().get(KeyCode::KeyA));
        assert!(!get_pressed().get(KeyCode::KeyB));
        release(KeyCode::KeyA);
        assert!(!get_pressed().get(KeyCode::KeyA));
    }
}
