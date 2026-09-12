//! 窗口事件模型（平台中立公共契约）
//!
//! 后端（当前 winit）把原生事件翻译为本模块类型；上层（base 用户、
//! `pygame/` 层、未来 PyO3 绑定）只面对这里，不接触任何后端类型。
//! 键码命名对齐 winit 风格；pygame 常量（`K_w` 等）由 `pygame/` 层做别名。
//!
//! 事件派发模型：引擎持循环（见 [`crate::base::app`]），事件在每帧前
//! 逐个回调给 `Application::event`；同时引擎维护键鼠状态表
//! （[`KeyboardState`] / [`MouseState`]），轮询式输入经 `Ctx` 查询。

use std::collections::HashSet;

/// 物理键码（布局无关，语义对齐 SDL Scancode / winit PhysicalKey）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum KeyCode {
    Escape,
    Space,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    SuperLeft,
    SuperRight,
    CapsLock,
    Minus,
    Equal,
    Backquote,
    Comma,
    Period,
    Slash,
    Backslash,
    Semicolon,
    Quote,
    BracketLeft,
    BracketRight,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    NumpadEnter,
    NumpadDecimal,
    /// 未收录键（后端键码的 u32 判别值，向前兼容）
    Other(u32),
}

/// winit KeyCode → 本枚举（收录表之外一律 `Other`）
pub(crate) fn map_key(code: winit::keyboard::KeyCode) -> KeyCode {
    use winit::keyboard::KeyCode as W;
    match code {
        W::Escape => KeyCode::Escape,
        W::Space => KeyCode::Space,
        W::Enter => KeyCode::Enter,
        W::Tab => KeyCode::Tab,
        W::Backspace => KeyCode::Backspace,
        W::Delete => KeyCode::Delete,
        W::Insert => KeyCode::Insert,
        W::Home => KeyCode::Home,
        W::End => KeyCode::End,
        W::PageUp => KeyCode::PageUp,
        W::PageDown => KeyCode::PageDown,
        W::ArrowLeft => KeyCode::ArrowLeft,
        W::ArrowRight => KeyCode::ArrowRight,
        W::ArrowUp => KeyCode::ArrowUp,
        W::ArrowDown => KeyCode::ArrowDown,
        W::KeyA => KeyCode::KeyA,
        W::KeyB => KeyCode::KeyB,
        W::KeyC => KeyCode::KeyC,
        W::KeyD => KeyCode::KeyD,
        W::KeyE => KeyCode::KeyE,
        W::KeyF => KeyCode::KeyF,
        W::KeyG => KeyCode::KeyG,
        W::KeyH => KeyCode::KeyH,
        W::KeyI => KeyCode::KeyI,
        W::KeyJ => KeyCode::KeyJ,
        W::KeyK => KeyCode::KeyK,
        W::KeyL => KeyCode::KeyL,
        W::KeyM => KeyCode::KeyM,
        W::KeyN => KeyCode::KeyN,
        W::KeyO => KeyCode::KeyO,
        W::KeyP => KeyCode::KeyP,
        W::KeyQ => KeyCode::KeyQ,
        W::KeyR => KeyCode::KeyR,
        W::KeyS => KeyCode::KeyS,
        W::KeyT => KeyCode::KeyT,
        W::KeyU => KeyCode::KeyU,
        W::KeyV => KeyCode::KeyV,
        W::KeyW => KeyCode::KeyW,
        W::KeyX => KeyCode::KeyX,
        W::KeyY => KeyCode::KeyY,
        W::KeyZ => KeyCode::KeyZ,
        W::Digit0 => KeyCode::Digit0,
        W::Digit1 => KeyCode::Digit1,
        W::Digit2 => KeyCode::Digit2,
        W::Digit3 => KeyCode::Digit3,
        W::Digit4 => KeyCode::Digit4,
        W::Digit5 => KeyCode::Digit5,
        W::Digit6 => KeyCode::Digit6,
        W::Digit7 => KeyCode::Digit7,
        W::Digit8 => KeyCode::Digit8,
        W::Digit9 => KeyCode::Digit9,
        W::F1 => KeyCode::F1,
        W::F2 => KeyCode::F2,
        W::F3 => KeyCode::F3,
        W::F4 => KeyCode::F4,
        W::F5 => KeyCode::F5,
        W::F6 => KeyCode::F6,
        W::F7 => KeyCode::F7,
        W::F8 => KeyCode::F8,
        W::F9 => KeyCode::F9,
        W::F10 => KeyCode::F10,
        W::F11 => KeyCode::F11,
        W::F12 => KeyCode::F12,
        W::ShiftLeft => KeyCode::ShiftLeft,
        W::ShiftRight => KeyCode::ShiftRight,
        W::ControlLeft => KeyCode::ControlLeft,
        W::ControlRight => KeyCode::ControlRight,
        W::AltLeft => KeyCode::AltLeft,
        W::AltRight => KeyCode::AltRight,
        W::SuperLeft => KeyCode::SuperLeft,
        W::SuperRight => KeyCode::SuperRight,
        W::CapsLock => KeyCode::CapsLock,
        W::Minus => KeyCode::Minus,
        W::Equal => KeyCode::Equal,
        W::Backquote => KeyCode::Backquote,
        W::Comma => KeyCode::Comma,
        W::Period => KeyCode::Period,
        W::Slash => KeyCode::Slash,
        W::Backslash => KeyCode::Backslash,
        W::Semicolon => KeyCode::Semicolon,
        W::Quote => KeyCode::Quote,
        W::BracketLeft => KeyCode::BracketLeft,
        W::BracketRight => KeyCode::BracketRight,
        W::Numpad0 => KeyCode::Numpad0,
        W::Numpad1 => KeyCode::Numpad1,
        W::Numpad2 => KeyCode::Numpad2,
        W::Numpad3 => KeyCode::Numpad3,
        W::Numpad4 => KeyCode::Numpad4,
        W::Numpad5 => KeyCode::Numpad5,
        W::Numpad6 => KeyCode::Numpad6,
        W::Numpad7 => KeyCode::Numpad7,
        W::Numpad8 => KeyCode::Numpad8,
        W::Numpad9 => KeyCode::Numpad9,
        W::NumpadAdd => KeyCode::NumpadAdd,
        W::NumpadSubtract => KeyCode::NumpadSubtract,
        W::NumpadMultiply => KeyCode::NumpadMultiply,
        W::NumpadDivide => KeyCode::NumpadDivide,
        W::NumpadEnter => KeyCode::NumpadEnter,
        W::NumpadDecimal => KeyCode::NumpadDecimal,
        other => KeyCode::Other(other as u32),
    }
}

/// 鼠标按键
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    Other(u16),
}

/// 修饰键快照（由引擎从 `ModifiersChanged` 事件维护）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub win: bool,
}

/// 键盘状态表（引擎从事件流维护，轮询式输入查询用）
#[derive(Debug, Default)]
pub struct KeyboardState {
    pressed: HashSet<KeyCode>,
    modifiers: KeyModifiers,
}

impl KeyboardState {
    /// 指定键当前是否按住
    pub fn is_pressed(&self, key: KeyCode) -> bool {
        self.pressed.contains(&key)
    }

    /// 当前按住的键（无序）
    pub fn pressed(&self) -> impl Iterator<Item = KeyCode> + '_ {
        self.pressed.iter().copied()
    }

    /// 修饰键快照
    pub fn modifiers(&self) -> KeyModifiers {
        self.modifiers
    }

    pub(crate) fn press(&mut self, key: KeyCode) {
        self.pressed.insert(key);
    }

    pub(crate) fn release(&mut self, key: KeyCode) {
        self.pressed.remove(&key);
    }

    pub(crate) fn set_modifiers(&mut self, m: KeyModifiers) {
        self.modifiers = m;
    }
}

/// 鼠标状态表（引擎从事件流维护）
#[derive(Debug, Default)]
pub struct MouseState {
    pressed: HashSet<MouseButton>,
    position: (f64, f64),
}

impl MouseState {
    /// 指定按键当前是否按住
    pub fn is_pressed(&self, button: MouseButton) -> bool {
        self.pressed.contains(&button)
    }

    /// 光标位置（窗口物理坐标）
    pub fn position(&self) -> (f64, f64) {
        self.position
    }

    pub(crate) fn press(&mut self, b: MouseButton) {
        self.pressed.insert(b);
    }

    pub(crate) fn release(&mut self, b: MouseButton) {
        self.pressed.remove(&b);
    }

    pub(crate) fn set_position(&mut self, x: f64, y: f64) {
        self.position = (x, y);
    }
}

/// 窗口事件（平台中立；由后端翻译）
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum WindowEvent {
    /// 用户请求关闭（点 × 等）。v1 语义：收到即退出，不可否决
    CloseRequested,
    /// 窗口尺寸变化（物理像素）；渲染表面需在此 resize
    Resized { width: u32, height: u32 },
    /// 键盘焦点得失
    Focused(bool),
    /// 按键按下（物理键码，含系统重复）
    KeyPressed(KeyCode),
    /// 按键松开
    KeyReleased(KeyCode),
    /// 文本输入字符（由输入法/布局产生的最终文本，逐字符派发）
    TextInput(char),
    /// 鼠标移动（窗口物理坐标）
    MouseMoved { x: f64, y: f64 },
    /// 鼠标按下
    MousePressed(MouseButton),
    /// 鼠标松开
    MouseReleased(MouseButton),
    /// 滚轮滚动（单位：行；PixelDelta 按 1 行 = 40px 折算）
    MouseWheel { x: f32, y: f32 },
}
