//! pygame.locals 对应物：常量枢纽（契约 ADR-7）。
//!
//! pygame 的 locals 是整数常量；本层用**类型化常量**——`K_a` 即
//! [`KeyCode::KeyA`]，拼写与 pygame 一致（迁移优先），类型由编译器
//! 兜底（绑定层零翻译）。常量按子模块组织（key/mouse），新增常量
//! 一律进这里，不散落各处。

use crate::base::window::{KeyCode, MouseButton};

/// 键盘常量全集，命名对齐 pygame.locals。
///
/// 引擎 [`KeyCode`] 为 W3C KeyboardEvent.code 命名，未收录键由
/// `KeyCode::Other(u32)` 携带原生键码（不丢按键）。
pub mod key {
    use super::KeyCode;

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
}

/// 鼠标按钮常量（对齐 pygame 惯例：左/中/右 = 1/2/3 → 本层类型化）。
pub mod mouse {
    use super::MouseButton;

    pub const BUTTON_LEFT: MouseButton = MouseButton::Left;
    pub const BUTTON_MIDDLE: MouseButton = MouseButton::Middle;
    pub const BUTTON_RIGHT: MouseButton = MouseButton::Right;
    pub const BUTTON_BACK: MouseButton = MouseButton::Back;
    pub const BUTTON_FORWARD: MouseButton = MouseButton::Forward;
}

/// display 旗标（`display::set_mode_ex` 用；位值 = 引擎 `WindowFlags` 位，
/// 映射直通——位值自定、只承诺名字与语义，兼容层模块架构 §六.5 定案）。
pub mod display {
    /// 可调整大小（对位 pygame RESIZABLE）
    pub const RESIZABLE: u32 = 1 << 0;
    /// 全屏（无边框占屏；对位 pygame FULLSCREEN）
    pub const FULLSCREEN: u32 = 1 << 1;
    /// 无边框（对位 pygame NOFRAME）
    pub const NOFRAME: u32 = 1 << 2;
    /// 创建时隐藏（对位 pygame WINDOW_HIDDEN）
    pub const HIDDEN: u32 = 1 << 3;
    /// 窗口置顶（对位 pygame WINDOW_ALWAYS_ON_TOP）
    pub const ALWAYS_ON_TOP: u32 = 1 << 6;
    /// 透明窗口（**创建期**——透明合成 + Screen 清屏改 alpha 0；
    /// 桌宠/悬浮件形态。运行期不可追溯）
    pub const TRANSPARENT: u32 = 1 << 7;
    /// 后端声明（wgpu 在声明位集内自动挑选、不可用即隐式退化；
    /// 未声明任何后端位 = `Backends::all()` 平台最优）。
    /// 注意：GPU 初始化在 set_mode_ex 内进程一次——后端声明须在
    /// 首次 set_mode 时给出（运行期不可重选）。
    pub const VULKAN: u32 = 1 << 8;
    pub const DIRECTX: u32 = 1 << 9;
    pub const METAL: u32 = 1 << 10;
    pub const OPENGLES: u32 = 1 << 11;
    /// GL 上下文请求（2026-09-30 语义变更：由"显式报错"改为后端声明
    /// = wgpu GL 后端，不可用隐式退化）
    pub const OPENGL: u32 = 1 << 30;

    /// 已登记旗标全集（未登记位 → 警告并忽略，垫底原则）
    pub const KNOWN_MASK: u32 = RESIZABLE
        | FULLSCREEN
        | NOFRAME
        | HIDDEN
        | ALWAYS_ON_TOP
        | TRANSPARENT
        | VULKAN
        | DIRECTX
        | METAL
        | OPENGLES
        | OPENGL;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_constants_are_typed_and_distinct() {
        use key::*;
        assert_ne!(K_a, K_b);
        assert_eq!(K_a, crate::base::window::KeyCode::KeyA);
        assert_eq!(K_RETURN, crate::base::window::KeyCode::Enter);
        assert_eq!(K_KP_ENTER, crate::base::window::KeyCode::NumpadEnter);
        // 同义别名一致（pygame 惯例：无侧别名的默认左）
        assert_eq!(K_SHIFT, K_LSHIFT);
        assert_eq!(K_CTRL, K_LCTRL);
    }

    #[test]
    fn mouse_constants_align() {
        assert_eq!(mouse::BUTTON_LEFT, crate::base::window::MouseButton::Left);
        assert_eq!(mouse::BUTTON_RIGHT, crate::base::window::MouseButton::Right);
    }
}
