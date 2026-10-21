//! pygame.event 对应物（契约 ADR-7：`poll_event` → pygame 词汇薄翻译）
//!
//! **SDL 队列模型**：宿主帧循环每帧 [`pump`] 一次——引擎事件翻译灌入
//! 内部队列（翻译时**顺带维护 key/mouse 状态表**，ADR-7：引擎事件是
//! 唯一事实源）；`get`/`poll`/`clear` 消费同一份队列（get = 排空；
//! fastevent 不做——我们的 get 本就是直接排空）。窗口关闭请求经
//! [`pump`] 的返回值上报（pygame 的 QUIT 不含 ESC——ESC 退出是脚本
//! 层策略）。
//!
//! 事件词汇 = [`Event`] 枚举（类型化变体，Rust 侧 match 解构——整数
//! 常量不做，枚举即词汇）。

use std::cell::RefCell;
use std::collections::VecDeque;

use starfish_we::{Event as WindowEvent, KeyCode, TouchPhase, UserEvent};
use crate::{display, key, mouse};

/// pygame 事件（薄翻译自引擎 [`WindowEvent`]）
#[derive(Debug, Clone)]
pub enum Event {
    /// 关闭请求（窗口 × / 系统关闭）
    Quit,
    /// 窗口尺寸变化（物理像素；ADR-8：display 尺寸不跟随）
    Resized { width: u32, height: u32 },
    FocusGained,
    FocusLost,
    /// 按键按下（`key` = [`KeyCode`]，与 [`crate::locals`] 常量同型）
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
    /// 自定义事件（starfish-window `UserEvent` 的透传：code 由发送方
    /// 语义约定，申请用 [`custom_code`] 计数器）。**载荷不上浮**——
    /// Rust 侧 `Any` 载荷经 starfish-window 直取；Python 侧的附加
    /// 字段由 pygame-rs 包裹层自理（PyObject 不跨此层）
    User { code: u32 },
    /// 未翻译变体兜底（RedrawRequested 等不丢不炸）
    Other,
}

thread_local! {
    /// 待派发事件队列（pump 灌入，get/poll/clear 消费——SDL 模型）
    static QUEUE: RefCell<VecDeque<Event>> = RefCell::new(VecDeque::new());
}

/// 事件泵（宿主帧循环每帧一次）：把引擎事件全部翻译灌入队列（翻译时
/// 照常喂 key/mouse/touch 状态表）；窗口关闭请求返回 true。手柄每帧
/// 排水（gilrs/Web 轮询型后端，just_pressed 是帧间边沿——一帧恰好
/// 一次，归宿主帧管线）。
pub fn pump() -> bool {
    let mut quit = false;
    crate::gamepad::poll();
    while let Some(ev) = poll_engine() {
        if matches!(ev, Event::Quit) {
            quit = true;
        }
        QUEUE.with(|q| q.borrow_mut().push_back(ev));
    }
    quit
}

/// 取自上次调用以来的全部事件（排空队列；状态表已随 pump 喂过）
pub fn get() -> Vec<Event> {
    QUEUE.with(|q| q.borrow_mut().drain(..).collect())
}

/// 取下一个事件（无则 None；其余事件留在队列）
pub fn poll() -> Option<Event> {
    QUEUE.with(|q| q.borrow_mut().pop_front())
}

/// 清空队列（状态表已随 pump 喂过——按键抬起不丢，pygame clear 同语义）
pub fn clear() {
    QUEUE.with(|q| q.borrow_mut().clear());
}

// ── 自定义事件（starfish-window UserEvent 透传）──────────────────

/// 分配一个自定义事件 code（**单调递增、不复用**——starfish-window
/// 的 code 无系统资源占用，此分配器仅为"发送方约定 code"提供不冲突
/// 保证；SDL RegisterEvents 的对应物，但无上限）。0 保留为"未指定"。
pub fn custom_code() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// post_user(code)：注入一条自定义事件入队（下一次 get/poll 即可取到
/// ——同帧内 post 先于 get 亦可见；载荷由 pygame-rs 包裹层附加）。
pub fn post_user(code: u32) {
    QUEUE.with(|q| q.borrow_mut().push_back(Event::User { code }));
}

/// 引擎窗口事件直取（仅 pump 使用——翻译 + 状态表喂入的唯一入口）
fn poll_engine() -> Option<Event> {
    crate::display::with_window(|w| w.poll_event().and_then(translate)).flatten()
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
            crate::touch::on_touch(phase, finger, x as i32, y as i32);
            Some(Event::Touch {
                phase,
                finger,
                x: x as i32,
                y: y as i32,
            })
        }
        WindowEvent::Suspended => Some(Event::Suspend),
        WindowEvent::Resumed => Some(Event::Resume),
        // 自定义事件透传（code 上浮；Any 载荷留在窗口层——Rust 消费者
        // 经 starfish-window 直取，Python 载荷由 pygame-rs 包裹层附加）
        WindowEvent::User(ue) => Some(Event::User { code: ue.code }),
        // RedrawRequested 不上报（循环恒每帧 present）
        _ => Some(Event::Other),
    }
}

/// 引擎鼠标键 → pygame 惯例编号（1=左 2=中 3=右 4=后 5=前）
fn mouse_button(b: starfish_we::MouseButton) -> u8 {
    use starfish_we::MouseButton as Mb;
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
    use starfish_we::MouseButton;

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

    #[test]
    fn custom_user_event_passes_code_through() {
        // User 透传：code 上浮（载荷留在窗口层——Any 不跨 pygame 层）
        let ev = translate(WindowEvent::User(starfish_we::UserEvent::new(
            7,
            "payload",
        )));
        assert!(matches!(ev, Some(Event::User { code: 7 })));

        // post_user 入队 → poll 立即可取（同帧语义）
        crate::event::post_user(42);
        assert!(matches!(crate::event::poll(), Some(Event::User { code: 42 })));

        // 分配器：单调递增、从 1 起
        let a = crate::event::custom_code();
        let b = crate::event::custom_code();
        assert_eq!((a, b), (1, 2));
    }
}
