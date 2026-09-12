//! base/gamepad —— 手柄输入（设备接口模块第一批）
//!
//! 模式对齐键鼠状态表：引擎每帧排水设备事件刷新快照，应用在 `frame` 里经
//! `ctx.gamepad()` 只读轮询（见 [`GamepadState`]）。
//!
//! 平台矩阵：
//! - Windows / Linux / macOS：gilrs 原生（SDL 兼容布局，按钮跨平台统一）
//! - Web：自持 js_sys Reflect 轮询 `navigator.getGamepads()`（标准布局映射；
//!   Gamepad API 为稳定接口，不经 web-sys unstable 门控）
//! - Android / iOS：空实现占位（恒空表），待引擎侧事件管线立项
//!
//! 震动（rumble）：Windows / Linux ✓，macOS / Web ✗（gilrs 平台上限）。

use std::collections::{BTreeMap, HashSet};

/// 手柄按键（SDL 兼容布局，与 gilrs `Button` 一一对应；Web 端映射标准布局）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    South,
    East,
    North,
    West,
    LeftTrigger,
    LeftTrigger2,
    RightTrigger,
    RightTrigger2,
    Select,
    Start,
    LeftThumb,
    RightThumb,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
}

/// 手柄轴（-1.0..=1.0；扳机类 0..=1.0）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    LeftStickX,
    LeftStickY,
    RightStickX,
    RightStickY,
    LeftZ,
    RightZ,
}

/// 单个手柄的帧快照
#[derive(Default)]
struct PadSnapshot {
    buttons: HashSet<Button>,
    axes: BTreeMap<Axis, f32>,
}

impl PadSnapshot {
    fn set_axis(&mut self, axis: Axis, value: f32) {
        self.axes.insert(axis, value);
    }
}

/// 手柄状态表（应用经 `ctx.gamepad()` 只读；引擎每帧 [`GamepadState::poll`] 刷新）
///
/// 手柄编号为平台内自增序号（连接顺序）；热插拔即时反映。
pub struct GamepadState {
    pads: BTreeMap<usize, PadSnapshot>,
    /// 上帧按键集合（`just_pressed` 边沿检测用）
    prev_buttons: BTreeMap<usize, HashSet<Button>>,
    /// 本帧新按下（上帧无、本帧有，或帧内收到按下事件）
    just: HashSet<(usize, Button)>,
    /// 桌面：gilrs 句柄（事件源 + 自动状态缓存）
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    gilrs: Option<gilrs::Gilrs>,
    /// 桌面：自有编号 → gilrs GamepadId（gilrs id 不透明，连接顺序分配自有编号）
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    gilrs_ids: BTreeMap<usize, gilrs::GamepadId>,
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    next_id: usize,
}

impl GamepadState {
    /// 引擎内部：创建（gilrs 初始化失败降级为空表，不 panic）
    pub(crate) fn new() -> Self {
        Self {
            pads: BTreeMap::new(),
            prev_buttons: BTreeMap::new(),
            just: HashSet::new(),
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            gilrs: gilrs::Gilrs::new().ok(),
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            gilrs_ids: BTreeMap::new(),
            #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
            next_id: 0,
        }
    }

    /// 引擎内部：每帧排水设备事件、刷新快照（`about_to_wait` 中、`frame` 前调用）
    pub(crate) fn poll(&mut self) {
        // just_pressed 是帧间边沿：先滚存上帧按键，再清空本帧边沿集
        self.prev_buttons.clear();
        for (id, pad) in &self.pads {
            self.prev_buttons.insert(*id, pad.buttons.clone());
        }
        self.just.clear();

        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        self.poll_gilrs();
        #[cfg(target_arch = "wasm32")]
        self.poll_web();
        // android / ios：无后端，恒空表
    }

    pub fn is_connected(&self, id: usize) -> bool {
        self.pads.contains_key(&id)
    }

    /// 已连接手柄编号（连接顺序）
    pub fn connected(&self) -> impl Iterator<Item = usize> + '_ {
        self.pads.keys().copied()
    }

    /// 第一个连接的手柄（单手柄场景的便捷入口）
    pub fn primary(&self) -> Option<usize> {
        self.pads.keys().next().copied()
    }

    pub fn is_pressed(&self, id: usize, btn: Button) -> bool {
        self.pads.get(&id).is_some_and(|p| p.buttons.contains(&btn))
    }

    /// 本帧新按下（上帧未按下；含帧内按下又释放的情况）
    pub fn just_pressed(&self, id: usize, btn: Button) -> bool {
        if !self.is_pressed(id, btn) && !self.just.contains(&(id, btn)) {
            return false;
        }
        self.prev_buttons
            .get(&id)
            .is_none_or(|prev| !prev.contains(&btn))
    }

    /// 轴值（摇杆 -1.0..=1.0；扳机 0..=1.0）；未连接/未采样返回 0.0
    pub fn axis(&self, id: usize, axis: Axis) -> f32 {
        self.pads
            .get(&id)
            .and_then(|p| p.axes.get(&axis).copied())
            .unwrap_or(0.0)
    }
}

// ── 桌面：gilrs 排水 ──────────────────────────────────────────

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
impl GamepadState {
    fn poll_gilrs(&mut self) {
        let Some(gilrs) = &mut self.gilrs else {
            return;
        };
        // gilrs 0.11 的 GamepadId 不透明：以 Connected 事件为锚，连接顺序分配自有编号
        while let Some(ev) = gilrs.next_event() {
            match ev.event {
                gilrs::EventType::Connected => {
                    if !self.gilrs_ids.values().any(|g| *g == ev.id) {
                        let id = self.next_id;
                        self.next_id += 1;
                        self.gilrs_ids.insert(id, ev.id);
                        self.pads.entry(id).or_default();
                    }
                }
                gilrs::EventType::Disconnected => {
                    if let Some(id) = self
                        .gilrs_ids
                        .iter()
                        .find(|(_, g)| **g == ev.id)
                        .map(|(k, _)| *k)
                    {
                        self.gilrs_ids.remove(&id);
                        self.pads.remove(&id);
                        self.prev_buttons.remove(&id);
                    }
                }
                _ => {
                    // 其余事件按 gilrs id 反查自有编号
                    let Some(id) = self
                        .gilrs_ids
                        .iter()
                        .find(|(_, g)| **g == ev.id)
                        .map(|(k, _)| *k)
                    else {
                        continue;
                    };
                    match ev.event {
                        gilrs::EventType::ButtonPressed(btn, _code) => {
                            let pad = self.pads.entry(id).or_default();
                            let b = from_gilrs_button(btn);
                            if pad.buttons.insert(b) {
                                self.just.insert((id, b));
                            }
                        }
                        gilrs::EventType::ButtonReleased(btn, _code) => {
                            if let Some(pad) = self.pads.get_mut(&id) {
                                pad.buttons.remove(&from_gilrs_button(btn));
                            }
                        }
                        gilrs::EventType::ButtonChanged(btn, value, _code) => {
                            let pad = self.pads.entry(id).or_default();
                            let b = from_gilrs_button(btn);
                            if value > 0.5 {
                                if pad.buttons.insert(b) {
                                    self.just.insert((id, b));
                                }
                            } else {
                                pad.buttons.remove(&b);
                            }
                        }
                        gilrs::EventType::AxisChanged(axis, value, _code) => {
                            let pad = self.pads.entry(id).or_default();
                            pad.set_axis(from_gilrs_axis(axis), value);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn from_gilrs_button(btn: gilrs::ev::Button) -> Button {
    use gilrs::ev::Button as G;
    match btn {
        G::South => Button::South,
        G::East => Button::East,
        G::North => Button::North,
        G::West => Button::West,
        G::LeftTrigger => Button::LeftTrigger,
        G::LeftTrigger2 => Button::LeftTrigger2,
        G::RightTrigger => Button::RightTrigger,
        G::RightTrigger2 => Button::RightTrigger2,
        G::Select => Button::Select,
        G::Start => Button::Start,
        G::LeftThumb => Button::LeftThumb,
        G::RightThumb => Button::RightThumb,
        G::DPadUp => Button::DPadUp,
        G::DPadDown => Button::DPadDown,
        G::DPadLeft => Button::DPadLeft,
        G::DPadRight => Button::DPadRight,
        _ => Button::South, // Unknown 兜底（调用方语义查询不影响其它键）
    }
}

#[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
fn from_gilrs_axis(axis: gilrs::ev::Axis) -> Axis {
    use gilrs::ev::Axis as G;
    match axis {
        G::LeftStickX => Axis::LeftStickX,
        G::LeftStickY => Axis::LeftStickY,
        G::RightStickX => Axis::RightStickX,
        G::RightStickY => Axis::RightStickY,
        G::LeftZ => Axis::LeftZ,
        G::RightZ => Axis::RightZ,
        _ => Axis::LeftStickX,
    }
}

// ── Web：navigator.getGamepads() 轮询（稳定 API，Reflect 直调）──────

#[cfg(target_arch = "wasm32")]
impl GamepadState {
    fn poll_web(&mut self) {
        use wasm_bindgen::JsCast;

        let Some(window) = web_sys::window() else {
            return;
        };
        let Ok(navigator) = js_sys::Reflect::get(&window, &"navigator".into()) else {
            return;
        };
        let Ok(pads_val) = js_sys::Reflect::get(&navigator, &"getGamepads".into()) else {
            return;
        };
        let Ok(get_pads) = pads_val.dyn_into::<js_sys::Function>() else {
            return;
        };
        let Ok(list) = get_pads.call0(&navigator) else {
            return;
        };
        let Ok(list) = list.dyn_into::<js_sys::Array>() else {
            return;
        };

        // 标准布局：按钮/轴索引 → 本模块语义（与 SDL 布局对齐）
        const BTNS: [Button; 16] = [
            Button::South,
            Button::East,
            Button::West,
            Button::North,
            Button::LeftTrigger,
            Button::RightTrigger,
            Button::LeftTrigger2,
            Button::RightTrigger2,
            Button::Select,
            Button::Start,
            Button::LeftThumb,
            Button::RightThumb,
            Button::DPadUp,
            Button::DPadDown,
            Button::DPadLeft,
            Button::DPadRight,
        ];
        const AXES: [Axis; 4] = [
            Axis::LeftStickX,
            Axis::LeftStickY,
            Axis::RightStickX,
            Axis::RightStickY,
        ];

        let mut seen: HashSet<usize> = HashSet::new();
        for i in 0..list.length() {
            let Ok(pad) = list.get(i).dyn_into::<js_sys::Object>() else {
                continue;
            };
            // 断槽位为 null
            if !js_sys::Reflect::get(&pad, &"connected".into())
                .ok()
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                continue;
            }
            let id = i as usize;
            seen.insert(id);
            let snap = self.pads.entry(id).or_default();

            if let Ok(btns) =
                js_sys::Reflect::get(&pad, &"buttons".into()).and_then(|v| v.dyn_into())
            {
                let btns: js_sys::Array = btns;
                for (bi, b) in BTNS.iter().enumerate() {
                    let pressed = js_sys::Reflect::get(&btns, &(bi as u32).into())
                        .ok()
                        .and_then(|bd| js_sys::Reflect::get(&bd, &"pressed".into()).ok())
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if pressed {
                        if snap.buttons.insert(*b) {
                            self.just.insert((id, *b));
                        }
                    } else {
                        snap.buttons.remove(b);
                    }
                }
            }
            if let Ok(axes) =
                js_sys::Reflect::get(&pad, &"axes".into()).and_then(|v| v.dyn_into())
            {
                let axes: js_sys::Array = axes;
                for (ai, a) in AXES.iter().enumerate() {
                    let v = js_sys::Reflect::get(&axes, &(ai as u32).into())
                        .ok()
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    snap.set_axis(*a, v);
                }
            }
        }

        // 消失的槽位 = 断开
        let gone: Vec<usize> = self.pads.keys().copied().filter(|id| !seen.contains(id)).collect();
        for id in gone {
            self.pads.remove(&id);
            self.prev_buttons.remove(&id);
        }
    }
}
