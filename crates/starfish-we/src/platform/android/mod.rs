//! Android (NativeActivity) backend.
//! Android (NativeActivity) åç«¯ã
//!
//! Android's entry point and lifecycle are system-driven â the inverse of
//! this crate's poll model. After [`android_init`] wires up the system
//! callbacks, [`poll_event`](WindowInner::poll_event) pumps one round of
//! `AndroidApp::poll_events` per call (timeout = 0, non-blocking),
//! **translating** lifecycle / input events into a buffer queue and
//! popping one from the head â the "callbacks write, poll drains"
//! inversion device.
//! Android çå¥å£ä¸çå½å¨æç±ç³»ç»é©±å¨ï¼ä¸æ¬ crate ç poll æ¨¡åç¸åï¼
//! [`android_init`] è£éç³»ç»åè°åï¼[`poll_event`](WindowInner::poll_event)
//! å¨æ¯æ¬¡è°ç¨æ¶æ³µä¸è½® `AndroidApp::poll_events`ï¼timeout=0 éé»å¡ï¼ï¼
//! æçå½å¨æ / è¾å¥äºä»¶**ç¿»è¯è¿ç¼å²éå**åä»éå¤´åä¸ä¸ªââå³"åè°åå¥ã
//! poll æç©º"çåè½¬è£ç½®ã
//!
//! Conventions (matching the public docs):
//! çº¦å®ï¼ä¸å¬å±ææ¡£ä¸è´ï¼ï¼
//! - Sizes / coordinates are physical pixels (ANativeWindow coordinates
//!   are physical pixels).
//!   åæ  / å°ºå¯¸ä¸ºç©çåç´ ï¼ANativeWindow åæ å³ç©çåç´ ï¼ã
//! - Touch â `Event::Touch`, with the primary finger simultaneously
//!   synthesized into mouse events.
//!   è§¦æ¸ â `Event::Touch`ï¼ä¸»ææï¼pointer id 0ï¼åæ¶åæé¼ æ äºä»¶ã
//! - Lifecycle: `Pause`/`Stop`/`TerminateWindow` â `Suspended`,
//!   `Resume` â `Resumed`, `Destroy` â `CloseRequested`.
//!   çå½å¨æï¼`Pause`/`Stop`/`TerminateWindow` â `Suspended`ï¼
//!   `Resume` â `Resumed`ï¼`Destroy` â `CloseRequested`ã
//! - The back key (`Keycode::Back`) â `CloseRequested` (Android app
//!   convention).
//!   è¿åé®ï¼`Keycode::Back`ï¼â `CloseRequested`ï¼Android åºç¨æ¯ä¾ï¼ã
//! - `TextInput` comes from the system IME's committed text
//!   (`InputEvent::TextEvent`).
//!   `TextInput` æ¥èªç³»ç» IME çæäº¤ææ¬ï¼`InputEvent::TextEvent`ï¼ã
//!
//! All calls (`Window::new` / `poll_event`) must happen on the
//! `android_main` thread (android-activity asserts this in `poll_events`).
//! ææè°ç¨ï¼`Window::new` / `poll_event`ï¼å¿é¡»å¨ `android_main` çº¿ç¨
//! ï¼android-activity ç `poll_events` ææ­¤è¦æ±ï¼ã

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub(crate) mod entry;

use android_activity::input::{InputEvent, KeyAction, Keycode, MotionAction};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};

use crate::error::WindowError;
use crate::event::{Event, KeyCode, Modifiers, MouseButton, TouchPhase};
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

/// The app handle wired up by `android_main`.
/// `android_main` è£éè¿æ¥ç app å¥æã
///
/// Contract: Android caches processes â "back" only destroys the Activity
/// while the process survives; on the next launch `android_main` runs
/// again **in the same process, on a new thread**. This slot must
/// therefore be updated by overwrite: with a `OnceLock`, the second
/// `android_init` would be silently ignored and every later call would
/// land on the previous run's destroyed app/looper â a crash on relaunch
/// (android-activity's poll thread assertion panics).
/// å¥çº¦ï¼Android ä¼ç¼å­è¿ç¨ââç¨æ·"è¿å"åªéæ¯ Activityï¼è¿ç¨å­æ´»ï¼
/// åæ¬¡å¯å¨æ¶ `android_main` ä¼å¨**åä¸è¿ç¨ãæ°çº¿ç¨**ä¸åæ¬¡æ§è¡ã
/// å æ­¤è¿éå¿é¡»è¦çå¼æ´æ°ï¼ç¨ `OnceLock` çè¯ç¬¬äºæ¬¡è£éè¢«éé»å¿½ç¥ï¼
/// ä¹åææè°ç¨é½è½å¨ä¸æ¬¡å·²éæ¯ç app / looper ä¸ââäºæ¬¡å¯å¨éªé
/// ï¼android-activity ç poll çº¿ç¨æ­è¨ç´æ¥ panicï¼ã
static APP: Mutex<Option<AndroidApp>> = Mutex::new(None);

/// Events translated from platform callbacks (single window; process-wide
/// buffer).
/// å¹³å°åè°ç¿»è¯åºçå¾åäºä»¶ï¼åçªå£ï¼è¿ç¨çº§ç¼å²ï¼ã
static PENDING: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// Whether this run has seen `InitWindow` (surface ready).
/// æ¬æ¬¡è¿è¡æ¯å¦å·²è§è¿ `InitWindow`ï¼surface å°±ç»ªï¼ã
///
/// Used to drop the **previous instance's cross-talk**: a late `onDestroy`
/// from the old activity can land in the new instance's event stream;
/// translating it as-is yields a `CloseRequested`, presenting as "the app
/// exits immediately on relaunch".
/// ç¨äºä¸¢å¼è¿ç¨å¤ç¨æ¶ç**æ§å®ä¾ä¸²æ°**ï¼ä¸ä¸å®ä¾ç onDestroy å¯è½æå°ã
/// è½è¿æ°å®ä¾çäºä»¶æµï¼è¥ç§åå¨æ¶ä¼è¢«ç¿»è¯æ `CloseRequested`ï¼
/// è¡¨ç°ä¸º"åæ¬¡å¯å¨ç§é"ã
static WINDOW_READY: AtomicBool = AtomicBool::new(false);

/// Called by the host in `android_main` to wire up the system callbacks.
/// å®¿ä¸»å¨ `android_main` éè°ç¨ï¼è£éç³»ç»åè°ã
///
/// Supports multiple calls within one process (when the process is cached
/// and reused, `android_main` runs a second time): overwrites the old app,
/// clears the previous run's buffered events and resets the readiness
/// flag.
/// æ¯æåè¿ç¨åå¤æ¬¡è°ç¨ï¼è¿ç¨è¢«ç³»ç»ç¼å­å¤ç¨æ¶ï¼`android_main` ä¼äºæ¬¡
/// æ§è¡ï¼ï¼è¦çæ§ appãæ¸ç©ºä¸æ¬¡è¿è¡éççäºä»¶ç¼å²ãéç½®å°±ç»ªæ è®°ã
pub fn android_init(app: AndroidApp) {
    *APP.lock().expect("APP lock poisoned / APP éä¸­æ¯") = Some(app);
    // The single-window slot is counted per "run": during the overlap the
    // previous instance may still hold the slot.
    // åçªå£å ä½æ"è¿è¡"è®¡ç®ï¼äº¤å æåä¸ä¸å®ä¾å¯è½è¿å çä½å­ã
    crate::window::reset_single_window_slot();
    {
        PENDING.lock().expect("PENDING éä¸­æ¯").clear();
        WINDOW_READY.store(false, Ordering::Release);
    }
}

// ââ åµå¥æ¨¡å¼ï¼å¤é¨ Surfaceï¼2026-10-26ï¼ï¼å®¿ä¸» app ç» JNI æ³¨å¥èªå·±
// SurfaceView ç ANativeWindowï¼NativeActivity/glue ä¸åä¸ââåµå¥åºæ¯
// å®¿ä¸»æ¯æ®é Kotlin Activityï¼ââââââââââââââââââââââââââââââââââââ
// è£éå£ = attach_external_windowï¼JNI è½¬åï¼ï¼äºä»¶ = æ³¨å¥ PENDINGï¼
// åæ´¾æå¨é¡µé¢/å®¿ä¸»ä¾§ï¼web å®¿ä¸» boot è£å³çåæè®¾è®¡ï¼ã

/// å¤é¨æ³¨å¥çªå£ï¼ANativeWindow å¼ç¨ï¼fromSurface è·å¾_acquireï¼detach
/// æ¶ releaseï¼+ ä½å¾å°ºå¯¸ã
struct ExternalWindow {
    ptr: *mut std::ffi::c_void,
    width: u32,
    height: u32,
}

static EXTERNAL: Mutex<Option<ExternalWindow>> = Mutex::new(None);

// ANativeWindow å¼ç¨è·¨çº¿ç¨ä¼ éå®å¨ï¼Android å¾å½¢å¥æçº¿ç¨æ å³ï¼
// éæ¾èµ° detach ç release è°ç¨ï¼
unsafe impl Send for ExternalWindow {}

/// åµå¥æ¨¡å¼æ¯å¦æ¿æ´»ï¼glue è·¯å¾çç­è·¯å¼å³ï¼
pub fn external_active() -> bool {
    EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// åµå¥å®¿ä¸»æ³¨å¥å¤é¨çªå£ï¼`ANativeWindow_fromSurface` çè¿åå¼ç¨ï¼
/// å¼ç¨è®¡æ°å·² +1ï¼[`detach_external_window`] æ¶ releaseï¼ã
///
/// # Safety
/// `ptr` å¿é¡»æ¯ææç `ANativeWindow*`ï¼fromSurface è¿åå¼ï¼ã
pub unsafe fn attach_external_window(ptr: *mut std::ffi::c_void, width: u32, height: u32) {
    // attach 即新场开演（进程一次契约下，每次 attach = 一轮新运行）：
    // 清上一场残留事件（detach 落下的 CloseRequested / 旧 Resized / 触摸
    // ——否则新场首泵吃到旧场 CloseRequested 立即收口，"第二次运行黑屏"
    // 真机实证）；释放单窗槽位（窗口生命周期按"运行"而非"进程"计，
    // 镜像 android_init 的每场重置）
    {
        let mut queue = PENDING.lock().unwrap_or_else(|p| p.into_inner());
        queue.clear();
    }
    crate::window::reset_single_window_slot();
    *EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()) =
        Some(ExternalWindow { ptr, width, height });
    PENDING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push_back(Event::Resized { width, height });
}

/// åµå¥å®¿ä¸»è§£é¤çªå£ï¼SurfaceView éæ¯ï¼push CloseRequested â å¼æ
/// å¸§å¾ªç¯èªç¶æ¶å£ââä¸ NativeActivity æ¨¡å¼ç Destroy è¯­ä¹ä¸è´ï¼
pub fn detach_external_window(env: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) {
    // 无条件清槽 + push CloseRequested（引擎帧循环自然收口，与
    // NativeActivity 的 Destroy 语义一致）。陈旧 detach（换场重叠期旧
    // 场销毁晚于新场 attach）由 **Java 层代数守卫**拦截——ANCO 实证：
    // fromSurface 的指针比对不可靠（同 Surface 未必返回同一
    // ANativeWindow，比对失败 = 合法 detach 被误忽略 = 引擎永不收口，
    // 声音不停 + 新场黑屏）。
    let query = unsafe { ANativeWindow_fromSurface(env, surface) };
    if query.is_null() {
        return;
    }
    *EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()) = None;
    // 槽位引用（attach 时 +1）的对消延迟到引擎完全停止（防
    // present-after-release）：暂存指针，壳收尾经
    // release_pending_external 归还
    *PENDING_RELEASE.lock().unwrap_or_else(|p| p.into_inner()) = Some(query as usize);
    unsafe { ANativeWindow_release(query) };
    PENDING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push_back(Event::CloseRequested);
}

/// 待物理释放的窗口引用（detach 暂存；引擎完全停止后由壳经
/// [`release_pending_external`] 精确取走——usize 存裸指针绕开
/// static Send 约束）
static PENDING_RELEASE: std::sync::Mutex<Option<usize>> = std::sync::Mutex::new(None);

/// 物理释放 detach 暂存的窗口引用（引擎完全停止后由壳调用一次；
/// 空 = no-op。只动暂存槽，不碰当前窗口——换场安全）
pub fn release_pending_external() {
    if let Ok(mut pending) = PENDING_RELEASE.lock() {
        if let Some(ptr) = pending.take() {
            unsafe { ANativeWindow_release(ptr as *mut std::ffi::c_void) };
        }
    }
}

/// åµå¥å®¿ä¸»æ³¨å¥è§¦æ¸ï¼ä¸»ææ â é¼ æ äºä»¶åæï¼SDL æ¯ä¾ââä¸ glue è¾å¥
/// ç¿»è¯åè¯­ä¹ï¼ãactionï¼0 = downï¼1 = moveï¼2 = upãåæ  = ä½å¾åç´ ã
pub fn inject_external_touch(action: u8, x: f64, y: f64) {
    use crate::event::MouseButton;
    let mut queue = PENDING.lock().unwrap_or_else(|p| p.into_inner());
    match action {
        0 => {
            queue.push_back(Event::MouseMoved { x, y });
            queue.push_back(Event::MouseDown { button: MouseButton::Left });
        }
        1 => queue.push_back(Event::MouseMoved { x, y }),
        2 => queue.push_back(Event::MouseUp { button: MouseButton::Left }),
        _ => {}
    }
}

// 坌入宿主的尺寸变化事件（Resized 事件入队）
pub fn inject_external_resized(width: u32, height: u32) {
    PENDING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push_back(Event::Resized { width, height });
}

pub(crate) struct WindowInner;

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        // åµå¥æ¨¡å¼ï¼çªå£å·²ç±å®¿ä¸»æ³¨å¥ï¼è·³è¿ glue ç­å¾
        if external_active() {
            return Ok(Self);
        }
        let app = app_handle()?;
        // NativeActivity's surface only exists after `InitWindow`: block
        // and wait (the android_main thread may block), capped at 10 s.
        // NativeActivity ç surface å¨ `InitWindow` ä¹åæå­å¨ï¼
        // é»å¡ç­å¾ï¼android_main çº¿ç¨åè®¸é»å¡ï¼ï¼ä¸é 10 ç§ã
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
        // åµå¥æ¨¡å¼ï¼äºä»¶ç±æ³¨å¥å£ç´å¥ PENDINGï¼æ  glue æ³µï¼
        if external_active() {
            return PENDING
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .pop_front();
        }
        let app = app_handle().ok()?;
        // Poison-tolerant lock: recover the queue after a concurrent panic
        // (e.g. the thread assertion during process-reuse overlap) instead
        // of cascading â during overlap the new instance takes over anyway.
        // éä¸­æ¯å®¹éï¼å¹¶å panicï¼å¦è¿ç¨å¤ç¨äº¤å æççº¿ç¨æ­è¨ï¼åæ¢å¤éå
        // å¹¶æ¸ç©ºï¼ä¸è¿ç¯çââäº¤å ææ¬å°±è¯¥ç±æ°å®ä¾æ¥ç®¡ã
        let mut queue = PENDING.lock().unwrap_or_else(|poisoned| {
            let mut queue = poisoned.into_inner();
            queue.clear();
            queue
        });

        // 1) Pump lifecycle events (non-blocking).
        // 1) æ³µçå½å¨æäºä»¶ï¼éé»å¡ï¼ã
        app.poll_events(Some(Duration::ZERO), |poll_event| {
            if let PollEvent::Main(main) = poll_event {
                translate_main(&app, main, &mut queue);
            }
        });

        // 2) Pump input events (android-activity's callback-style input
        //    iterator).
        // 2) æ³µè¾å¥äºä»¶ï¼android-activity çè¾å¥è¿­ä»£å¨ï¼åè°å¼æ¶è´¹ï¼ã
        if let Ok(mut iter) = app.input_events_iter() {
            while iter.next(|event| {
                translate_input(&mut queue, event);
                InputStatus::Handled
            }) {}
        }

        queue.pop_front()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        // åµå¥æ¨¡å¼ï¼å¤é¨çªå£å°ºå¯¸ä¼å
        if let Some(ext) = EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            return (ext.width, ext.height);
        }
        native_window_size().unwrap_or((0, 0))
    }

    pub(crate) fn set_title(&self, _title: &str) {
        // Android has no window-title concept; ignored.
        // Android æ çªå£æ é¢æ¦å¿µï¼å¿½ç¥
    }

    // ââ Runtime window capabilities (the Android support matrix) /
    //    è¿è¡æçªå£è½åï¼Android å¹³å°ç©éµï¼ââââââââââââââââââââââââ
    // Naturally fullscreen (follows the system surface); window-decoration
    // concepts like NOFRAME / ALWAYS_ON_TOP / min-max do not apply to
    // Android â the corresponding methods are warn-once no-ops.
    // å¤©ç¶å¨å±ï¼è·éç³»ç» surfaceï¼ï¼NOFRAME/ALWAYS_ON_TOP/min-max ç­
    // çªå£è£é¥°æ¦å¿µä¸éç¨äº Androidââå¯¹åºæ¹æ³ warn-once no-opã
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
        // æ¡é¢/Web/Androidï¼ææ¬éæé®äºä»¶èªå¨éè¾¾ï¼æ è½¯é®çå¯æ§æ¾éã
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
        // åµå¥æ¨¡å¼ï¼å¤é¨æ³¨å¥ç ANativeWindow ä¼å
        if let Some(ext) = EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            return RawWindowHandle::Android {
                a_native_window: ext.ptr,
            };
        }
        // Returns a null pointer while the surface is absent (reclaimed in
        // the background); callers should only hand it to a rendering
        // backend while `Resumed`.
        // surface ä¸å¨æ¶ï¼åå°è¢«åæ¶ï¼è¿åç©ºæéï¼è°ç¨æ¹åºä»å¨
        // `Resumed` ç¶æä¸åæ¸²æåç«¯ä¼ éã
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
/// å½å appï¼Arc åéï¼å»ä»·ï¼ï¼æªè£éæ¶æ¥éã
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
/// çå½å¨æäºä»¶ç¿»è¯ã
fn translate_main(app: &AndroidApp, main: MainEvent<'_>, queue: &mut VecDeque<Event>) {
    use Event as E;
    match main {
        // Surface ready / reclaimed: the signal for when to rebuild the
        // wgpu surface.
        // surface å°±ç»ª / åæ¶ï¼è¿æ¯ wgpu surface éå»ºçæ¶æºä¿¡å·ã
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
        // å°±ç»ªåæ¶å°ç Destroy å¿ç¶æ¯ä¸ä¸å®ä¾çä¸²æ°ï¼æ­£å¸¸éæ¯ä¸å®åçå¨
        // InitWindow ä¹åï¼ï¼ä¸¢å¼ä»¥é²"åæ¬¡å¯å¨ç§é"ã
        MainEvent::Destroy => {
            if WINDOW_READY.load(Ordering::Acquire) {
                queue.push_back(E::CloseRequested);
            }
        }
        // Start/SaveState/ConfigChanged/LowMemory/ContentRectChanged/
        // InsetsChanged: ignored in the first version.
        // Start/SaveState/ConfigChanged/LowMemory/ContentRectChanged/InsetsChangedï¼é¦çå¿½ç¥
        _ => {
            let _ = app;
        }
    }
}

/// Input event translation: touch (with primary-finger mouse synthesis)
/// and keys / IME text.
/// è¾å¥äºä»¶ç¿»è¯ï¼è§¦æ¸ï¼å«ä¸»ææåæé¼ æ ï¼ä¸æé® / IME ææ¬ã
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
                // Hover / Scroll / ButtonPress ç­ï¼é¦çå¿½ç¥
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
                // ä¸»ææï¼id 0ï¼åæé¼ æ äºä»¶ï¼SDL æ¯ä¾ï¼ã
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
            // è¿åé® = å³é­è¯·æ±ï¼Android åºç¨æ¯ä¾ï¼ã
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
                // Multipleï¼é¿æè¿åï¼ï¼æä¸æ¬¡å¸¦ repeat ç KeyDown å¤ç
                KeyAction::Multiple | _ => E::KeyDown {
                    key: keycode_to_keycode(keycode),
                    modifiers,
                    repeat: true,
                },
            };
            queue.push_back(key_event);
        }
        // Final text committed by the system IME.
        // ç³»ç» IME æäº¤çæç»ææ¬ã
        InputEvent::TextEvent(state) => {
            if !state.text.is_empty() {
                queue.push_back(E::TextInput {
                    text: state.text.clone(),
                });
            }
        }
        // TextAction (IME action keys): ignored in the first version.
        // TextActionï¼IME å¨ä½é®ï¼ï¼é¦çå¿½ç¥
        _ => {}
    }
}

/// Android `Keycode` â [`KeyCode`]: ranges + item-by-item mapping;
/// unrecognized keys become [`KeyCode::Other`] (carrying the native
/// AKEYCODE value) so no key is lost.
/// Android `Keycode` â [`KeyCode`]ï¼åºé´ + éé¡¹æ å°ï¼æªè¯å«åæ
/// [`KeyCode::Other`]ï¼æºå¸¦ AKEYCODE åçå¼ï¼ï¼ä¸ä¸¢æé®ã
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

// ── JNI 桥实现（starfish-we::embedded 门面的委托目标；libandroid.so
//    原生符号直连）────────────────────────────────────────────

unsafe extern "C" {
    fn ANativeWindow_fromSurface(
        env: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn ANativeWindow_release(window: *mut std::ffi::c_void);
    fn ANativeWindow_getWidth(window: *mut std::ffi::c_void) -> i32;
    fn ANativeWindow_getHeight(window: *mut std::ffi::c_void) -> i32;
}

/// [`crate::embedded::attach_surface`] 的委托目标（fromSurface 获得引用 +
/// 尺寸读取 + 装配登记）。# Safety 见 facade 文档。
pub(crate) unsafe fn attach_surface_impl(
    env: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
) -> (u32, u32) {
    let ptr = unsafe { ANativeWindow_fromSurface(env, surface) };
    let width = unsafe { ANativeWindow_getWidth(ptr).max(0) } as u32;
    let height = unsafe { ANativeWindow_getHeight(ptr).max(0) } as u32;
    attach_external_window(ptr, width, height);
    (width, height)
}

/// [`crate::embedded::surface_resized`] 的委托目标
pub(crate) fn surface_resized_impl(width: u32, height: u32) {
    inject_external_resized(width, height);
}

/// [`crate::embedded::detach_surface`] 的委托目标：比对清除 + push
/// CloseRequested（引擎帧循环收口）。ANativeWindow 引用的物理释放由壳
/// 在引擎收口后回调 [`release_external_window`]（防 present-after-release
/// OOB）。
///
/// # Safety
/// `env`/`surface` 必须是有效的 JNI 环境/Surface 句柄。
pub(crate) unsafe fn detach_surface_impl(
    env: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
) {
    unsafe { detach_external_window(env, surface) };
}

/// [`crate::embedded::inject_touch`] 的委托目标（action：0 down / 1 move / 2 up）
pub(crate) fn inject_touch_impl(action: u8, x: f64, y: f64) {
    inject_external_touch(action, x, y);
}
