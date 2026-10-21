//! iOS (UIKit) backend.
//! iOS (UIKit) 后端。
//!
//! Same inversion as Android: iOS owns the entry point and lifecycle, so
//! events flow through a **process-global buffer** drained by
//! `poll_event`. v0 bootstrap is synchronous and delegate-less:
//! `Window::new` lazily creates the `UIApplication` singleton + a
//! full-screen `UIWindow` via the (deprecated but functional)
//! `initWithFrame:` path, and the host loop pumps the main runloop with a
//! zero-timeout `runMode:beforeDate:` on every poll so system machinery
//! stays responsive.
//! 与 Android 相同的反转：iOS 掌握入口与生命周期，事件流经**进程级
//! 全局缓冲**由 `poll_event` 排空。v0 装配是同步且无委托的：
//! `Window::new`（惰性）创建 `UIApplication` 单例 + 全屏 `UIWindow`
//! （走已弃用但可用的 `initWithFrame:` 路径），宿主循环每次 poll 时以
//! 零超时 `runMode:beforeDate:` 泵一次主 runloop，保持系统机制运转。
//!
//! Conventions:
//! 约定：
//! - iOS windows are inherently full-screen, borderless and
//!   non-resizable; the corresponding capability methods are no-ops with
//!   fixed answers (the platform support matrix).
//!   iOS 窗口天然全屏、无边框、不可调；对应能力方法为 no-op + 固定值
//!   （平台支持矩阵）。
//! - Sizes / coordinates are physical pixels (bounds × scale).
//!   尺寸 / 坐标为物理像素（bounds × scale）。
//! - Event injection: `ios_inject_test_event` (crate root, doc-hidden)
//!   feeds synthetic events into the same buffer the OS path would use —
//!   the deterministic event-test path on the simulator.
//!   事件注入：`ios_inject_test_event`（crate 根，doc-hidden）将合成
//!   事件写入与 OS 路径相同的缓冲——模拟器上的确定性事件测试通道。
//!
//! v0 boundaries (documented, follow-ups): no UIApplicationDelegate
//! lifecycle (no `Suspended`/`Resumed` yet), real touch delivery requires
//! a responder subclass (next step), windows use the deprecated
//! initWithFrame path on purpose (simulator-verified).
//! v0 边界（已记录，后续项）：尚无 UIApplicationDelegate 生命周期
//! （`Suspended`/`Resumed` 未接入）、真实触摸投递需 responder 子类
//! （下一步）、有意使用已弃用的 initWithFrame 路径（模拟器已验证）。

pub(crate) mod entry;

use std::collections::VecDeque;
use std::sync::Mutex;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_core_foundation::CGRect;
// The v0 bootstrap intentionally uses the deprecated mainScreen paths (see
// module docs).
// v0 装配有意使用已弃用的 mainScreen 路径（见模块文档）。
#[allow(deprecated)]
use objc2_ui_kit::{UIApplication, UIColor, UIScreen, UIViewController, UIWindow};

use crate::error::WindowError;
use crate::event::Event;
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

/// Events translated from OS callbacks and the test-injection hook.
/// OS 回调与测试注入钩子翻译出的待发事件。
static PENDING: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// Last known client size (physical pixels), maintained at creation.
/// 最近一次已知客户区尺寸（物理像素），创建时维护。
static SIZE: Mutex<(u32, u32)> = Mutex::new((0, 0));

/// Pushes an event into the buffer (shared by the test hook).
/// 将事件写入缓冲（测试钩子共用）。
fn push_event(event: Event) {
    PENDING
        .lock()
        .expect("PENDING poisoned / PENDING 锁中毒")
        .push_back(event);
}

/// Test-only event injection: feeds a synthetic event into the same buffer
/// the OS path uses (the deterministic "simulated event send" on the
/// simulator — real touch delivery requires a responder subclass, a
/// follow-up).
/// 仅测试用事件注入：将合成事件写入与 OS 路径相同的缓冲——模拟器上的
/// 确定性"模拟发送事件"通道（真实触摸投递需 responder 子类，属后续项）。
pub fn inject_test_event(event: Event) {
    push_event(event);
}

pub(crate) struct WindowInner {
    /// Captured at creation; also makes `WindowInner` `!Send`, enforcing
    /// the main-thread contract at the type level.
    /// 创建时捕获；同时使 `WindowInner` 为 `!Send`，在类型层面强制主线程
    /// 契约。
    _mtm: MainThreadMarker,
    /// Held to keep the UIApplication singleton alive for the window's
    /// lifetime.
    /// 保活 UIApplication 单例于窗口存活期内。
    _app: Retained<UIApplication>,
    window: Retained<UIWindow>,
}

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        // The main-thread marker both gates this call and lives in the
        // struct, enforcing affinity for every later call.
        // 主线程标记既门禁本次调用，也保存在结构体里，为后续所有调用
        // 强制线程亲和。
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(WindowError::OsError(
                "not on the main thread (UIKit requires it) / 不在主线程（UIKit 要求）".into(),
            ));
        };

        // Lazily instantiates the UIApplication singleton (valid on the
        // main thread without UIApplicationMain).
        // 惰性创建 UIApplication 单例（主线程上、不经 UIApplicationMain
        // 亦合法）。
        let app = UIApplication::sharedApplication(mtm);

        // iOS windows are inherently full-screen: the frame is the main
        // screen's bounds (points); the requested size is noted but the
        // platform wins.
        // iOS 窗口天然全屏：frame = 主屏 bounds（点）；请求尺寸仅记录，
        // 平台优先。
        let _ = options.size;
        let bounds = screen_bounds(mtm);
        let window = unsafe {
            // initWithFrame is deprecated in favor of init(windowScene:) but
            // remains functional — including in the simulator (v0 choice,
            // see module docs).
            // initWithFrame 已弃用（推荐 init(windowScene:)）但仍可用——
            // 模拟器内亦然（v0 选择，见模块文档）。
            UIWindow::initWithFrame(mtm.alloc::<UIWindow>(), bounds)
        };
        // Per-pixel transparency ready (clear background); off by default.
        // 逐像素透明就绪（清澈背景色）；默认关闭。
        if options.transparent {
            window.setOpaque(false);
            let clear = unsafe { UIColor::clearColor() };
            window.setBackgroundColor(Some(&clear));
        }

        // UIKit asserts at the end of application launch that the key
        // window has a root view controller (NSInternalInconsistencyException
        // otherwise). v0: a bare controller, no view hierarchy of its own.
        // UIKit 在应用启动结束时断言 key window 必须有 root view
        // controller（否则抛 NSInternalInconsistencyException）。
        // v0：裸控制器，自身无视图层级。
        let root = unsafe { UIViewController::init(mtm.alloc::<UIViewController>()) };
        window.setRootViewController(Some(&root));

        // Physical size cache: bounds (points) × scale.
        // 物理尺寸缓存：bounds（点）× scale。
        *SIZE.lock().expect("SIZE poisoned / SIZE 锁中毒") = physical_size(mtm);

        if !options.hidden {
            window.makeKeyAndVisible();
        }

        Ok(Self {
            _mtm: mtm,
            _app: app,
            window,
        })
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        // Zero-timeout runloop pump: drains ready sources without blocking,
        // keeping UIApplication machinery alive inside the host-owned loop.
        // 零超时 runloop 泵：排空就绪源且不阻塞，使 UIApplication 机制在
        // 宿主自持的循环内存活。
        objc2_core_foundation::CFRunLoop::run_in_mode(
            unsafe { objc2_core_foundation::kCFRunLoopDefaultMode },
            0.0,
            false,
        );
        PENDING
            .lock()
            .expect("PENDING poisoned / PENDING 锁中毒")
            .pop_front()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        *SIZE.lock().expect("SIZE poisoned / SIZE 锁中毒")
    }

    pub(crate) fn set_title(&self, _title: &str) {
        // iOS has no window-title concept; ignored.
        // iOS 无窗口标题概念，忽略。
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        // SAFETY: main thread (contract); the layer is owned by the view
        // hierarchy, retained by the window.
        // SAFETY: 主线程（契约）；layer 由视图层级持有，窗口保活。
        let ui_view = Retained::as_ptr(&self.window) as *mut std::ffi::c_void;
        let layer = unsafe { self.window.layer() };
        let layer_ptr = Retained::as_ptr(&layer) as *mut std::ffi::c_void;
        RawWindowHandle::UiKit {
            ui_view,
            layer: layer_ptr,
        }
    }

    // ── Runtime window capabilities (SDL flag semantics; the iOS column
    //    of the platform matrix) / 运行期窗口能力（SDL flag 语义；平台
    //    矩阵 iOS 列）────────────────────────────────────────────────

    pub(crate) fn set_resizable(&self, _resizable: bool) {
        // Inherently screen-sized: nothing to resize.
        // 天然随屏：无可调整。
    }

    pub(crate) fn is_resizable(&self) -> bool {
        false
    }

    pub(crate) fn set_size(&self, _width: u32, _height: u32) {
        // Inherently screen-sized: ignored.
        // 天然随屏：忽略。
    }

    pub(crate) fn set_fullscreen(&self, _fullscreen: bool) {
        // Inherently fullscreen.
        // 天然全屏。
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        true
    }

    pub(crate) fn set_borderless(&self, _borderless: bool) {}

    pub(crate) fn is_borderless(&self) -> bool {
        true
    }

    pub(crate) fn set_mouse_grabbed(&self, _grabbed: bool) {
        // No mouse on iOS.
        // iOS 无鼠标。
    }

    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        false
    }

    pub(crate) fn set_mouse_relative(&self, _enable: bool) {}

    pub(crate) fn set_cursor_visible(&self, _visible: bool) {}

    pub(crate) fn is_cursor_visible(&self) -> bool {
        false
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        self.window.setHidden(!visible);
    }

    pub(crate) fn is_visible(&self) -> bool {
        !self.window.isHidden()
    }

    pub(crate) fn set_always_on_top(&self, _top: bool) {
        // Single full-screen window: inherently on top.
        // 单一全屏窗口：天然置顶。
    }

    pub(crate) fn is_always_on_top(&self) -> bool {
        true
    }

    pub(crate) fn minimize(&self) {}

    pub(crate) fn maximize(&self) {}

    pub(crate) fn restore(&self) {}

    pub(crate) fn raise(&self) {
        self.window.makeKeyAndVisible();
    }

    pub(crate) fn is_minimized(&self) -> bool {
        false
    }

    pub(crate) fn is_maximized(&self) -> bool {
        true
    }

    pub(crate) fn has_input_focus(&self) -> bool {
        self.window.isKeyWindow()
    }

    pub(crate) fn is_occluded(&self) -> bool {
        false
    }

    pub(crate) fn set_keyboard_grabbed(&self, _grab: bool) {}

    pub(crate) fn set_position(&self, _x: i32, _y: i32) {
        // Inherently screen-positioned.
        // 天然随屏。
    }

    pub(crate) fn position(&self) -> (i32, i32) {
        (0, 0)
    }

    pub(crate) fn center_on_screen(&self) {}

    pub(crate) fn title(&self) -> String {
        String::new()
    }

    pub(crate) fn set_opacity(&self, opacity: f32) {
        self.window.setAlpha(f64::from(opacity.clamp(0.0, 1.0)));
    }

    pub(crate) fn opacity(&self) -> f32 {
        self.window.alpha() as f32
    }

    pub(crate) fn dpi_scale(&self) -> f32 {
        // SAFETY: main thread (contract); UIScreen query.
        // SAFETY: 主线程（契约）；UIScreen 查询。
        unsafe { UIScreen::mainScreen(self._mtm) }.scale() as f32
    }

    pub(crate) fn high_pixel_density(&self) -> bool {
        true
    }
}

// SAFETY: `Sync` exists only to satisfy renderer bounds (wgpu's
// `create_surface(&window)` requires shared references); they only read
// handle values. All methods remain bound to the main thread (see `Window`
// docs) — the same trade-off winit makes on iOS.
// SAFETY: `Sync` 仅为满足渲染后端的 bound（wgpu `create_surface(&window)`
// 要求共享引用）；它们只读取句柄值。所有方法仍限主线程（见 `Window` 文档）
// ——与 winit 在 iOS 上的取舍一致。
unsafe impl Sync for WindowInner {}

impl Drop for WindowInner {
    fn drop(&mut self) {
        // Hide on release; the Retained drop deallocates the UIWindow.
        // 释放时隐藏；Retained 的 drop 销毁 UIWindow。
        self.window.setHidden(true);
    }
}

/// Bounds of the main screen in points (for the window frame).
/// 主屏 bounds（点），用于窗口 frame。
fn screen_bounds(mtm: MainThreadMarker) -> CGRect {
    // SAFETY: main thread (contract); UIScreen query.
    // SAFETY: 主线程（契约）；UIScreen 查询。
    unsafe { UIScreen::mainScreen(mtm).bounds() }
}

/// Physical size of the main screen (bounds × scale).
/// 主屏物理尺寸（bounds × scale）。
fn physical_size(mtm: MainThreadMarker) -> (u32, u32) {
    let bounds = screen_bounds(mtm);
    let scale = unsafe { UIScreen::mainScreen(mtm) }.scale() as f64;
    (
        ((bounds.size.width * scale).round() as u32).max(1),
        ((bounds.size.height * scale).round() as u32).max(1),
    )
}
