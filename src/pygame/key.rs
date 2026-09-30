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
