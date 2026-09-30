//! pygame.mouse 对应物（契约 ADR-7：状态表随事件翻译维护）
//!
//! 坐标 = 客户区物理像素（y 向下）；按键三元组对齐 pygame 惯例
//! （左/中/右）。可见态走窗口层 `set_cursor_visible`；warp（set_pos）
//! 垫底——仅更新状态表（starfish-window v1 无光标扭曲，无合成事件）。

use std::cell::Cell;

thread_local! {
    static POS: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
    static BUTTONS: Cell<(bool, bool, bool)> = const { Cell::new((false, false, false)) };
    /// get_rel 的相对位移基准（上次调用时的位置）
    static LAST_READ: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
    /// 光标可见态（set_visible 维护；窗口实际态可能被系统改写）
    static VISIBLE: Cell<bool> = const { Cell::new(true) };
}

/// 更新状态表位置（事件翻译喂 + `pygame.mouse.set_pos` 垫底共用——
/// starfish-window v1 无光标扭曲，仅状态表生效）
pub fn set_pos(x: i32, y: i32) {
    POS.with(|p| p.set((x, y)));
}

/// button：1=左 2=中 3=右（pygame 惯例；4/5 忽略不占三元组）
pub(crate) fn press(button: u8) {
    set_button(button, true);
}

pub(crate) fn release(button: u8) {
    set_button(button, false);
}

fn set_button(button: u8, down: bool) {
    BUTTONS.with(|b| {
        let cur = b.get();
        b.set(match button {
            1 => (down, cur.1, cur.2),
            2 => (cur.0, down, cur.2),
            3 => (cur.0, cur.1, down),
            _ => cur,
        });
    });
}

/// 鼠标位置（客户区物理像素）
pub fn get_pos() -> (i32, i32) {
    POS.with(|p| p.get())
}

/// 相对位移（自上次 get_rel 调用起；pygame 语义——连续调用逐段累加）
pub fn get_rel() -> (i32, i32) {
    let cur = get_pos();
    LAST_READ.with(|last| {
        let d = (cur.0 - last.get().0, cur.1 - last.get().1);
        last.set(cur);
        d
    })
}

/// 三键状态（左/中/右，pygame 惯例）
pub fn get_pressed() -> (bool, bool, bool) {
    BUTTONS.with(|b| b.get())
}

/// 光标可见态（窗口层 set_cursor_visible；移动端无光标 = 无操作语义）
pub fn set_visible(visible: bool) {
    VISIBLE.with(|s| s.set(visible));
    let _ = super::display::with_window(|w| w.set_cursor_visible(visible));
}

/// 光标可见态查询（set_visible 维护的本地态）
pub fn get_visible() -> bool {
    VISIBLE.with(|s| s.get())
}

/// 窗口鼠标聚焦态（简化：与键盘聚焦同源——FocusGained/Lost 事件）
pub fn get_focused() -> bool {
    super::key::get_focused()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_and_buttons_roundtrip() {
        set_pos(5, 6);
        assert_eq!(get_pos(), (5, 6));
        press(1);
        press(3);
        assert_eq!(get_pressed(), (true, false, true));
        release(1);
        release(3);
        assert_eq!(get_pressed(), (false, false, false));
        // 4/5（后/前键）不进三元组，不 panic
        press(4);
        release(5);
    }
}
