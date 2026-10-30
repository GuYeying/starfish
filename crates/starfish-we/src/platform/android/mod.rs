//! Android (NativeActivity) backend.
//! Android (NativeActivity) 后端。
//!
//! Android's entry point and lifecycle are system-driven — the inverse of
//! this crate's poll model. After [`android_init`] wires up the system
//! callbacks, [`poll_event`](WindowInner::poll_event) pumps one round of
//! `AndroidApp::poll_events` per call (timeout = 0, non-blocking),
//! **translating** lifecycle / input events into a buffer queue and
//! popping one from the head — the "callbacks write, poll drains"
//! inversion device.
//! Android 的入口与生命周期由系统驱动，与本 crate 的 poll 模型相反：
//! [`android_init`] 装配系统回调后，[`poll_event`](WindowInner::poll_event)
//! 在每次调用时泵一轮 `AndroidApp::poll_events`（timeout=0 非阻塞），
//! 把生命周期 / 输入事件**翻译进缓冲队列**再从队头取一个——即"回调写入、
//! poll 排空"的反转装置。
//!
//! Conventions (matching the public docs):
//! 约定（与公共文档一致）：
//! - Sizes / coordinates are physical pixels (ANativeWindow coordinates
//!   are physical pixels).
//!   坐标 / 尺寸为物理像素（ANativeWindow 坐标即物理像素）。
//! - Touch → `Event::Touch`, with the primary finger simultaneously
//!   synthesized into mouse events.
//!   触摸 → `Event::Touch`，主手指（pointer id 0）同时合成鼠标事件。
//! - Lifecycle: `Pause`/`Stop`/`TerminateWindow` → `Suspended`,
//!   `Resume` → `Resumed`, `Destroy` → `CloseRequested`.
//!   生命周期：`Pause`/`Stop`/`TerminateWindow` → `Suspended`，
//!   `Resume` → `Resumed`，`Destroy` → `CloseRequested`。
//! - The back key (`Keycode::Back`) → `CloseRequested` (Android app
//!   convention).
//!   返回键（`Keycode::Back`）→ `CloseRequested`（Android 应用惯例）。
//! - `TextInput` comes from the system IME's committed text
//!   (`InputEvent::TextEvent`).
//!   `TextInput` 来自系统 IME 的提交文本（`InputEvent::TextEvent`）。
//!
//! All calls (`Window::new` / `poll_event`) must happen on the
//! `android_main` thread (android-activity asserts this in `poll_events`).
//! 所有调用（`Window::new` / `poll_event`）必须在 `android_main` 线程
//! （android-activity 的 `poll_events` 有此要求）。

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) mod entry;
pub(crate) mod embedded;

pub(crate) use embedded::{
    attach_surface_impl, detach_surface_impl, external_active, external_size_impl,
    inject_touch_impl, release_pending_external, surface_resized_impl, EXTERNAL,
};

use android_activity::input::{InputEvent, KeyAction, Keycode, MotionAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};

use crate::error::WindowError;
use crate::event::{Event, KeyCode, Modifiers, MouseButton, TouchPhase};
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

/// The app handle wired up by `android_main`.
/// `android_main` 装配进来的 app 句柄。
///
/// Contract: Android caches processes — "back" only destroys the Activity
/// while the process survives; on the next launch `android_main` runs
/// again **in the same process, on a new thread**. This slot must
/// therefore be updated by overwrite: with a `OnceLock`, the second
/// `android_init` would be silently ignored and every later call would
/// land on the previous run's destroyed app/looper — a crash on relaunch
/// (android-activity's poll thread assertion panics).
/// 契约：Android 会缓存进程——用户"返回"只销毁 Activity，进程存活；
/// 再次启动时 `android_main` 会在**同一进程、新线程**上再次执行。
/// 因此这里必须覆盖式更新：用 `OnceLock` 的话第二次装配被静默忽略，
/// 之后所有调用都落在上次已销毁的 app / looper 上——二次启动闪退
/// （android-activity 的 poll 线程断言直接 panic）。
static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);

/// Events translated from platform callbacks (single window; process-wide
/// buffer).
/// 平台回调翻译出的待发事件（单窗口，进程级缓冲）。
static PENDING: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// Whether this run has seen `InitWindow` (surface ready).
/// 本次运行是否已见过 `InitWindow`（surface 就绪）。
///
/// Used to drop the **previous instance's cross-talk**: a late `onDestroy`
/// from the old activity can land in the new instance's event stream;
/// translating it as-is yields a `CloseRequested`, presenting as "the app
/// exits immediately on relaunch".
/// 用于丢弃进程复用时的**旧实例串扰**：上一实例的 onDestroy 可能晚到、
/// 落进新实例的事件流，若照单全收会被翻译成 `CloseRequested`，
/// 表现为"再次启动秒退"。
static WINDOW_READY: AtomicBool = AtomicBool::new(false);

/// Called by the host in `android_main` to wire up the system callbacks.
/// 宿主在 `android_main` 里调用，装配系统回调。
///
/// Supports multiple calls within one process (when the process is cached
/// and reused, `android_main` runs a second time): overwrites the old app,
/// clears the previous run's buffered events and resets the readiness
/// flag.
/// 支持同进程内多次调用（进程被系统缓存复用时，`android_main` 会二次
/// 执行）：覆盖旧 app、清空上次运行遗留的事件缓冲、重置就绪标记。
pub fn android_init(app: AndroidApp) {
    *APP.lock().expect("APP lock poisoned / APP 锁中毒") = Some(app);
    // The single-window slot is counted per "run": during the overlap the
    // previous instance may still hold the slot.
    // 单窗口占位按"运行"计算：交叠期内上一实例可能还占着位子。
    crate::window::reset_single_window_slot();
    {
        PENDING.lock().expect("PENDING 锁中毒").clear();
        WINDOW_READY.store(false, Ordering::Release);
    }
}

pub(crate) struct WindowInner;

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        // 嵌入模式：窗口已由宿主注入，跳过 glue 等待
        if external_active() {
            return Ok(Self);
        }
        let app = app_handle()?;
        // NativeActivity's surface only exists after `InitWindow`: block
        // and wait (the android_main thread may block), capped at 10 s.
        // NativeActivity 的 surface 在 `InitWindow` 之后才存在：
        // 阻塞等待（android_main 线程允许阻塞），上限 10 秒。
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while app.native_window().is_none() {
            if std::time::Instant::now() > deadline {
                return Err(WindowError::OsError(
                    "timed out waiting for the Android surface (10s): no InitWindow".into(),
                ));
            }
            app.poll_events(Some(Duration::from_millis(50)), |_| {});
        }
        let _ = options; // Size comes from the system surface; title/frame concepts do not apply to Android
        Ok(Self)
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        // 嵌入模式：事件由注入口直入 PENDING（无 glue 泵）
        if external_active() {
            return PENDING
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pop_front();
        }
        let app = app_handle().ok()?;
        // Poison-tolerant lock: recover the queue after a concurrent panic
        // (e.g. the thread assertion during process-reuse overlap) instead
        // of cascading — during overlap the new instance takes over anyway.
        // 锁中毒容错：并发 panic（如进程复用交叠期的线程断言）后恢复队列
        // 并清空，不连环爆——交叠期本就该由新实例接管。
        let mut queue = PENDING.lock().unwrap_or_else(|poisoned| {
            let mut queue = poisoned.into_inner();
            queue.clear();
            queue
        });

        // 1) Pump lifecycle events (non-blocking).
        // 1) 泵生命周期事件（非阻塞）。
        app.poll_events(Some(Duration::ZERO), |poll_event| {
            if let PollEvent::Main(main) = poll_event {
                translate_main(&app, main, &mut queue);
            }
        });

        // 2) Pump input events (android-activity's callback-style input
        //    iterator).
        // 2) 泵输入事件（android-activity 的输入迭代器，回调式消费）。
        if let Ok(mut iter) = app.input_events_iter() {
            while iter.next(|event| {
                translate_input(&mut queue, event);
                InputStatus::Handled
            }) {}
        }

        queue.pop_front()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        // 嵌入模式：外部窗口尺寸优先
        if let Some(ext) = EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            return (ext.width, ext.height);
        }
        native_window_size().unwrap_or((0, 0))
    }

    pub(crate) fn set_title(&self, _title: &str) {
        // Android has no window-title concept; ignored.
        // Android 无窗口标题概念，忽略
    }

    // ── Runtime window capabilities (the Android support matrix) /
    //    运行期窗口能力（Android 平台矩阵）────────────────────────
    // Naturally fullscreen (follows the system surface); window-decoration
    // concepts like NOFRAME / ALWAYS_ON_TOP / min-max do not apply to
    // Android — the corresponding methods are warn-once no-ops.
    // 天然全屏（跟随系统 surface）；NOFRAME/ALWAYS_ON_TOP/min-max 等
    // 窗口装饰概念不适用于 Android——对应方法 warn-once no-op。
    pub(crate) fn set_resizable(&self, _resizable: bool) {}
    pub(crate) fn is_resizable(&self) -> bool {
        false
    }
    pub(crate) fn set_size(&self, _width: u32, _height: u32) {
        // ⚠️ 有意 no-op（2026-10-27 真机实验结论）：曾尝试接通
        // ANativeWindow_setBuffersGeometry（脚本 set_mode → buffer 几何
        // 对齐 → SurfaceFlinger 缩放铺满 SurfaceView）——几何本身生效，
        // 但先于几何变更创建的 EGLSurface 在部分驱动（ColorOS 实测）不
        // 跟随重建，此后 eglSwapBuffers 恒失败 → 全黑（帧循环健康、无
        // panic、present 静默丢帧）。正解属 wgpu/EGL 层（surface 重建）
        // 或引擎 letterbox MVP（缩放内容而非改几何），不在壳层做。
    }
    pub(crate) fn set_fullscreen(&self, _fullscreen: bool) {}
    pub(crate) fn is_fullscreen(&self) -> bool {
        true
    }
    pub(crate) fn set_borderless(&self, _borderless: bool) {}
    pub(crate) fn is_borderless(&self) -> bool {
        true
    }
    pub(crate) fn set_mouse_grabbed(&self, _grabbed: bool) {}
    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        false
    }
    pub(crate) fn set_mouse_relative(&self, _enable: bool) {}
    pub(crate) fn set_cursor_visible(&self, _visible: bool) {}
    pub(crate) fn is_cursor_visible(&self) -> bool {
        true
    }
    pub(crate) fn set_visible(&self, _visible: bool) {}
    pub(crate) fn is_visible(&self) -> bool {
        true
    }
    pub(crate) fn set_always_on_top(&self, _top: bool) {}
    pub(crate) fn is_always_on_top(&self) -> bool {
        false
    }
    pub(crate) fn minimize(&self) {}
    pub(crate) fn maximize(&self) {}
    pub(crate) fn restore(&self) {}
    pub(crate) fn raise(&self) {}
    pub(crate) fn is_minimized(&self) -> bool {
        false
    }
    pub(crate) fn is_maximized(&self) -> bool {
        false
    }
    pub(crate) fn has_input_focus(&self) -> bool {
        true
    }
    pub(crate) fn is_occluded(&self) -> bool {
        false
    }
    pub(crate) fn set_keyboard_grabbed(&self, _grab: bool) {}

    pub(crate) fn set_text_input_visible(&self, _visible: bool) -> bool {
        // 桌面/Web/Android：文本随按键事件自动送达，无软键盘可控显隐。
        // Desktop/Web/Android: text arrives with the key events; no
        // controllable soft keyboard.
        false
    }

    pub(crate) fn is_text_input_visible(&self) -> bool {
        false
    }
    pub(crate) fn set_position(&self, _x: i32, _y: i32) {}
    pub(crate) fn position(&self) -> (i32, i32) {
        (0, 0)
    }
    pub(crate) fn center_on_screen(&self) {}
    pub(crate) fn title(&self) -> String {
        String::new()
    }
    pub(crate) fn set_opacity(&self, _opacity: f32) {}
    pub(crate) fn opacity(&self) -> f32 {
        1.0
    }
    pub(crate) fn dpi_scale(&self) -> f32 {
        1.0
    }
    pub(crate) fn high_pixel_density(&self) -> bool {
        true
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        // 嵌入模式：外部注入的 ANativeWindow 优先
        if let Some(ext) = EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            return RawWindowHandle::Android {
                a_native_window: ext.ptr,
            };
        }
        // Returns a null pointer while the surface is absent (reclaimed in
        // the background); callers should only hand it to a rendering
        // backend while `Resumed`.
        // surface 不在时（后台被回收）返回空指针；调用方应仅在
        // `Resumed` 状态下向渲染后端传递。
        let ptr = app_handle()
            .ok()
            .and_then(|app| app.native_window())
            .map(|window| window.ptr().as_ptr() as *mut std::ffi::c_void)
            .unwrap_or(std::ptr::null_mut());
        RawWindowHandle::Android {
            a_native_window: ptr,
        }
    }
}

/// The current app (cheap Arc clone); errors when not wired up yet.
/// 当前 app（Arc 克隆，廉价）；未装配时报错。
fn app_handle() -> Result<AndroidApp, WindowError> {
    APP.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
        .ok_or_else(|| {
            WindowError::OsError(
                "android_init was not called (the host must wire it up in android_main)".into(),
            )
        })
}

fn native_window_size() -> Option<(u32, u32)> {
    let window = app_handle().ok()?.native_window()?;
    Some((window.width() as u32, window.height() as u32))
}

/// Lifecycle event translation.
/// 生命周期事件翻译。
fn translate_main(app: &AndroidApp, main: MainEvent<'_>, queue: &mut VecDeque<Event>) {
    use Event as E;
    match main {
        // Surface ready / reclaimed: the signal for when to rebuild the
        // wgpu surface.
        // surface 就绪 / 回收：这是 wgpu surface 重建的时机信号。
        MainEvent::InitWindow { .. } => {
            WINDOW_READY.store(true, Ordering::Release);
            if let Some((width, height)) = native_window_size() {
                queue.push_back(E::Resized { width, height });
            }
            queue.push_back(E::Resumed);
            queue.push_back(E::RedrawRequested);
        }
        MainEvent::TerminateWindow { .. } => queue.push_back(E::Suspended),
        MainEvent::WindowResized { .. } => {
            if let Some((width, height)) = native_window_size() {
                queue.push_back(E::Resized { width, height });
            }
        }
        MainEvent::RedrawNeeded { .. } => queue.push_back(E::RedrawRequested),
        MainEvent::GainedFocus => queue.push_back(E::FocusGained),
        MainEvent::LostFocus => queue.push_back(E::FocusLost),
        MainEvent::Resume { .. } => queue.push_back(E::Resumed),
        MainEvent::Pause | MainEvent::Stop => queue.push_back(E::Suspended),
        // A Destroy received before readiness is necessarily cross-talk
        // from the previous instance (a normal destroy always happens
        // after InitWindow); drop it to prevent "instant exit on
        // relaunch".
        // 就绪前收到的 Destroy 必然是上一实例的串扰（正常销毁一定发生在
        // InitWindow 之后），丢弃以防"再次启动秒退"。
        MainEvent::Destroy => {
            if WINDOW_READY.load(Ordering::Acquire) {
                queue.push_back(E::CloseRequested);
            }
        }
        // Start/SaveState/ConfigChanged/LowMemory/ContentRectChanged/
        // InsetsChanged: ignored in the first version.
        // Start/SaveState/ConfigChanged/LowMemory/ContentRectChanged/InsetsChanged：首版忽略
        _ => {
            let _ = app;
        }
    }
}

/// Input event translation: touch (with primary-finger mouse synthesis)
/// and keys / IME text.
/// 输入事件翻译：触摸（含主手指合成鼠标）与按键 / IME 文本。
fn translate_input(queue: &mut VecDeque<Event>, event: &InputEvent<'_>) {
    use Event as E;
    match event {
        InputEvent::MotionEvent(motion) => {
            let phase = match motion.action() {
                MotionAction::Down | MotionAction::PointerDown => TouchPhase::Started,
                MotionAction::Move => TouchPhase::Moved,
                MotionAction::Up | MotionAction::PointerUp => TouchPhase::Ended,
                MotionAction::Cancel => TouchPhase::Cancelled,
                // Hover / Scroll / ButtonPress etc.: ignored in the first
                // version.
                // Hover / Scroll / ButtonPress 等：首版忽略
                _ => return,
            };
            for pointer in motion.pointers() {
                let (x, y) = (pointer.x() as f64, pointer.y() as f64);
                let finger = pointer.pointer_id() as u64;
                queue.push_back(E::Touch {
                    phase,
                    finger,
                    x,
                    y,
                });
                // The primary finger (id 0) is synthesized into mouse
                // events (SDL convention).
                // 主手指（id 0）合成鼠标事件（SDL 惯例）。
                if pointer.pointer_id() == 0 {
                    let mouse = match phase {
                        TouchPhase::Started => E::MouseDown {
                            button: MouseButton::Left,
                        },
                        TouchPhase::Moved => E::MouseMoved { x, y },
                        TouchPhase::Ended | TouchPhase::Cancelled => E::MouseUp {
                            button: MouseButton::Left,
                        },
                    };
                    queue.push_back(mouse);
                }
            }
        }
        InputEvent::KeyEvent(key) => {
            let keycode = key.key_code();
            // Back = close request (Android app convention).
            // 返回键 = 关闭请求（Android 应用惯例）。
            if keycode == Keycode::Back {
                if key.action() == KeyAction::Down {
                    queue.push_back(E::CloseRequested);
                }
                return;
            }
            let modifiers = Modifiers {
                shift: key.meta_state().shift_on(),
                ctrl: key.meta_state().ctrl_on(),
                alt: key.meta_state().alt_on(),
                meta: key.meta_state().meta_on(),
            };
            let key_event = match key.action() {
                KeyAction::Down => E::KeyDown {
                    key: keycode_to_keycode(keycode),
                    modifiers,
                    repeat: key.repeat_count() > 0,
                },
                KeyAction::Up => E::KeyUp {
                    key: keycode_to_keycode(keycode),
                    modifiers,
                },
                // Multiple (long-press auto-repeat): treated as one KeyDown
                // with repeat = true.
                // Multiple（长按连发）：按一次带 repeat 的 KeyDown 处理
                KeyAction::Multiple | _ => E::KeyDown {
                    key: keycode_to_keycode(keycode),
                    modifiers,
                    repeat: true,
                },
            };
            queue.push_back(key_event);
        }
        // Final text committed by the system IME.
        // 系统 IME 提交的最终文本。
        InputEvent::TextEvent(state) => {
            if !state.text.is_empty() {
                queue.push_back(E::TextInput {
                    text: state.text.clone(),
                });
            }
        }
        // TextAction (IME action keys): ignored in the first version.
        // TextAction（IME 动作键）：首版忽略
        _ => {}
    }
}

/// Android `Keycode` → [`KeyCode`]: ranges + item-by-item mapping;
/// unrecognized keys become [`KeyCode::Other`] (carrying the native
/// AKEYCODE value) so no key is lost.
/// Android `Keycode` → [`KeyCode`]：区间 + 逐项映射，未识别包成
/// [`KeyCode::Other`]（携带 AKEYCODE 原生值），不丢按键。
fn keycode_to_keycode(keycode: Keycode) -> KeyCode {
    use KeyCode as K;
    match keycode {
        Keycode::A => K::KeyA,
        Keycode::B => K::KeyB,
        Keycode::C => K::KeyC,
        Keycode::D => K::KeyD,
        Keycode::E => K::KeyE,
        Keycode::F => K::KeyF,
        Keycode::G => K::KeyG,
        Keycode::H => K::KeyH,
        Keycode::I => K::KeyI,
        Keycode::J => K::KeyJ,
        Keycode::K => K::KeyK,
        Keycode::L => K::KeyL,
        Keycode::M => K::KeyM,
        Keycode::N => K::KeyN,
        Keycode::O => K::KeyO,
        Keycode::P => K::KeyP,
        Keycode::Q => K::KeyQ,
        Keycode::R => K::KeyR,
        Keycode::S => K::KeyS,
        Keycode::T => K::KeyT,
        Keycode::U => K::KeyU,
        Keycode::V => K::KeyV,
        Keycode::W => K::KeyW,
        Keycode::X => K::KeyX,
        Keycode::Y => K::KeyY,
        Keycode::Z => K::KeyZ,
        Keycode::Keycode0 => K::Digit0,
        Keycode::Keycode1 => K::Digit1,
        Keycode::Keycode2 => K::Digit2,
        Keycode::Keycode3 => K::Digit3,
        Keycode::Keycode4 => K::Digit4,
        Keycode::Keycode5 => K::Digit5,
        Keycode::Keycode6 => K::Digit6,
        Keycode::Keycode7 => K::Digit7,
        Keycode::Keycode8 => K::Digit8,
        Keycode::Keycode9 => K::Digit9,
        Keycode::F1 => K::F1,
        Keycode::F2 => K::F2,
        Keycode::F3 => K::F3,
        Keycode::F4 => K::F4,
        Keycode::F5 => K::F5,
        Keycode::F6 => K::F6,
        Keycode::F7 => K::F7,
        Keycode::F8 => K::F8,
        Keycode::F9 => K::F9,
        Keycode::F10 => K::F10,
        Keycode::F11 => K::F11,
        Keycode::F12 => K::F12,
        Keycode::Escape => K::Escape,
        Keycode::Tab => K::Tab,
        Keycode::CapsLock => K::CapsLock,
        Keycode::Space => K::Space,
        Keycode::Enter | Keycode::NumpadEnter => K::Enter,
        Keycode::Del => K::Backspace,
        Keycode::ForwardDel => K::Delete,
        Keycode::ShiftLeft => K::ShiftLeft,
        Keycode::ShiftRight => K::ShiftRight,
        Keycode::CtrlLeft => K::ControlLeft,
        Keycode::CtrlRight => K::ControlRight,
        Keycode::AltLeft => K::AltLeft,
        Keycode::AltRight => K::AltRight,
        Keycode::MetaLeft => K::MetaLeft,
        Keycode::MetaRight => K::MetaRight,
        Keycode::DpadLeft => K::ArrowLeft,
        Keycode::DpadRight => K::ArrowRight,
        Keycode::DpadUp => K::ArrowUp,
        Keycode::DpadDown => K::ArrowDown,
        Keycode::MoveHome => K::Home,
        Keycode::MoveEnd => K::End,
        Keycode::PageUp => K::PageUp,
        Keycode::PageDown => K::PageDown,
        Keycode::Insert => K::Insert,
        Keycode::Minus => K::Minus,
        Keycode::Equals => K::Equal,
        Keycode::LeftBracket => K::BracketLeft,
        Keycode::RightBracket => K::BracketRight,
        Keycode::Backslash => K::Backslash,
        Keycode::Semicolon => K::Semicolon,
        Keycode::Apostrophe => K::Quote,
        Keycode::Grave => K::Backquote,
        Keycode::Comma => K::Comma,
        Keycode::Period => K::Period,
        Keycode::Slash => K::Slash,
        Keycode::Numpad0 => K::Numpad0,
        Keycode::Numpad1 => K::Numpad1,
        Keycode::Numpad2 => K::Numpad2,
        Keycode::Numpad3 => K::Numpad3,
        Keycode::Numpad4 => K::Numpad4,
        Keycode::Numpad5 => K::Numpad5,
        Keycode::Numpad6 => K::Numpad6,
        Keycode::Numpad7 => K::Numpad7,
        Keycode::Numpad8 => K::Numpad8,
        Keycode::Numpad9 => K::Numpad9,
        Keycode::NumpadAdd => K::NumpadAdd,
        Keycode::NumpadSubtract => K::NumpadSubtract,
        Keycode::NumpadMultiply => K::NumpadMultiply,
        Keycode::NumpadDivide => K::NumpadDivide,
        Keycode::NumpadDot => K::NumpadDecimal,
        Keycode::NumLock => K::NumLock,
        Keycode::ScrollLock => K::ScrollLock,
        Keycode::Break => K::Pause,
        other => K::Other(u32::from(other)),
    }
}
