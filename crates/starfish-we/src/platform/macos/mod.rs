//! macOS (AppKit) backend.
//! macOS (AppKit) 后端。
//!
//! Model: AppKit is main-thread-only, so every call here requires the main
//! thread (enforced by storing a [`MainThreadMarker`] — the type is
//! `!Send`, which makes `Window` inherit the affinity). `poll_event`
//! drains events non-blockingly with
//! `nextEventMatchingMask:untilDate:inMode:dequeue:` (expiration =
//! `distantPast`), dispatching each through `sendEvent` so the window
//! delegate fires; input events are translated directly from the pumped
//! `NSEvent`s, window-level events arrive via the delegate. Control flow
//! always stays with the caller.
//! 模型：AppKit 仅限主线程，因此这里的所有调用都要求主线程（通过保存
//! [`MainThreadMarker`] 强制——该类型是 `!Send` 的，使 `Window` 继承线程
//! 亲和）。`poll_event` 用 `nextEventMatchingMask:untilDate:inMode:dequeue:`
//! （过期时间 = `distantPast`）非阻塞泵事件，并经 `sendEvent` 派发以触发
//! 窗口委托；输入事件直接从泵出的 `NSEvent` 翻译，窗口级事件经委托到达。
//! 控制流始终在调用者手里。
//!
//! Conventions:
//! 约定：
//! - Sizes / coordinates are **physical pixels**: AppKit talks in points,
//!   so everything is multiplied by the window's `backingScaleFactor`
//!   before leaving this module.
//!   尺寸 / 坐标均为**物理像素**：AppKit 使用点（point），离开本模块前
//!   一律乘以窗口的 `backingScaleFactor`。
//! - Positions use a top-left screen origin (converted from AppKit's
//!   bottom-left system at the boundary).
//!   位置使用屏幕左上角原点（在边界处从 AppKit 的左下角坐标系换算）。
//! - `WM_CLOSE`-equivalent: `windowShouldClose` reports `CloseRequested`
//!   and returns NO — destruction is decided by dropping the
//!   [`Window`](crate::Window).
//!   等价于 `WM_CLOSE`：`windowShouldClose` 只上报 `CloseRequested` 并返回
//!   NO——销毁由 drop [`Window`](crate::Window) 决定。
//!
//! State sharing: this crate is single-window on AppKit, so the delegate
//! and the pump share process-global buffers (same pattern as the Android
//! backend) instead of ObjC ivars — simpler and immune to ivar-API drift.
//! 状态共享：本 crate 在 AppKit 上是单窗口，委托与泵共享进程级全局缓冲
//! （与 Android 后端同一模式），而非 ObjC ivar——更简单且不受 ivar API
//! 变动影响。

mod translate;

use std::collections::VecDeque;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadOnly, define_class};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSCursor, NSEvent, NSEventType,
    NSFloatingWindowLevel, NSNormalWindowLevel, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use crate::error::WindowError;
use crate::event::Event;
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

use translate::{button_from_index, keycode_to_keycode, modifiers_from_flags};

/// Events translated from the pump and the delegate.
/// 泵与委托翻译出的待发事件。
static PENDING: Mutex<VecDeque<Event>> = Mutex::new(VecDeque::new());

/// Last known client size (physical pixels), maintained by the delegate's
/// `windowDidResize`.
/// 最近一次已知客户区尺寸（物理像素），由委托的 `windowDidResize` 维护。
static SIZE: Mutex<(u32, u32)> = Mutex::new((0, 0));

pub(crate) struct WindowInner {
    /// Captured at creation; also makes `WindowInner` `!Send`, enforcing
    /// the main-thread contract at the type level.
    /// 创建时捕获；同时使 `WindowInner` 为 `!Send`，在类型层面强制主线程
    /// 契约。
    _mtm: MainThreadMarker,
    app: Retained<NSApplication>,
    window: Retained<NSWindow>,
    /// Held so the delegate outlives any transient callback window.
    /// 持有委托，使其活过任何瞬时回调窗口。
    _delegate: Retained<WindowDelegate>,
}

// The delegate is stateless: callbacks read the notification/window,
// translate, and push into the process-global buffers above (the
// single-window contract makes this correct).
// 委托无状态：回调读取通知/窗口，翻译后写入上方进程级全局缓冲
// （单窗口契约使这正确）。
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct WindowDelegate;

    // NSWindowDelegate requires the implementer to be NSObjectProtocol.
    // NSWindowDelegate 要求实现方满足 NSObjectProtocol。
    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        // Close button pressed: report the intent and refuse the actual
        // close (destruction is decided by dropping the Window).
        // 点击关闭按钮：只上报意图并拒绝真正关闭（销毁由 drop Window 决定）。
        #[unsafe(method(windowShouldClose:))]
        unsafe fn window_should_close(&self, _sender: &NSWindow) -> Bool {
            PENDING
                .lock()
                .expect("PENDING poisoned / PENDING 锁中毒")
                .push_back(Event::CloseRequested);
            Bool::NO
        }

        // Client size changed: update the shared size cache and report.
        // 客户区尺寸变化：更新共享尺寸缓存并上报。
        #[unsafe(method(windowDidResize:))]
        unsafe fn window_did_resize(&self, notification: &objc2_foundation::NSNotification) {
            if let Some(window) = window_from_notification(notification) {
                let (width, height) = client_size_physical(&window);
                *SIZE.lock().expect("SIZE poisoned / SIZE 锁中毒") = (width, height);
                PENDING
                    .lock()
                    .expect("PENDING poisoned / PENDING 锁中毒")
                    .push_back(Event::Resized { width, height });
            }
        }

        #[unsafe(method(windowDidBecomeKey:))]
        unsafe fn window_did_become_key(&self, _notification: &objc2_foundation::NSNotification) {
            PENDING
                .lock()
                .expect("PENDING poisoned / PENDING 锁中毒")
                .push_back(Event::FocusGained);
        }

        #[unsafe(method(windowDidResignKey:))]
        unsafe fn window_did_resign_key(&self, _notification: &objc2_foundation::NSNotification) {
            PENDING
                .lock()
                .expect("PENDING poisoned / PENDING 锁中毒")
                .push_back(Event::FocusLost);
        }

        // Minimize ≈ "content unavailable" in our vocabulary.
        // 最小化 ≈ 本库语义中的"内容暂不可用"。
        #[unsafe(method(windowDidMiniaturize:))]
        unsafe fn window_did_miniaturize(&self, _notification: &objc2_foundation::NSNotification) {
            PENDING
                .lock()
                .expect("PENDING poisoned / PENDING 锁中毒")
                .push_back(Event::Suspended);
        }

        #[unsafe(method(windowDidDeminiaturize:))]
        unsafe fn window_did_deminiaturize(
            &self,
            _notification: &objc2_foundation::NSNotification,
        ) {
            PENDING
                .lock()
                .expect("PENDING poisoned / PENDING 锁中毒")
                .push_back(Event::Resumed);
        }
    }
);

/// Extracts the NSWindow out of a delegate notification.
/// 从委托通知中提取 NSWindow。
fn window_from_notification(
    notification: &objc2_foundation::NSNotification,
) -> Option<Retained<NSWindow>> {
    let object = notification.object()?;
    object.downcast::<NSWindow>().ok()
}

/// Client size in physical pixels: the content view's point size × the
/// window's backing scale.
/// 物理像素客户区尺寸：内容视图点尺寸 × 窗口 backing scale。
fn client_size_physical(window: &NSWindow) -> (u32, u32) {
    let scale = window.backingScaleFactor();
    // SAFETY: AppKit call on the main thread.
    // SAFETY: 主线程上的 AppKit 调用。
    let size = window.contentView().expect("content view").frame().size;
    (
        ((size.width * scale).round() as u32).max(1),
        ((size.height * scale).round() as u32).max(1),
    )
}

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        // The main-thread marker both gates this call and lives in the
        // struct, enforcing affinity for every later call.
        // 主线程标记既门禁本次调用，也保存在结构体里，为后续所有调用
        // 强制线程亲和。
        let Some(mtm) = MainThreadMarker::new() else {
            return Err(WindowError::OsError(
                "not on the main thread (AppKit requires it) / 不在主线程（AppKit 要求）".into(),
            ));
        };

        // Bring the app up once (regular activation, like a real windowed
        // app). Without activating, the window server keeps loginwindow
        // frontmost and the app never receives distributed events.
        // 拉起应用（一次；Regular 激活策略，与真实窗口应用一致）。
        // 不激活的话，窗口服务器让 loginwindow 保持在最前，应用收不到
        // 分发的事件。
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
        app.finishLaunching();
        app.activate();

        let (width, height) = options.size;
        // The requested size is the CLIENT size in physical pixels; AppKit
        // wants points — divide by the main screen's current scale
        // (pre-window).
        // 请求尺寸是物理像素的客户区；AppKit 要点——先除以（主屏的）当前
        // 缩放（建窗前无窗口可查）。
        let scale = objc2_app_kit::NSScreen::mainScreen(mtm)
            .map(|s| s.backingScaleFactor())
            .unwrap_or(1.0);
        let rect = NSRect::new(
            NSPoint::new(100.0, 100.0),
            NSSize::new(
                (width as f64 / scale).max(1.0),
                (height as f64 / scale).max(1.0),
            ),
        );

        let style = if options.borderless {
            NSWindowStyleMask::Borderless
        } else {
            let mut style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable;
            if options.resizable {
                style |= NSWindowStyleMask::Resizable;
            }
            style
        };

        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc::<NSWindow>(),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setTitle(&objc2_foundation::NSString::from_str(&options.title));
        // Our Drop calls close() once; the default release-on-close would
        // double-release against the Retained.
        // 我们的 Drop 只调用一次 close()；默认的"关闭即释放"会与 Retained
        // 二次释放。
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        unsafe { window.setReleasedWhenClosed(false) };
        // NSWindow defaults to NOT delivering mouse-moved events; without
        // this the backend never sees MouseMoved outside of drags.
        // NSWindow 默认不派发 mouse-moved 事件；不开此开关，拖拽之外
        // 永远收不到 MouseMoved。
        window.setAcceptsMouseMovedEvents(true);

        // SAFETY: msg_send of `init` on a freshly allocated object.
        // SAFETY: 对新分配对象调用 `init`。
        let delegate: Retained<WindowDelegate> =
            unsafe { objc2::msg_send![WindowDelegate::alloc(mtm), init] };
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        // Per-pixel transparency: a non-opaque window over a clear
        // background; the swapchain's alpha composites onto the desktop.
        // 逐像素透明：非不透明窗口 + 清澈背景色；交换链 alpha 直接与桌面
        // 混合。
        if options.transparent {
            window.setOpaque(false);
            let color = NSColor::clearColor();
            window.setBackgroundColor(Some(&color));
        }
        // `layered` is a Windows-only channel (UpdateLayeredWindow); on
        // macOS transparency above already covers the use case.
        // `layered` 是 Windows 专属通道（UpdateLayeredWindow）；macOS 上
        // 上方的 transparency 已覆盖该用例。
        let _ = options.layered;

        // Client size → physical cache + initial show.
        // 客户区尺寸 → 物理缓存 + 初始显示。
        *SIZE.lock().expect("SIZE poisoned / SIZE 锁中毒") = (width, height);
        if !options.hidden {
            window.makeKeyAndOrderFront(None);
        }

        let this = Self {
            _mtm: mtm,
            app,
            window,
            _delegate: delegate,
        };

        // Apply post-create flags (the runtime method family works
        // directly on the creating thread).
        // post-create flag 应用（运行期方法族在创建线程上直接可用）。
        if options
            .flags
            .contains(crate::flags::WindowFlags::FULLSCREEN)
        {
            this.set_fullscreen(true);
        }
        if options
            .flags
            .contains(crate::flags::WindowFlags::MOUSE_GRABBED)
        {
            this.set_mouse_grabbed(true);
        }
        if options.flags.contains(crate::flags::WindowFlags::MINIMIZED) {
            this.minimize();
        }
        if options.flags.contains(crate::flags::WindowFlags::MAXIMIZED) {
            this.maximize();
        }
        Ok(this)
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        let mtm = self._mtm;
        // SAFETY: all AppKit calls happen on the main thread (enforced by
        // the stored MainThreadMarker and the thread-affinity contract).
        // SAFETY: 所有 AppKit 调用都发生在主线程（由保存的
        // MainThreadMarker 与线程亲和契约保证）。
        unsafe {
            // Drain available events; translate input events directly, and
            // dispatch window-level ones so the delegate fires.
            // 泵干可用事件；输入事件直接翻译，窗口级事件派发以触发委托。
            loop {
                let Some(event) = app_next_event(mtm, &self.app) else {
                    break;
                };
                translate_nsevent(mtm, &event);
                self.app.sendEvent(&event);
            }
        }
        PENDING
            .lock()
            .expect("PENDING poisoned / PENDING 锁中毒")
            .pop_front()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        *SIZE.lock().expect("SIZE poisoned / SIZE 锁中毒")
    }

    pub(crate) fn set_title(&self, title: &str) {
        self.window
            .setTitle(&objc2_foundation::NSString::from_str(title));
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        let ns_window = Retained::as_ptr(&self.window) as *mut std::ffi::c_void;
        let content_view = self.window.contentView().expect("content view / 内容视图");
        let ns_view = Retained::as_ptr(&content_view) as *mut std::ffi::c_void;
        RawWindowHandle::AppKit { ns_window, ns_view }
    }

    // ── Runtime window capabilities (SDL flag semantics on AppKit) /
    //    运行期窗口能力（SDL flag 语义的 AppKit 落地）──────────────────

    pub(crate) fn set_resizable(&self, resizable: bool) {
        self.with_style(|style| {
            if resizable {
                *style |= NSWindowStyleMask::Resizable;
            } else {
                *style &= !NSWindowStyleMask::Resizable;
            }
        });
    }

    pub(crate) fn is_resizable(&self) -> bool {
        self.window
            .styleMask()
            .contains(NSWindowStyleMask::Resizable)
    }

    pub(crate) fn set_size(&self, width: u32, height: u32) {
        // Physical → points, then set the CONTENT size, keeping the
        // top-left corner fixed.
        // 物理 → 点，然后设置内容尺寸，并保持左上角不动。
        let scale = self.window.backingScaleFactor();
        let frame = self.window.frame();
        let _content = self.window.contentRectForFrameRect(frame);
        self.window.setContentSize(NSSize::new(
            (width as f64 / scale).max(1.0),
            (height as f64 / scale).max(1.0),
        ));
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        self.window.setFrameTopLeftPoint(NSPoint::new(
            frame.origin.x,
            frame.origin.y + frame.size.height,
        ));
    }

    pub(crate) fn set_fullscreen(&self, fullscreen: bool) {
        self.with_style(|style| {
            if fullscreen {
                *style |= NSWindowStyleMask::FullScreen;
            } else {
                *style &= !NSWindowStyleMask::FullScreen;
            }
        });
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        self.window
            .styleMask()
            .contains(NSWindowStyleMask::FullScreen)
    }

    pub(crate) fn set_borderless(&self, borderless: bool) {
        self.with_style(|style| {
            if borderless {
                *style &= !NSWindowStyleMask::Titled;
            } else {
                *style |= NSWindowStyleMask::Titled;
            }
        });
    }

    pub(crate) fn is_borderless(&self) -> bool {
        !self.window.styleMask().contains(NSWindowStyleMask::Titled)
    }

    pub(crate) fn set_mouse_grabbed(&self, grabbed: bool) {
        // Confine the mouse to this window via CG (the associated-cursor
        // switch; the clip rect comes with raw-input work later).
        // 经 CG 将鼠标限定在本窗口（关联开关；裁剪矩形随 raw-input 工作
        // 后续补齐）。
        unsafe extern "C" {
            fn CGAssociateMouseAndMouseCursorPosition(connected: i32) -> i32;
        }
        // SAFETY: CoreGraphics call; harmless either way.
        // SAFETY: CoreGraphics 调用；两种取值均无害。
        unsafe {
            CGAssociateMouseAndMouseCursorPosition(if grabbed { 0 } else { 1 });
        }
    }

    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        // CG exposes no cheap query; report the opt-in default.
        // CG 无廉价查询接口；报告默认值。
        false
    }

    pub(crate) fn set_mouse_relative(&self, enable: bool) {
        self.set_mouse_grabbed(enable);
        self.set_cursor_visible(!enable);
    }

    pub(crate) fn set_cursor_visible(&self, visible: bool) {
        if visible {
            NSCursor::unhide();
        } else {
            NSCursor::hide();
        }
    }

    pub(crate) fn is_cursor_visible(&self) -> bool {
        // NSCursor exposes no query; assume visible (hide is opt-in).
        // NSCursor 无查询接口；默认可见（隐藏是显式操作）。
        true
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        if visible {
            self.window.makeKeyAndOrderFront(None);
        } else {
            self.window.orderOut(None);
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.window.isVisible()
    }

    pub(crate) fn set_always_on_top(&self, top: bool) {
        if top {
            self.window.setLevel(NSFloatingWindowLevel);
        } else {
            self.window.setLevel(NSNormalWindowLevel);
        }
    }

    pub(crate) fn is_always_on_top(&self) -> bool {
        // SAFETY: AppKit query on the main thread.
        // SAFETY: 主线程上的 AppKit 查询。
        self.window.level() == NSFloatingWindowLevel
    }

    pub(crate) fn minimize(&self) {
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        self.window.miniaturize(None);
    }

    pub(crate) fn maximize(&self) {
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        self.window.zoom(None);
    }

    pub(crate) fn restore(&self) {
        if self.window.isMiniaturized() {
            self.window.deminiaturize(None);
        } else if self.window.isZoomed() {
            self.window.zoom(None);
        }
    }

    pub(crate) fn raise(&self) {
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        self.window.makeKeyAndOrderFront(None);
    }

    pub(crate) fn is_minimized(&self) -> bool {
        // SAFETY: AppKit query on the main thread.
        // SAFETY: 主线程上的 AppKit 查询。
        self.window.isMiniaturized()
    }

    pub(crate) fn is_maximized(&self) -> bool {
        // SAFETY: AppKit query on the main thread.
        // SAFETY: 主线程上的 AppKit 查询。
        self.window.isZoomed()
    }

    pub(crate) fn has_input_focus(&self) -> bool {
        // SAFETY: AppKit query on the main thread.
        // SAFETY: 主线程上的 AppKit 查询。
        self.window.isKeyWindow()
    }

    pub(crate) fn is_occluded(&self) -> bool {
        false
    }

    pub(crate) fn set_keyboard_grabbed(&self, _grab: bool) {}

    pub(crate) fn set_position(&self, x: i32, y: i32) {
        // Physical top-left → points bottom-left: y flips around the
        // screen height.
        // 物理左上 → 点左下：y 绕屏幕高度翻转。
        let scale = self.window.backingScaleFactor();
        let screen_h = self
            .window
            .screen()
            .map(|s| s.frame().size.height)
            .unwrap_or(1080.0);
        let point = NSPoint::new(
            x as f64 / scale,
            screen_h - (y as f64 / scale) - self.window.frame().size.height,
        );
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        self.window.setFrameTopLeftPoint(point);
    }

    pub(crate) fn position(&self) -> (i32, i32) {
        let scale = self.window.backingScaleFactor();
        let screen_h = self
            .window
            .screen()
            .map(|s| s.frame().size.height)
            .unwrap_or(1080.0);
        let frame = self.window.frame();
        let top = screen_h - (frame.origin.y + frame.size.height);
        (
            (frame.origin.x * scale).round() as i32,
            (top * scale).round() as i32,
        )
    }

    pub(crate) fn center_on_screen(&self) {
        // SAFETY: AppKit call on the main thread.
        // SAFETY: 主线程上的 AppKit 调用。
        self.window.center();
    }

    pub(crate) fn title(&self) -> String {
        self.window.title().to_string()
    }

    pub(crate) fn set_opacity(&self, opacity: f32) {
        self.window
            .setAlphaValue(f64::from(opacity.clamp(0.0, 1.0)));
    }

    pub(crate) fn opacity(&self) -> f32 {
        self.window.alphaValue() as f32
    }

    pub(crate) fn dpi_scale(&self) -> f32 {
        self.window.backingScaleFactor() as f32
    }

    pub(crate) fn high_pixel_density(&self) -> bool {
        true
    }
}

// SAFETY: `Sync` exists only to satisfy renderer bounds (wgpu's
// `create_surface(&window)` requires shared references); they only read
// handle values. All methods remain bound to the main thread (see `Window`
// docs) — the same trade-off winit makes on macOS.
// SAFETY: `Sync` 仅为满足渲染后端的 bound（wgpu `create_surface(&window)`
// 要求共享引用）；它们只读取句柄值。所有方法仍限主线程（见 `Window` 文档）
// ——与 winit 在 macOS 上的取舍一致。
unsafe impl Sync for WindowInner {}

impl Drop for WindowInner {
    fn drop(&mut self) {
        // The user dropped the Window deliberately: close and destroy the
        // NSWindow (the delegate is released alongside).
        // 用户主动 drop 了 Window：关闭并销毁 NSWindow（委托随之释放）。
        self.window.close();
    }
}

impl WindowInner {
    /// Applies `f` to a mutable copy of the current style mask and writes
    /// it back (AppKit has no in-place style editor).
    /// 对当前样式掩码的可变副本应用 `f` 后写回（AppKit 无原地样式编辑）。
    fn with_style(&self, f: impl FnOnce(&mut NSWindowStyleMask)) {
        let mut style = self.window.styleMask();
        f(&mut style);
        self.window.setStyleMask(style);
    }
}

/// Non-blocking event pump: `distantPast` expiration means "whatever is
/// already queued, nothing more".
/// 非阻塞事件泵：过期时间为 `distantPast`——"只取已入队的，绝不等待"。
unsafe fn app_next_event(_mtm: MainThreadMarker, app: &NSApplication) -> Option<Retained<NSEvent>> {
    // SAFETY: main thread (contract); constants are process-global.
    // SAFETY: 主线程（契约）；常量为进程级全局。
    app.nextEventMatchingMask_untilDate_inMode_dequeue(
        objc2_app_kit::NSEventMask::Any,
        Some(&objc2_foundation::NSDate::distantPast()),
        unsafe { objc2_foundation::NSDefaultRunLoopMode },
        true,
    )
}

/// Translates an `NSEvent` into our events (input events only; window
/// events arrive via the delegate during `sendEvent`).
/// 将 `NSEvent` 翻译为本库事件（仅输入事件；窗口级事件在 `sendEvent`
/// 期间经委托到达）。
unsafe fn translate_nsevent(mtm: MainThreadMarker, event: &NSEvent) {
    let Some(window) = event.window(mtm) else {
        return; // Events without a window (app-level) are not ours / 无窗口事件非本库职责
    };
    let scale = window.backingScaleFactor();
    let content_h = window
        .contentView()
        .expect("content view")
        .frame()
        .size
        .height;
    let location = event.locationInWindow();
    // Window points (bottom-left origin) → client physical pixels
    // (top-left origin).
    // 窗口点（左下原点）→ 客户区物理像素（左上原点）。
    let (x, y) = (location.x * scale, (content_h - location.y) * scale);
    let modifiers = modifiers_from_flags(event.modifierFlags());

    match event.r#type() {
        NSEventType::MouseMoved
        | NSEventType::LeftMouseDragged
        | NSEventType::RightMouseDragged
        | NSEventType::OtherMouseDragged => {
            push_simple(Event::MouseMoved { x, y });
        }
        NSEventType::LeftMouseDown | NSEventType::RightMouseDown | NSEventType::OtherMouseDown => {
            push_simple(Event::MouseDown {
                button: button_from_index(event.buttonNumber() as i64),
            });
        }
        NSEventType::LeftMouseUp | NSEventType::RightMouseUp | NSEventType::OtherMouseUp => {
            push_simple(Event::MouseUp {
                button: button_from_index(event.buttonNumber() as i64),
            });
        }
        NSEventType::ScrollWheel => {
            // Natural scrolling delivers positive-down; positive-up here.
            // 自然滚动向下为正；本约定取正值向上。
            push_simple(Event::MouseWheel {
                delta_x: -event.deltaX(),
                delta_y: -event.deltaY(),
            });
        }
        NSEventType::KeyDown | NSEventType::KeyUp => {
            let down = event.r#type() == NSEventType::KeyDown;
            let key = keycode_to_keycode(event.keyCode());
            if down {
                // No-IME passthrough: the key's plain characters become
                // TextInput (control characters skipped).
                // 无 IME 直通：按键的纯字符作为 TextInput（跳过控制字符）。
                if let Some(text) = event.characters() {
                    let text = text.to_string();
                    let mut chars = text.chars();
                    if let (Some(c), None) = (chars.next(), chars.next())
                        && !c.is_control()
                    {
                        push_simple(Event::TextInput {
                            text: c.to_string(),
                        });
                    }
                }
            }
            if down {
                push_simple(Event::KeyDown {
                    key,
                    modifiers,
                    repeat: event.isARepeat(),
                });
            } else {
                push_simple(Event::KeyUp { key, modifiers });
            }
        }
        // FlagsChanged (modifier-only press/release): modifiers are carried
        // by the next key event; v1 ignores the bare toggle.
        // FlagsChanged（仅修饰键按下/抬起）：修饰键由下一个键事件携带；
        // v1 忽略裸切换。
        _ => {}
    }
}

/// Helper: push an event into the global buffer.
/// 辅助：写入全局事件缓冲。
fn push_simple(event: Event) {
    PENDING
        .lock()
        .expect("PENDING poisoned / PENDING 锁中毒")
        .push_back(event);
}
