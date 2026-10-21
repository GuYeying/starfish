//! Platform message → [`Event`] translation (compiled only under the
//! `event` feature).
//! 平台消息 → [`Event`] 翻译（仅 `event` feature 编译）。
//!
//! [`handle_message`] is called by the parent module's wndproc:
//! `Some(LRESULT)` means handled (wndproc returns that result directly),
//! `None` falls back to default system processing. Private fields of the
//! parent's `WndState` are visible to this child module.
//! [`handle_message`] 由父模块的 wndproc 调用：返回 `Some(LRESULT)` 表示
//! 已处理（wndproc 直接返回该结果），`None` 表示回落到系统默认处理。
//! 父模块 `WndState` 的私有字段对本子模块可见。

use std::mem;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi as gdi;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{self as km, ReleaseCapture, SetCapture};
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

use super::WndState;
use crate::event::{Event, KeyCode, Modifiers, MouseButton};

/// Virtual key code → [`KeyCode`]: letters / digits / F-keys / numpad
/// digits go through ranges + constant tables, the rest map item by item;
/// unrecognized keys become [`KeyCode::Other`] (carrying the native VK
/// code) so no event is lost.
/// 虚拟键码 → [`KeyCode`]：字母 / 数字 / F 键 / 小键盘数字用区间 + 常量表，
/// 其余逐项映射；未识别的键包成 [`KeyCode::Other`]（携带原生 VK 码），不丢事件。
pub(super) fn vk_to_keycode(vk: u32) -> KeyCode {
    // windows-sys's VK_* constants are u16; a VK code only occupies the
    // low 16 bits.
    // windows-sys 的 VK_* 常量是 u16；VK 码只占低 16 位。
    let vk = vk as u16;
    match vk {
        0x41..=0x5A => LETTERS[(vk - 0x41) as usize],
        0x30..=0x39 => DIGITS[(vk - 0x30) as usize],
        0x70..=0x7B => FKEYS[(vk - 0x70) as usize],
        0x60..=0x69 => NUMPAD[(vk - 0x60) as usize],
        km::VK_BACK => KeyCode::Backspace,
        km::VK_TAB => KeyCode::Tab,
        // Numpad Enter is not distinguished (lparam extended bit); mapped
        // to Enter uniformly in the first version.
        // 小键盘 Enter 不区分（lparam 扩展位），首版统一映射 Enter。
        km::VK_RETURN => KeyCode::Enter,
        km::VK_ESCAPE => KeyCode::Escape,
        km::VK_SPACE => KeyCode::Space,
        km::VK_CAPITAL => KeyCode::CapsLock,
        km::VK_LSHIFT => KeyCode::ShiftLeft,
        km::VK_RSHIFT => KeyCode::ShiftRight,
        km::VK_LCONTROL => KeyCode::ControlLeft,
        km::VK_RCONTROL => KeyCode::ControlRight,
        km::VK_LMENU => KeyCode::AltLeft,
        km::VK_RMENU => KeyCode::AltRight,
        km::VK_LWIN => KeyCode::MetaLeft,
        km::VK_RWIN => KeyCode::MetaRight,
        km::VK_APPS => KeyCode::ContextMenu,
        km::VK_LEFT => KeyCode::ArrowLeft,
        km::VK_RIGHT => KeyCode::ArrowRight,
        km::VK_UP => KeyCode::ArrowUp,
        km::VK_DOWN => KeyCode::ArrowDown,
        km::VK_HOME => KeyCode::Home,
        km::VK_END => KeyCode::End,
        km::VK_PRIOR => KeyCode::PageUp,
        km::VK_NEXT => KeyCode::PageDown,
        km::VK_INSERT => KeyCode::Insert,
        km::VK_DELETE => KeyCode::Delete,
        km::VK_OEM_MINUS => KeyCode::Minus,
        km::VK_OEM_PLUS => KeyCode::Equal,
        km::VK_OEM_4 => KeyCode::BracketLeft,
        km::VK_OEM_6 => KeyCode::BracketRight,
        km::VK_OEM_5 => KeyCode::Backslash,
        km::VK_OEM_1 => KeyCode::Semicolon,
        km::VK_OEM_7 => KeyCode::Quote,
        km::VK_OEM_3 => KeyCode::Backquote,
        km::VK_OEM_COMMA => KeyCode::Comma,
        km::VK_OEM_PERIOD => KeyCode::Period,
        km::VK_OEM_2 => KeyCode::Slash,
        km::VK_ADD => KeyCode::NumpadAdd,
        km::VK_SUBTRACT => KeyCode::NumpadSubtract,
        km::VK_MULTIPLY => KeyCode::NumpadMultiply,
        km::VK_DIVIDE => KeyCode::NumpadDivide,
        km::VK_DECIMAL => KeyCode::NumpadDecimal,
        km::VK_SNAPSHOT => KeyCode::PrintScreen,
        km::VK_SCROLL => KeyCode::ScrollLock,
        km::VK_PAUSE => KeyCode::Pause,
        km::VK_NUMLOCK => KeyCode::NumLock,
        other => KeyCode::Other(other as u32),
    }
}

const LETTERS: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

const DIGITS: [KeyCode; 10] = [
    KeyCode::Digit0,
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

// VK_F1 (0x70) … VK_F12 (0x7B) are a contiguous range.
// VK_F1 (0x70) … VK_F12 (0x7B) 连续区间。
const FKEYS: [KeyCode; 12] = [
    KeyCode::F1,
    KeyCode::F2,
    KeyCode::F3,
    KeyCode::F4,
    KeyCode::F5,
    KeyCode::F6,
    KeyCode::F7,
    KeyCode::F8,
    KeyCode::F9,
    KeyCode::F10,
    KeyCode::F11,
    KeyCode::F12,
];

// VK_NUMPAD0 (0x60) … VK_NUMPAD9 (0x69) 连续区间。
const NUMPAD: [KeyCode; 10] = [
    KeyCode::Numpad0,
    KeyCode::Numpad1,
    KeyCode::Numpad2,
    KeyCode::Numpad3,
    KeyCode::Numpad4,
    KeyCode::Numpad5,
    KeyCode::Numpad6,
    KeyCode::Numpad7,
    KeyCode::Numpad8,
    KeyCode::Numpad9,
];

/// Translates one window message; `Some` = handled.
/// 翻译一条窗口消息；`Some` = 已处理。
pub(super) fn handle_message(
    state: &mut WndState,
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    match msg {
        // Destruction is decided by the user dropping the Window; only the
        // intent is reported here.
        // 是否销毁由用户 drop Window 决定；这里只上报意图。
        wm::WM_CLOSE => {
            state.push(Event::CloseRequested);
            Some(0)
        }
        wm::WM_SIZE => {
            let width = (lparam & 0xFFFF) as u16 as u32;
            let height = ((lparam >> 16) & 0xFFFF) as u16 as u32;
            if wparam as u32 != wm::SIZE_MINIMIZED {
                state.push(Event::Resized { width, height });
            }
            Some(0)
        }
        wm::WM_PAINT => {
            // Validate the invalid region (we have no GDI content) and hand
            // the redraw timing to the user.
            // 校验无效区（我们没有 GDI 内容），并把重绘时机交给用户。
            // SAFETY: hwnd comes from the system callback; ps is a local of
            // this function.
            // SAFETY: hwnd 来自系统回调，ps 为本函数局部变量。
            unsafe {
                let mut ps: gdi::PAINTSTRUCT = mem::zeroed();
                let hdc = gdi::BeginPaint(hwnd, &mut ps);
                if !hdc.is_null() {
                    gdi::EndPaint(hwnd, &ps);
                }
            }
            state.push(Event::RedrawRequested);
            Some(0)
        }
        wm::WM_SETFOCUS => {
            state.push(Event::FocusGained);
            Some(0)
        }
        wm::WM_KILLFOCUS => {
            state.push(Event::FocusLost);
            Some(0)
        }
        wm::WM_CHAR => {
            // UTF-16 unit passthrough: control characters are skipped (Tab
            // kept); surrogate pair units fail char::from_u32 and are
            // dropped automatically (supplementary-plane characters are not
            // supported in the first version).
            // UTF-16 单元直通：跳过控制字符（保留 Tab）；代理对单元来自
            // char::from_u32 失败自动丢弃（补充平面字符首版不支持）。
            if let Some(ch) = char::from_u32(wparam as u32) {
                let c = ch as u32;
                if c == 0x09 || (0x20..0x7F).contains(&c) || c >= 0xA0 {
                    state.push(Event::TextInput {
                        text: ch.to_string(),
                    });
                }
            }
            Some(0)
        }
        wm::WM_KEYDOWN | wm::WM_SYSKEYDOWN => {
            let key = vk_to_keycode(wparam as u32);
            let repeat = lparam & (1 << 30) != 0;
            state.push(Event::KeyDown {
                key,
                modifiers: current_modifiers(),
                repeat,
            });
            // Hand back to the system: Alt+F4 → WM_CLOSE, menu mnemonics
            // and friends keep their default behavior.
            // 交给系统：Alt+F4 → WM_CLOSE、菜单助记符等保持默认行为。
            None
        }
        wm::WM_KEYUP | wm::WM_SYSKEYUP => {
            let key = vk_to_keycode(wparam as u32);
            state.push(Event::KeyUp {
                key,
                modifiers: current_modifiers(),
            });
            None
        }
        wm::WM_MOUSEMOVE => {
            state.push(Event::MouseMoved {
                x: get_x_lparam(lparam) as f64,
                y: get_y_lparam(lparam) as f64,
            });
            Some(0)
        }
        wm::WM_LBUTTONDOWN => {
            state.mouse_press(MouseButton::Left);
            Some(0)
        }
        wm::WM_RBUTTONDOWN => {
            state.mouse_press(MouseButton::Right);
            Some(0)
        }
        wm::WM_MBUTTONDOWN => {
            state.mouse_press(MouseButton::Middle);
            Some(0)
        }
        wm::WM_XBUTTONDOWN => {
            state.mouse_press(xbutton(wparam));
            Some(0)
        }
        wm::WM_LBUTTONUP => {
            state.mouse_release(MouseButton::Left);
            Some(0)
        }
        wm::WM_RBUTTONUP => {
            state.mouse_release(MouseButton::Right);
            Some(0)
        }
        wm::WM_MBUTTONUP => {
            state.mouse_release(MouseButton::Middle);
            Some(0)
        }
        wm::WM_XBUTTONUP => {
            state.mouse_release(xbutton(wparam));
            Some(0)
        }
        wm::WM_MOUSEWHEEL | wm::WM_MOUSEHWHEEL => {
            let delta = ((wparam >> 16) & 0xFFFF) as u16 as i16 as f64 / wm::WHEEL_DELTA as f64;
            let event = if msg == wm::WM_MOUSEWHEEL {
                Event::MouseWheel {
                    delta_x: 0.0,
                    delta_y: delta,
                }
            } else {
                Event::MouseWheel {
                    delta_x: delta,
                    delta_y: 0.0,
                }
            };
            state.push(event);
            Some(0)
        }
        _ => None,
    }
}

impl WndState {
    pub(super) fn push(&mut self, event: Event) {
        self.pending.push_back(event);
    }

    pub(super) fn mouse_press(&mut self, button: MouseButton) {
        let bit = button_bit(button);
        let was_up = self.mouse_down & bit == 0;
        self.mouse_down |= bit;
        if was_up {
            // SAFETY: hwnd is valid (the message comes from it); drags
            // outside the window keep producing move events.
            // SAFETY: hwnd 有效（消息正来自它）；拖拽出窗口仍能收到移动事件。
            unsafe { SetCapture(self.hwnd) };
        }
        self.push(Event::MouseDown { button });
    }

    pub(super) fn mouse_release(&mut self, button: MouseButton) {
        self.mouse_down &= !button_bit(button);
        if self.mouse_down == 0 {
            // SAFETY: paired with the SetCapture in mouse_press.
            // SAFETY: 与 mouse_press 中的 SetCapture 配对。
            unsafe { ReleaseCapture() };
        }
        self.push(Event::MouseUp { button });
    }
}

fn get_x_lparam(lparam: LPARAM) -> i16 {
    (lparam & 0xFFFF) as u16 as i16
}

fn get_y_lparam(lparam: LPARAM) -> i16 {
    ((lparam >> 16) & 0xFFFF) as u16 as i16
}

fn xbutton(wparam: WPARAM) -> MouseButton {
    if ((wparam >> 16) & 0xFFFF) as u16 == wm::XBUTTON1 {
        MouseButton::Back
    } else {
        MouseButton::Forward
    }
}

/// Modifier state is taken from the message-queue state at the time of the
/// message (`GetKeyState` semantics).
/// 修饰键状态取自消息时刻的消息队列状态（`GetKeyState` 语义）。
fn current_modifiers() -> Modifiers {
    fn down(vk: i32) -> bool {
        // SAFETY: VK_* are all valid virtual key codes.
        // SAFETY: VK_* 均为合法虚拟键码。
        unsafe { km::GetKeyState(vk) as u16 & 0x8000 != 0 }
    }
    Modifiers {
        shift: down(km::VK_SHIFT as i32),
        ctrl: down(km::VK_CONTROL as i32),
        alt: down(km::VK_MENU as i32),
        meta: down(km::VK_LWIN as i32) || down(km::VK_RWIN as i32),
    }
}

fn button_bit(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 1 << 0,
        MouseButton::Right => 1 << 1,
        MouseButton::Middle => 1 << 2,
        MouseButton::Back => 1 << 3,
        MouseButton::Forward => 1 << 4,
        MouseButton::Other(_) => 0,
    }
}
