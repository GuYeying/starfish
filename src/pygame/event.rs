//! pygame.event 对应物（契约 ADR-7：`poll_event` → pygame 词汇薄翻译）
//!
//! `get`/`poll`/`clear` 对齐 pygame 语义（get = 排空；fastevent 不做——
//! 我们的 get 本就是直接排空）。翻译时**顺带维护 key/mouse 状态表**
//! （ADR-7：引擎事件是唯一事实源，不引入额外维护对象）。
//!
//! 事件词汇 = [`Event`] 枚举（类型化变体，Rust 侧 match 解构——整数
//! 常量不做，枚举即词汇）。

use crate::base::window::{Event as WindowEvent, KeyCode, TouchPhase};
use crate::pygame::{display, key, mouse};

/// pygame 事件（薄翻译自引擎 [`WindowEvent`]）
#[derive(Debug, Clone)]
pub enum Event {
    /// 关闭请求（窗口 × / 系统关闭）
    Quit,
    /// 窗口尺寸变化（物理像素；ADR-8：display 尺寸不跟随）
    Resized { width: u32, height: u32 },
    FocusGained,
    FocusLost,
    /// 按键按下（`key` = [`KeyCode`]，与 [`crate::pygame::locals`] 常量同型）
    KeyDown { key: KeyCode, repeat: bool },
    KeyUp { key: KeyCode },
    /// 键盘最终文本输入（与 KeyDown 相互独立）
    TextInput { text: String },
    /// 鼠标移动（客户区物理像素）
    MouseMotion { x: i32, y: i32 },
    /// 鼠标键按下（button：1=左 2=中 3=右，pygame 惯例）
    MouseButtonDown { button: u8, x: i32, y: i32 },
    MouseButtonUp { button: u8, x: i32, y: i32 },
    /// 滚轮（正值向上/向右）
    MouseWheel { x: i32, y: i32 },
    /// 触摸（仅触屏设备；移动端后端已把主手指合成鼠标事件）
    Touch { phase: TouchPhase, finger: u64, x: i32, y: i32 },
    /// 进后台 / 表面失效（移动端与 Web；期间不得渲染）
    Suspend,
    /// 回前台 / 可继续渲染
    Resume,
    /// 未翻译变体兜底（RedrawRequested / User 等不丢不炸）
    Other,
}

/// 取自上次调用以来的全部事件（排空队列；同时喂 key/mouse 状态表）
pub fn get() -> Vec<Event> {
    let mut out = Vec::new();
    while let Some(e) = poll() {
        out.push(e);
    }
    out
}

/// 取下一个事件（无则 None）
pub fn poll() -> Option<Event> {
    crate::pygame::display::with_window(|w| w.poll_event().and_then(translate)).flatten()
}

/// 清空队列（状态表照常喂——按键抬起不可丢，pygame clear 同语义）
pub fn clear() {
    while poll().is_some() {}
}

/// 引擎事件 → pygame 事件（返回 None = 不上报给用户）。
/// 翻译时**顺带喂 key/mouse 状态表**（ADR-7：引擎事件 = 唯一事实源）。
fn translate(e: WindowEvent) -> Option<Event> {
    match e {
        WindowEvent::CloseRequested => Some(Event::Quit),
        WindowEvent::Resized { width, height } => {
            // 批次二十接线：渲染表面/正交相机自动跟随物理尺寸（Android
            // 旋转重建 ANativeWindow 的自愈路径；桌面拖拽窗口同享）。
            // ADR-8：display 逻辑尺寸不跟随。
            display::handle_resized(width, height);
            Some(Event::Resized { width, height })
        }
        WindowEvent::FocusGained => {
            key::set_focused(true);
            Some(Event::FocusGained)
        }
        WindowEvent::FocusLost => {
            key::set_focused(false);
            Some(Event::FocusLost)
        }
        WindowEvent::KeyDown { key, modifiers, repeat } => {
            key::press(key);
            key::set_mods(modifiers);
            Some(Event::KeyDown { key, repeat })
        }
        WindowEvent::KeyUp { key, modifiers } => {
            key::release(key);
            key::set_mods(modifiers);
            Some(Event::KeyUp { key })
        }
        WindowEvent::TextInput { text } => Some(Event::TextInput { text }),
        WindowEvent::MouseMoved { x, y } => {
            let (x, y) = (x as i32, y as i32);
            mouse::set_pos(x, y);
            Some(Event::MouseMotion { x, y })
        }
        WindowEvent::MouseDown { button } => {
            let (b, (x, y)) = (mouse_button(button), mouse::get_pos());
            mouse::press(b);
            Some(Event::MouseButtonDown { button: b, x, y })
        }
        WindowEvent::MouseUp { button } => {
            let (b, (x, y)) = (mouse_button(button), mouse::get_pos());
            mouse::release(b);
            Some(Event::MouseButtonUp { button: b, x, y })
        }
        WindowEvent::MouseWheel { delta_x, delta_y } => Some(Event::MouseWheel {
            x: delta_x as i32,
            y: delta_y as i32,
        }),
        WindowEvent::Touch { phase, finger, x, y } => {
            crate::pygame::touch::on_touch(phase, finger, x as i32, y as i32);
            Some(Event::Touch {
                phase,
                finger,
                x: x as i32,
                y: y as i32,
            })
        }
        WindowEvent::Suspended => Some(Event::Suspend),
        WindowEvent::Resumed => Some(Event::Resume),
        // RedrawRequested（循环恒每帧 present）/ User(_) 不上报
        _ => Some(Event::Other),
    }
}

/// 引擎鼠标键 → pygame 惯例编号（1=左 2=中 3=右 4=后 5=前）
fn mouse_button(b: crate::base::window::MouseButton) -> u8 {
    use crate::base::window::MouseButton as Mb;
    match b {
        Mb::Left => 1,
        Mb::Middle => 2,
        Mb::Right => 3,
        Mb::Back => 4,
        Mb::Forward => 5,
        _ => 0, // non_exhaustive 兜底（新增键不炸翻译层）
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::window::MouseButton;

    #[test]
    fn translation_maps_core_events() {
        assert!(matches!(translate(WindowEvent::CloseRequested), Some(Event::Quit)));

        let ev = translate(WindowEvent::KeyDown {
            key: KeyCode::ArrowLeft,
            modifiers: Default::default(),
            repeat: false,
        });
        assert!(matches!(ev, Some(Event::KeyDown { key: KeyCode::ArrowLeft, repeat: false })));
        assert!(key::get_pressed().left(), "KeyDown 应喂进状态表");
        key::release(KeyCode::ArrowLeft);

        let ev = translate(WindowEvent::MouseMoved { x: 12.0, y: 34.0 });
        assert!(matches!(ev, Some(Event::MouseMotion { x: 12, y: 34 })));
        assert_eq!(mouse::get_pos(), (12, 34), "MouseMoved 应喂进位置表");

        let ev = translate(WindowEvent::MouseDown { button: MouseButton::Left });
        assert!(matches!(ev, Some(Event::MouseButtonDown { button: 1, x: 12, y: 34 })));
        assert_eq!(mouse::get_pressed(), (true, false, false));
        mouse::release(1);
    }

    #[test]
    fn unknown_variants_fall_back() {
        assert!(matches!(translate(WindowEvent::RedrawRequested), Some(Event::Other)));
    }
}
