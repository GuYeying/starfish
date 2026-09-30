//! pygame.touch 对应物（薄模块，契约 §五 touch 行）
//!
//! 触摸状态表：[`crate::pygame::event`] 翻译 Touch 事件时维护
//! （与 key/mouse 同款——引擎事件 = 唯一事实源）。按**手指 id** 组织
//! （多点触控；现代移动惯例——pygame 官方按设备编号的 API 不适配
//! 多点语义，绑定层标注差异）。
//!
//! 桌面端永不产生 Touch 事件（表恒空）；移动端后端已把主手指合成为
//! 鼠标事件，鼠标交互逻辑可直接复用。

use std::cell::RefCell;
use std::collections::HashMap;

use crate::base::window::TouchPhase;

/// 活动中的手指
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finger {
    /// 手指 id（多点触控时区分手指）
    pub id: u64,
    /// 客户区物理像素 X
    pub x: i32,
    /// 客户区物理像素 Y
    pub y: i32,
    /// 最近一次事件的阶段
    pub phase: TouchPhase,
}

thread_local! {
    static FINGERS: RefCell<HashMap<u64, Finger>> = RefCell::new(HashMap::new());
}

pub(crate) fn on_touch(phase: TouchPhase, finger: u64, x: i32, y: i32) {
    FINGERS.with(|s| {
        let mut s = s.borrow_mut();
        match phase {
            TouchPhase::Started | TouchPhase::Moved => {
                s.insert(finger, Finger { id: finger, x, y, phase });
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                s.remove(&finger);
            }
            _ => {}
        }
    });
}

/// 活动手指快照（按 id 升序；空 = 无触摸）
pub fn fingers() -> Vec<Finger> {
    let mut v: Vec<Finger> =
        FINGERS.with(|s| s.borrow().values().copied().collect());
    v.sort_by_key(|f| f.id);
    v
}

/// 活动手指数量
pub fn get_count() -> usize {
    FINGERS.with(|s| s.borrow().len())
}

/// 指定手指的位置（无该手指 = None）
pub fn get_pos(finger: u64) -> Option<(i32, i32)> {
    FINGERS.with(|s| s.borrow().get(&finger).map(|f| (f.x, f.y)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_move_end_lifecycle() {
        on_touch(TouchPhase::Started, 7, 10, 20);
        assert_eq!(get_count(), 1);
        assert_eq!(get_pos(7), Some((10, 20)));

        on_touch(TouchPhase::Moved, 7, 30, 40);
        on_touch(TouchPhase::Started, 3, 1, 2);
        let fs = fingers();
        assert_eq!(fs.len(), 2);
        assert_eq!(fs[0].id, 3, "按 id 升序");
        assert_eq!(get_pos(7), Some((30, 40)));

        on_touch(TouchPhase::Ended, 7, 30, 40);
        on_touch(TouchPhase::Cancelled, 3, 1, 2);
        assert_eq!(get_count(), 0);
    }
}
