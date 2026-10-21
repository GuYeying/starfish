//! Event types (platform-independent).
//! 事件类型定义（平台无关）。

use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// The set of modifier keys currently held down.
/// 当前按下的修饰键集合。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Shift.
    /// Shift。
    pub shift: bool,
    /// Ctrl (Control on macOS).
    /// Ctrl（macOS Control）。
    pub ctrl: bool,
    /// Alt (Option on macOS).
    /// Alt（macOS Option）。
    pub alt: bool,
    /// Windows logo key / macOS Command.
    /// Windows 徽标键 / macOS Command。
    pub meta: bool,
}

/// A mouse button. Variant names are self-explanatory (left / right /
/// middle / back / forward); individual variants are not annotated.
/// 鼠标按键。变体名自解释（左 / 右 / 中 / 前进 / 后退），不再逐项注释。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Back,
    Forward,
    /// An unmapped extra button (carries the native button index).
    /// 未映射的额外按键（附原生按钮序号）。
    Other(u8),
}

/// A physical key, named after the W3C `KeyboardEvent.code` (layout
/// independent). Text content (including IME) is delivered via
/// [`Event::TextInput`], not here. Variant names are self-explanatory
/// (`KeyA` = letter A, `Digit3` = top-row 3, `Numpad5` = keypad 5);
/// individual variants are not annotated.
/// 物理按键，按 W3C `KeyboardEvent.code` 命名（与键位布局无关）。
/// 文本内容（含 IME）走 [`Event::TextInput`]，不在这里表达。
/// 变体名即含义（`KeyA` = 字母 A 键、`Digit3` = 主键盘数字 3、
/// `Numpad5` = 小键盘 5），不逐项注释。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[allow(missing_docs)]
pub enum KeyCode {
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
    Escape,
    Tab,
    CapsLock,
    Space,
    Enter,
    Backspace,
    ShiftLeft,
    ShiftRight,
    ControlLeft,
    ControlRight,
    AltLeft,
    AltRight,
    MetaLeft,
    MetaRight,
    ContextMenu,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Minus,
    Equal,
    BracketLeft,
    BracketRight,
    Backslash,
    Semicolon,
    Quote,
    Backquote,
    Comma,
    Period,
    Slash,
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
    PrintScreen,
    ScrollLock,
    Pause,
    NumLock,
    /// An unmapped key (carries the native key code).
    /// 未映射的按键（附原生键码）。
    Other(u32),
}

/// The phase of a touch interaction.
/// 触摸阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TouchPhase {
    /// Finger down.
    /// 手指按下。
    Started,
    /// Finger moving.
    /// 手指移动。
    Moved,
    /// Finger up.
    /// 手指抬起。
    Ended,
    /// Cancelled by the system (incoming call, gesture takeover, …).
    /// 系统取消（来电、手势接管等）。
    Cancelled,
}

/// The W3C `code` mapping is the Web backend's vocabulary bridge (the key
/// naming is derived from W3C in the first place).
/// W3C `code` 映射是 Web 后端的词汇桥（键盘命名本就同源于 W3C）。
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl KeyCode {
    /// W3C `KeyboardEvent.code` string → [`KeyCode`].
    ///
    /// The two namings share the same origin (this enum is defined after the
    /// W3C names), so this is a purely mechanical mapping. Returns `None`
    /// for unrecognized codes (the Web backend falls back to
    /// [`KeyCode::Other`]).
    /// W3C `KeyboardEvent.code` 字符串 → [`KeyCode`]。
    /// 两者命名同源（本枚举即按 W3C 命名定义），此处做纯机械映射；
    /// 未识别的 code 返回 `None`（Web 后端兜底为 `KeyCode::Other`）。
    pub(crate) fn from_w3c_code(code: &str) -> Option<KeyCode> {
        use KeyCode as K;
        const LETTERS: [KeyCode; 26] = [
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
        const DIGITS: [KeyCode; 10] = [
            K::Digit0,
            K::Digit1,
            K::Digit2,
            K::Digit3,
            K::Digit4,
            K::Digit5,
            K::Digit6,
            K::Digit7,
            K::Digit8,
            K::Digit9,
        ];
        const FKEYS: [KeyCode; 12] = [
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
        const NUMPAD: [KeyCode; 10] = [
            K::Numpad0,
            K::Numpad1,
            K::Numpad2,
            K::Numpad3,
            K::Numpad4,
            K::Numpad5,
            K::Numpad6,
            K::Numpad7,
            K::Numpad8,
            K::Numpad9,
        ];
        const NAMED: &[(&str, KeyCode)] = &[
            ("Escape", K::Escape),
            ("Tab", K::Tab),
            ("CapsLock", K::CapsLock),
            ("Space", K::Space),
            ("Enter", K::Enter),
            ("Backspace", K::Backspace),
            ("ShiftLeft", K::ShiftLeft),
            ("ShiftRight", K::ShiftRight),
            ("ControlLeft", K::ControlLeft),
            ("ControlRight", K::ControlRight),
            ("AltLeft", K::AltLeft),
            ("AltRight", K::AltRight),
            ("MetaLeft", K::MetaLeft),
            ("MetaRight", K::MetaRight),
            ("ContextMenu", K::ContextMenu),
            ("ArrowLeft", K::ArrowLeft),
            ("ArrowRight", K::ArrowRight),
            ("ArrowUp", K::ArrowUp),
            ("ArrowDown", K::ArrowDown),
            ("Home", K::Home),
            ("End", K::End),
            ("PageUp", K::PageUp),
            ("PageDown", K::PageDown),
            ("Insert", K::Insert),
            ("Delete", K::Delete),
            ("Minus", K::Minus),
            ("Equal", K::Equal),
            ("BracketLeft", K::BracketLeft),
            ("BracketRight", K::BracketRight),
            ("Backslash", K::Backslash),
            ("Semicolon", K::Semicolon),
            ("Quote", K::Quote),
            ("Backquote", K::Backquote),
            ("Comma", K::Comma),
            ("Period", K::Period),
            ("Slash", K::Slash),
            ("NumpadAdd", K::NumpadAdd),
            ("NumpadSubtract", K::NumpadSubtract),
            ("NumpadMultiply", K::NumpadMultiply),
            ("NumpadDivide", K::NumpadDivide),
            ("NumpadEnter", K::NumpadEnter),
            ("NumpadDecimal", K::NumpadDecimal),
            ("PrintScreen", K::PrintScreen),
            ("ScrollLock", K::ScrollLock),
            ("Pause", K::Pause),
            ("NumLock", K::NumLock),
        ];

        if let Some(rest) = code.strip_prefix("Key") {
            if rest.len() == 1 {
                let c = rest.as_bytes()[0];
                if c.is_ascii_uppercase() {
                    return Some(LETTERS[(c - b'A') as usize]);
                }
            }
            return None;
        }
        if let Some(rest) = code.strip_prefix("Digit") {
            if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
                return Some(DIGITS[(rest.as_bytes()[0] - b'0') as usize]);
            }
            return None;
        }
        if let Some(rest) = code.strip_prefix('F') {
            if let Ok(n) = rest.parse::<usize>() {
                if (1..=12).contains(&n) {
                    return Some(FKEYS[n - 1]);
                }
            }
            return None;
        }
        if let Some(rest) = code.strip_prefix("Numpad") {
            if rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit() {
                return Some(NUMPAD[(rest.as_bytes()[0] - b'0') as usize]);
            }
        }
        NAMED
            .iter()
            .find(|(name, _)| *name == code)
            .map(|(_, key)| *key)
    }
}

/// A host-injected custom event — a typed replacement for SDL's
/// `SDL_RegisterEvents` + `SDL_PushEvent` (payload is `Any + Send` instead
/// of two raw pointers).
/// 宿主注入的自定义事件（SDL `SDL_RegisterEvents` + `SDL_PushEvent`
/// 的类型化替代：载荷是 `Any + Send` 而非两个裸指针）。
///
/// Injection entry points:
/// [`Window::push_event`](crate::Window::push_event) (same thread) and
/// [`EventProxy`](crate::EventProxy) (cross-thread). Delivered as
/// `Event::User`.
/// 注入入口：[`Window::push_event`](crate::Window::push_event)（同线程）、
/// [`EventProxy`](crate::EventProxy)（跨线程）。经 `Event::User` 派发。
///
/// # 示例 / Example
/// ```ignore
/// // On any thread / 任意线程：
/// proxy.send(1, "script message".to_string())?;
/// // Main loop / 主循环：
/// if let Event::User(user) = event {
///     if user.code == 1 {
///         let msg = user.downcast_ref::<String>().unwrap();
///     }
/// }
/// ```
///
/// `Event: Clone` shares the payload: cloning an event does not copy the
/// payload (reference counted) — well suited for re-dispatching to multiple
/// subscribers inside the interpreter.
/// `Event: Clone` 与载荷共享：克隆事件不复制载荷（引用计数），
/// 适合把事件再分发给解释器内的多个订阅者。
#[derive(Clone)]
pub struct UserEvent {
    /// The event category code (host-defined semantics: 1 = timer,
    /// 2 = script message, 3 = task finished, …).
    /// 事件类别码（宿主自定义语义，如 1=定时器、2=脚本消息、3=任务完成）。
    pub code: u32,
    /// The payload is reference-counted (`Event: Clone` shares the same
    /// payload — broadcast semantics).
    /// 载荷按引用计数共享（`Event: Clone` 即共享同一载荷，广播语义）。
    payload: Arc<dyn Any + Send + Sync>,
}

impl UserEvent {
    /// Creates a custom event.
    /// 构造一条自定义事件。
    pub fn new(code: u32, payload: impl Any + Send + Sync) -> Self {
        Self {
            code,
            payload: Arc::new(payload),
        }
    }

    /// Borrows the payload and downcasts it.
    /// 借取载荷并向下转型。
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        let any: &dyn Any = self.payload.as_ref();
        any.downcast_ref()
    }

    /// Mutably borrows the payload and downcasts it (only succeeds when no
    /// other sharer exists — typically the single-consumer case where the
    /// sender no longer holds a handle).
    /// 可变借取载荷并向下转型（仅当载荷无其他共享者时成功——
    /// 通常即"发送者未再持有"的单消费场景）。
    pub fn downcast_mut<T: Any>(&mut self) -> Option<&mut T> {
        Arc::get_mut(&mut self.payload)?.downcast_mut()
    }

    /// Takes ownership of the payload and downcasts it. Returns
    /// `Err(self)` on a type mismatch, or when other clones still share
    /// the payload.
    /// 取回载荷所有权并向下转型（载荷类型不符时原样返回 `Err(self)`；
    /// 存在其他克隆共享者时无法取出，同样返回 `Err`）。
    pub fn downcast<T: Any + Send + Sync>(self) -> Result<Arc<T>, Self> {
        match Arc::downcast(self.payload) {
            Ok(payload) => Ok(payload),
            Err(payload) => Err(Self {
                code: self.code,
                payload,
            }),
        }
    }
}

impl fmt::Debug for UserEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The payload is not printable (dyn Any); expose the code only.
        // 载荷不可打印（dyn Any），只暴露类别码。
        f.debug_struct("UserEvent")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

/// A single window event. SDL style: pulled one at a time via
/// [`Window::poll_event`](crate::Window::poll_event); `None` when the
/// queue is empty.
/// 单个窗口事件。SDL 风格：由 [`Window::poll_event`](crate::Window::poll_event)
/// 逐个拉取，队列为空时返回 `None`。
///
/// Mouse coordinates are physical pixels within the window client area,
/// origin at the top-left (logical-pixel / DPI-scaled APIs are left for
/// later versions).
/// 鼠标坐标为窗口客户区内的物理像素，原点在左上角
/// （逻辑像素 / DPI 缩放 API 留待后续版本）。
///
/// `PartialEq` is intentionally not implemented: the `User` variant carries
/// a non-comparable dynamic payload. Match events with `match`/`matches!`.
/// 不实现 `PartialEq`：`User` 变体携带不可比较的动态载荷，
/// 事件匹配请用 `match` / `matches!`。
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Event {
    /// The user requested to close the window (system close button /
    /// close shortcut). End the main loop and drop
    /// [`Window`](crate::Window) upon receiving it.
    /// 用户请求关闭窗口（系统关闭按钮 / 关闭快捷键）。
    /// 收到后应结束主循环并 drop [`Window`](crate::Window)。
    CloseRequested,
    /// The client-area size changed (physical pixels; includes
    /// un-minimize and DPI-change resizes).
    /// 窗口客户区尺寸变化（物理像素；含最小化恢复、DPI 变化引发的 resize）。
    Resized {
        /// New width.
        /// 新宽度。
        width: u32,
        /// New height.
        /// 新高度。
        height: u32,
    },
    /// The window gained keyboard focus.
    /// 窗口获得键盘焦点。
    FocusGained,
    /// The window lost keyboard focus.
    /// 窗口失去键盘焦点。
    FocusLost,
    /// A key was pressed.
    /// 按键按下。
    KeyDown {
        /// The physical key.
        /// 物理按键。
        key: KeyCode,
        /// Modifier state at the time of the event.
        /// 事件时刻的修饰键状态。
        modifiers: Modifiers,
        /// Whether this is an auto-repeat from holding the key.
        /// 是否为按住产生的自动重复。
        repeat: bool,
    },
    /// A key was released.
    /// 按键抬起。
    KeyUp {
        /// The physical key.
        /// 物理按键。
        key: KeyCode,
        /// Modifier state at the time of the event.
        /// 事件时刻的修饰键状态。
        modifiers: Modifiers,
    },
    /// Final text produced by the keyboard (independent of `KeyDown`;
    /// no IME composition in the first version).
    /// 键盘产生的最终文本输入（与 `KeyDown` 相互独立；首版无 IME 组合过程）。
    TextInput {
        /// The text (delivered character-by-character / unit-by-unit).
        /// 输入的文本（逐字符 / 逐单元递送）。
        text: String,
    },
    /// The mouse moved within the client area (includes out-of-window
    /// coordinates while dragging).
    /// 鼠标在客户区内移动（拖拽时含窗口外坐标）。
    MouseMoved {
        /// X (physical pixels, origin at the client-area top-left).
        /// X（物理像素，客户区左上角为原点）。
        x: f64,
        /// Y.
        /// Y。
        y: f64,
    },
    /// A mouse button was pressed.
    /// 鼠标键按下。
    MouseDown {
        /// Which button.
        /// 哪个键。
        button: MouseButton,
    },
    /// A mouse button was released.
    /// 鼠标键抬起。
    MouseUp {
        /// Which button.
        /// 哪个键。
        button: MouseButton,
    },
    /// Wheel scrolling; positive is up / right; unit is a wheel detent
    /// (one line / one notch).
    /// 滚轮滚动，正值向上 / 向右；单位为滚轮齿格（一行 / 一格）。
    MouseWheel {
        /// Horizontal delta.
        /// 水平增量。
        delta_x: f64,
        /// Vertical delta.
        /// 垂直增量。
        delta_y: f64,
    },
    /// Touch input (touch-screen devices only: Android / iOS; desktop
    /// never produces it). Coordinates are physical pixels within the
    /// window/view. Mobile backends also synthesize mouse events from the
    /// primary finger (SDL convention), so desktop interaction logic can
    /// be reused as-is.
    /// 触摸（仅触屏设备产生：Android / iOS；桌面端永不产生）。
    /// 坐标为窗口 / 视图内的物理像素。移动端后端同时把主手指
    /// 合成为鼠标事件（SDL 惯例），桌面交互逻辑可直接复用。
    Touch {
        /// The touch phase.
        /// 触摸阶段。
        phase: TouchPhase,
        /// Finger id; distinguishes fingers in multi-touch.
        /// 手指 id，多点触控时区分手指。
        finger: u64,
        /// X (physical pixels).
        /// X（物理像素）。
        x: f64,
        /// Y (physical pixels).
        /// Y（物理像素）。
        y: f64,
    },
    /// The platform requests a redraw (Windows `WM_PAINT`, macOS expose,
    /// …). wgpu users usually re-`present` here.
    /// 平台要求重绘（如 Windows `WM_PAINT`、macOS expose）。
    /// wgpu 用户通常在此重新 `present`。
    RedrawRequested,
    /// The app went to the background / the rendering surface became
    /// invalid (mobile & Web only): Android surface reclaimed by the
    /// system, iOS backgrounded, Web page hidden. Do not render during
    /// this period; rebuild / re-configure the wgpu surface after
    /// [`Resumed`](Event::Resumed).
    /// 应用进入后台 / 渲染表面失效（仅移动端与 Web 产生）：
    /// Android surface 被系统回收、iOS 进后台、Web 页面隐藏。
    /// 期间不得渲染，wgpu surface 需在 [`Resumed`](Event::Resumed) 后重建 / 重新 configure。
    Suspended,
    /// The app returned to the foreground / the rendering surface is
    /// available again (mobile & Web only).
    /// 应用回到前台 / 渲染表面恢复，可以继续渲染（仅移动端与 Web 产生）。
    Resumed,
    /// A custom event injected by the host (dispatched from `push_event` /
    /// `EventProxy`).
    /// 宿主注入的自定义事件（`push_event` / `EventProxy` 的派发形态）。
    User(UserEvent),
}
