//! pygame.gamepad 对应物（薄模块，契约 §二 gamepad 行）
//!
//! [`crate::base::gamepad::GamepadState`] 的进程级持有 + 查询面：与
//! key/mouse 同款 thread_local 状态表（引擎事件 = 唯一事实源；手柄无
//! 窗口事件流，由**宿主每帧 [`poll`]** 排水驱动——gilrs/Web 轮询型
//! 后端，一帧一次）。
//!
//! 命名定约（架构文档 §二）：本模块替代 joystick+controller 两名，
//! 按钮语义对齐 SDL 布局（文档标注 controller 对齐关系）。
//! Android/iOS 后端为空实现占位（恒空表，见 base/gamepad.rs 平台矩阵）。

use std::cell::RefCell;

use crate::base::gamepad::{Axis, Button, GamepadState};

thread_local! {
    static PADS: RefCell<GamepadState> = RefCell::new(GamepadState::new());
}

/// 每帧排水设备事件、刷新快照（宿主壳帧循环调用，一帧一次——
/// just_pressed 是帧间边沿，多次调用会破坏边沿语义）
pub fn poll() {
    PADS.with(|p| p.borrow_mut().poll());
}

/// 已连接手柄数
pub fn get_count() -> usize {
    PADS.with(|p| p.borrow().connected().count())
}

/// 已连接手柄编号列表（连接顺序）
pub fn connected() -> Vec<usize> {
    PADS.with(|p| p.borrow().connected().collect())
}

/// 第一个连接的手柄（单手柄便捷入口；无 = None）
pub fn primary() -> Option<usize> {
    PADS.with(|p| p.borrow().primary())
}

/// 指定手柄是否已连接
pub fn is_connected(id: usize) -> bool {
    PADS.with(|p| p.borrow().is_connected(id))
}

/// 按键按下态（未连接 = False）
pub fn is_pressed(id: usize, btn: Button) -> bool {
    PADS.with(|p| p.borrow().is_pressed(id, btn))
}

/// 本帧新按下（上帧未按下；未连接 = False）
pub fn just_pressed(id: usize, btn: Button) -> bool {
    PADS.with(|p| p.borrow().just_pressed(id, btn))
}

/// 轴值（摇杆 -1.0..=1.0；扳机 0..=1.0；未连接/未采样 = 0.0）
pub fn axis(id: usize, axis: Axis) -> f32 {
    PADS.with(|p| p.borrow().axis(id, axis))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 无设备环境（CI/无手柄）：查询面恒空且不 panic
    #[test]
    fn empty_table_queries_are_safe() {
        poll();
        assert_eq!(get_count(), 0);
        assert!(connected().is_empty());
        assert_eq!(primary(), None);
        assert!(!is_connected(0));
        assert!(!is_pressed(0, Button::South));
        assert!(!just_pressed(0, Button::South));
        assert_eq!(axis(0, Axis::LeftStickX), 0.0);
    }
}
