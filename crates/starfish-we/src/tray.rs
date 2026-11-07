//! System tray (feature "tray"): shared data plane + event drain.
//! 系统托盘（feature "tray"）：跨平台共享数据面 + 事件排水。
//!
//! Backend = the `tray-icon` crate (Windows/macOS default backends, Linux
//! ksni pure-D-Bus backend). This module owns only the cross-platform data
//! types and the non-blocking drain; each platform module owns the
//! `TrayIcon` lifetime (thread-local, matching the window thread-affinity
//! model) and its construction.
//! 后端 = tray-icon crate（Windows/macOS 默认后端、Linux ksni 纯 D-Bus
//! 后端）。本模块只持有跨平台数据类型与非阻塞排水；各平台模块自持
//! `TrayIcon` 生命周期（thread-local，与窗口线程亲和模型配套）与构造。
//!
//! Event flow: tray-icon's hidden window receives its messages from the
//! thread's message pump (our `poll_event` pumps all thread windows), so
//! menu / icon events land in the two global crossbeam channels; the drain
//! converts them into [`Event::Tray`] on the same poll.
//! 事件流：tray-icon 的隐藏窗口消息由线程消息泵派发（本库 `poll_event`
//! 泵全线程窗口），菜单/图标事件落入两个全局 crossbeam channel；
//! 排水在同一 poll 内转成 [`Event::Tray`]。

use std::collections::VecDeque;

use crate::event::Event;

/// Tray icon request: RGBA icon + tooltip + flat menu.
/// 托盘图标规格（set_tray 进参）：RGBA 图标 + 提示文本 + 平铺菜单。
#[derive(Debug, Clone)]
pub struct TraySpec {
    /// Icon pixels: tightly packed 32bpp RGBA8888 (`len == w * h * 4`).
    /// 图标像素（32bpp RGBA8888 紧排；len = w*h*4）。
    pub icon_rgba: Vec<u8>,
    /// Icon width in pixels.
    /// 图标宽（像素）。
    pub icon_w: u32,
    /// Icon height in pixels.
    /// 图标高（像素）。
    pub icon_h: u32,
    /// Hover tooltip (empty = none).
    /// 悬停提示（空串 = 无）。
    pub tooltip: String,
    /// Flat menu items `(id, label)` — `id` is the event echo key.
    /// Empty slice = bare icon (no menu).
    /// 平铺菜单项 `(id, label)`——id 为事件回执键；空表 = 纯图标（无菜单）。
    pub menu: Vec<(String, String)>,
}

/// Tray event kind.
/// 托盘事件类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayKind {
    /// A menu item was clicked (`id` = menu item id).
    /// 菜单项点击（id = 菜单项 id）。
    Menu,
    /// Single click on the tray icon.
    /// 托盘图标单击。
    Click,
    /// Double click on the tray icon (Windows-only form; other backends
    /// never produce it).
    /// 托盘图标双击（Windows 专属形态——其余后端不产生）。
    DoubleClick,
}

/// One tray event (payload of [`Event::Tray`](crate::Event::Tray)).
/// 单条托盘事件（[`Event::Tray`](crate::Event::Tray) 载荷）。
#[derive(Debug, Clone)]
pub struct TrayEvent {
    /// Which kind of tray interaction.
    /// 托盘交互类别。
    pub kind: TrayKind,
    /// `Menu` = clicked menu item id; `Click`/`DoubleClick` = tray icon id.
    /// `Menu` = 菜单项 id；`Click`/`DoubleClick` = 托盘图标 id。
    pub id: String,
    /// Button code (1 = left, 2 = middle, 3 = right; pygame convention).
    /// `Menu` = 0.
    /// 按键编号（1=左 2=中 3=右，pygame 惯例）；`Menu` = 0。
    pub button: u8,
    /// Screen position, physical pixels (`Menu` = 0).
    /// 屏幕物理坐标（`Menu` = 0）。
    pub x: f64,
    /// Screen position Y.
    /// 屏幕物理坐标 Y。
    pub y: f64,
}

/// Non-blocking tray event drain — called by each desktop platform's
/// `poll_event` after its message pump. Appends [`Event::Tray`] entries to
/// `pending`. Compiled with feature `tray` **on the three desktop targets
/// only** (the backend crate is target-gated; enabling the feature on
/// web/mobile keeps the facade types + Err stubs without linking the
/// backend).
/// 非阻塞托盘事件排水——各桌面平台 `poll_event` 消息泵后调用；把
/// [`Event::Tray`] 追加进 `pending`。仅 feature `tray` **且三桌面目标**
/// 编译（后端 crate 按目标平台门控；web/mobile 上启用 feature 时保留
/// 门面类型 + Err 站桩，后端不链接）。
#[cfg(all(
    feature = "tray",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub fn drain_events(pending: &mut VecDeque<Event>) {
    use tray_icon::TrayIconEvent as Tie;

    for ev in Tie::receiver().try_iter() {
        let Some(tray_ev) = convert_icon_event(ev) else {
            continue;
        };
        pending.push_back(Event::Tray(tray_ev));
    }
    for ev in tray_icon::menu::MenuEvent::receiver().try_iter() {
        pending.push_back(Event::Tray(TrayEvent {
            kind: TrayKind::Menu,
            id: ev.id.0,
            button: 0,
            x: 0.0,
            y: 0.0,
        }));
    }
}

/// Icon event conversion (hover noise dropped; a click counts on release
/// so press+release is one event).
/// 图标事件转换（悬停噪声丢弃；按下+抬起折算一次——抬起才算点击）。
#[cfg(all(
    feature = "tray",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
fn convert_icon_event(ev: tray_icon::TrayIconEvent) -> Option<TrayEvent> {
    use tray_icon::{MouseButtonState as Tbs, TrayIconEvent as Tie};

    match ev {
        Tie::Click {
            id,
            position,
            button,
            button_state,
            ..
        } => {
            if button_state != Tbs::Up {
                return None;
            }
            Some(TrayEvent {
                kind: TrayKind::Click,
                id: id.0,
                button: button_code(button),
                x: position.x,
                y: position.y,
            })
        }
        Tie::DoubleClick { id, position, button, .. } => Some(TrayEvent {
            kind: TrayKind::DoubleClick,
            id: id.0,
            button: button_code(button),
            x: position.x,
            y: position.y,
        }),
        // Enter / Move / Leave: hover noise — not surfaced.
        // Enter / Move / Leave：悬停噪声——不上浮。
        _ => None,
    }
}

/// tray-icon button → pygame button code (1 = left, 2 = middle, 3 = right).
/// tray-icon 按键 → pygame 编号（1=左 2=中 3=右）。
#[cfg(all(
    feature = "tray",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
fn button_code(button: tray_icon::MouseButton) -> u8 {
    match button {
        tray_icon::MouseButton::Left => 1,
        tray_icon::MouseButton::Middle => 2,
        tray_icon::MouseButton::Right => 3,
        _ => 0,
    }
}
