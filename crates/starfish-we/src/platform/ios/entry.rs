//! iOS entry point: `ios_main!` host integration + CADisplayLink frame
//! driver.
//! iOS 入口：`ios_main!` 宿主集成 + CADisplayLink 帧驱动。
//!
//! iOS owns the main thread: the host's poll loop cannot run standalone.
//! The inversion here is three-stage:
//! iOS 掌管主线程：宿主的 poll 循环无法独立运行。这里的反转分三级：
//!
//! 1. `ios_main!(tick)` installs the host tick and hands the main thread
//!    to `UIApplicationMain` (never returns normally).
//!    `ios_main!(tick)` 安装宿主 tick 并把主线程交给 `UIApplicationMain`
//!    （正常情况下永不返回）。
//! 2. UIKit instantiates our [`AppDelegate`]; `didFinishLaunching`
//!    creates the `UIWindow` and installs a `CADisplayLink`.
//!    UIKit 实例化 [`AppDelegate`]；`didFinishLaunching` 创建 `UIWindow`
//!    并安装 `CADisplayLink`。
//! 3. Each frame, the display link calls the host tick with `&Window` —
//!    the host polls events and renders inside its own tick.
//!    每帧，display link 以 `&Window` 调用宿主 tick——宿主在自己的 tick
//!    内 poll 事件并渲染。
//!
//! `tick: fn(&Window) -> Option<i32>`: returning `Some(code)` terminates
//! the app with that status; `None` keeps it running.
//! `tick: fn(&Window) -> Option<i32>`：返回 `Some(code)` 以该状态码退出
//! 应用；`None` 继续运行。

use std::cell::RefCell;
use std::sync::Mutex;

use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol};
use objc2::{MainThreadOnly, define_class};
use objc2_foundation::{NSDictionary, NSString};
use objc2_quartz_core::CADisplayLink;
use objc2_ui_kit::{UIApplication, UIApplicationDelegate};

use crate::Window;

/// The host tick, stored for the frame driver.
/// 宿主 tick，存储后由帧驱动调用。
static HOST_TICK: Mutex<Option<fn(&Window) -> Option<i32>>> = Mutex::new(None);

thread_local! {
    /// The only window (main thread).
    /// 唯一窗口（主线程）。
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    /// The display link handle (kept alive across frames).
    /// display link 句柄（跨帧保活）。
    static LINK: RefCell<Option<Retained<CADisplayLink>>> = const { RefCell::new(None) };
}

unsafe extern "C" {
    fn UIApplicationMain(
        argc: std::ffi::c_int,
        argv: *mut *const NSString,
        principal_class_name: Option<&NSString>,
        delegate_class_name: Option<&NSString>,
    ) -> std::ffi::c_int;
}

/// iOS host entry: store the tick, then hand the main thread to
/// `UIApplicationMain` (never returns).
/// iOS 宿主入口：安装 tick 后把主线程交给 `UIApplicationMain`
/// （正常情况下永不返回）。
pub fn ios_start(tick: fn(&Window) -> Option<i32>) -> ! {
    *HOST_TICK
        .lock()
        .expect("HOST_TICK poisoned / HOST_TICK 锁中毒") = Some(tick);
    // Force-register the delegate class with the ObjC runtime: objc2
    // registers lazily on first `class()` access, but `UIApplicationMain`
    // looks the class up by NAME — before any Rust code touches the type
    // the lookup fails and the app idles with no delegate at all.
    // 强制向 ObjC 运行时注册委托类：objc2 仅在首次访问 `class()` 时惰性
    // 注册，而 `UIApplicationMain` 按**名字**查找——若没有 Rust 代码先
    // 触碰该类型，查找会失败，应用将以无委托状态空转。
    let _ = <AppDelegate as objc2::ClassType>::class();
    let registered = objc2::runtime::AnyClass::get(c"StarfishAppDelegate").is_some();
    eprintln!("[starfish] delegate class registered: {registered} / 委托类已注册: {registered}");
    // SAFETY: argc=0 / argv=null is accepted by UIApplicationMain; the
    // delegate class name refers to the class registered just above.
    // SAFETY: UIApplicationMain 接受 argc=0、argv=null；委托类名指向
    // 上方刚注册的类。
    let status = unsafe {
        UIApplicationMain(
            0,
            std::ptr::null_mut(),
            None,
            Some(&NSString::from_str("StarfishAppDelegate")),
        )
    };
    // UIApplicationMain should never return; if it does, exit with its
    // status.
    // UIApplicationMain 不应返回；若返回则以其状态码退出。
    std::process::exit(status)
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    // The runtime name must match the string handed to `UIApplicationMain`:
    // objc2's auto-generated default (`module_path::Name` + crate version)
    // is never findable via `NSClassFromString`, so the delegate would
    // silently never be instantiated (the app idles with no
    // didFinishLaunching at all).
    // 运行时名必须与传给 `UIApplicationMain` 的字符串一致：objc2 自动
    // 生成的默认名（`模块路径::类名` + crate 版本）经 `NSClassFromString`
    // 永远查不到，委托会静默地不被实例化（应用空转、根本没有
    // didFinishLaunching）。
    #[name = "StarfishAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl UIApplicationDelegate for AppDelegate {
        // didFinishLaunching: create the window and install the frame
        // driver — the only reliable bootstrap moment on iOS.
        // didFinishLaunching：建窗并安装帧驱动——iOS 上唯一可靠的
        // 装配时机。
        #[unsafe(method(application:didFinishLaunchingWithOptions:))]
        unsafe fn application_did_finish_launching(
            &self,
            _application: &UIApplication,
            _options: Option<&NSDictionary>,
        ) -> Bool {
            eprintln!("[starfish] didFinishLaunching: creating window / 建窗中");
            match Window::new("starfish ios", (0, 0), crate::flags::WindowFlags::default()) {
                Ok(window) => {
                    WINDOW.with_borrow_mut(|slot| *slot = Some(window));
                    install_frame_driver(self);
                    eprintln!(
                        "[starfish] window created + frame driver installed / 建窗完成 + 帧驱动已装"
                    );
                    Bool::YES
                }
                Err(err) => {
                    eprintln!("window creation failed / 建窗失败: {err}");
                    Bool::NO
                }
            }
        }
    }

    impl AppDelegate {
        // CADisplayLink frame callback: drives the host tick each frame.
        // CADisplayLink 帧回调：每帧驱动一次宿主 tick。
        #[unsafe(method(on_frame:))]
        unsafe fn on_frame(&self, _link: &CADisplayLink) {
            let code = WINDOW.with_borrow(|slot| {
                let window = slot.as_ref().expect("window ready / 窗口未就绪");
                (HOST_TICK
                    .lock()
                    .expect("HOST_TICK poisoned / HOST_TICK 锁中毒"))
                .expect("tick not installed / tick 未安装")(window)
            });
            if let Some(code) = code {
                std::process::exit(code);
            }
        }
    }
);

/// Installs a CADisplayLink targeting the delegate's `on_frame:`; the
/// handle lives in the thread-local [`LINK`], cleared only on exit.
/// 安装以 `on_frame:` 为目标的 CADisplayLink；句柄存入线程局部槽，
/// 退出时清空。
fn install_frame_driver(delegate: &AppDelegate) {
    let link: Retained<CADisplayLink> =
        unsafe { CADisplayLink::displayLinkWithTarget_selector(delegate, objc2::sel!(on_frame:)) };
    // SAFETY: main thread; runloop mode constants are process-global.
    // SAFETY: 主线程；runloop 模式常量为进程级全局。
    unsafe {
        link.addToRunLoop_forMode(
            &objc2_foundation::NSRunLoop::mainRunLoop(),
            objc2_foundation::NSDefaultRunLoopMode,
        );
    }
    LINK.with_borrow_mut(|slot| *slot = Some(link));
}
