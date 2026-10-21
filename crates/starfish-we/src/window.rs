//! The public window type: a platform-independent shell that delegates to
//! the current platform's `imp`.
//! 公共窗口类型：平台无关的壳，具体能力委托给当前平台的 `imp`。

use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::WindowError;
use crate::handle::RawWindowHandle;
use crate::platform::imp;

use std::any::Any;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use crate::event::{Event, UserEvent};

/// The per-window user-event inbox (channel receiver + arrived events).
/// 每窗口用户事件收件箱（通道接收端 + 已到达事件）。
struct Inbox {
    rx: Receiver<UserEvent>,
    pending: VecDeque<UserEvent>,
}

/// The only window in the process.
/// 进程内唯一的窗口。
///
/// - Creation: [`Window::new`]; dropping destroys the platform window.
///   创建：[`Window::new`]；drop 时销毁平台窗口。
/// - Events: pull one at a time with [`poll_event`](Window::poll_event)
///   (non-blocking) inside your loop.
///   事件：循环内调用 [`poll_event`](Window::poll_event) 逐个拉取（非阻塞）。
/// - Invariant: at most one `Window` per process; a second `new` returns
///   [`WindowError::AlreadyExists`].
///   约束：同一时刻最多存在一个 `Window`，第二次 `new` 返回
///   [`WindowError::AlreadyExists`]。
/// - Thread affinity: the window is bound to its creating thread (Win32
///   message queue / AppKit main-thread requirement). All methods
///   (including `poll_event`) must be called on that thread; cross-thread
///   event injection is the single exception, via [`EventProxy`].
///   线程亲和：窗口与其创建线程绑定（Win32 消息队列 / AppKit 主线程要求），
///   所有方法（含 `poll_event`）必须在创建线程上调用；
///   跨线程注入事件是唯一例外，走 [`EventProxy`]。
pub struct Window {
    inner: imp::WindowInner,
    /// Sender side of the user-event channel (shared by `push_event` and
    /// `EventProxy`).
    /// 用户自定义事件通道的发送端（`push_event` / `EventProxy` 共享）。
    user_tx: Sender<UserEvent>,
    /// The user-event inbox: the channel receiver plus events that have
    /// arrived and await dispatch. The `Mutex` exists solely to keep
    /// `Window: Sync` (required by wgpu's `create_surface`); the lock is
    /// uncontended in practice — consumption happens on one thread.
    /// Interior mutability keeps every `Window` method `&self` — required
    /// so a wgpu surface can hold `&Window` while the loop keeps polling.
    /// 用户自定义事件的收件箱：通道接收端 + 已到达待派发的事件。
    /// 套 `Mutex` 只为保住 `Window: Sync`（wgpu `create_surface` 的 bound）；
    /// 锁实际无竞争——消费仅在创建线程。内部可变性让所有 `Window` 方法
    /// 保持 `&self`——这是 wgpu surface 持有 `&Window` 期间循环仍能 poll
    /// 的前提。
    user_inbox: Mutex<Inbox>,
}

/// Global slot for the single-window invariant; `true` while a `Window` is
/// alive.
/// 单窗口不变量的全局占位；`Window` 构造成功期间为 `true`。
static WINDOW_ALIVE: AtomicBool = AtomicBool::new(false);

/// Resets the single-window slot (Android process-reuse scenario only).
/// 重置单窗口占位（仅 Android 进程复用场景）。
///
/// Android caches processes: old and new Activity instances may briefly
/// overlap within one process. The window lifetime is counted per "run",
/// not per "process"; `android_init` releases the previous instance's slot
/// when wiring up a new one.
/// Android 会缓存进程，新旧 Activity 实例可能在同一进程内短暂交叠；
/// 窗口生命周期按"运行"而非"进程"计算，新实例装配时由
/// `android_init` 调用此函数释放上一实例的占位。
#[cfg(target_os = "android")]
pub(crate) fn reset_single_window_slot() {
    WINDOW_ALIVE.store(false, Ordering::SeqCst);
}

impl Window {
    /// Creates a window with the given title, client size (physical
    /// pixels) and creation `flags` (SDL-style shorthand).
    /// 创建一个标题为 `title`、客户区初始尺寸为 `size`（物理像素）、
    /// 带创建标志 `flags` 的窗口（SDL 风格快捷构造）。
    ///
    /// Equivalent to [`WindowBuilder`] with `title` + `size` + `flags` and
    /// defaults for everything else; use [`Window::builder`] for more
    /// options (borderless, size ranges, always-on-top, …).
    /// 等价于 [`WindowBuilder`] 的 `title` + `size` + `flags` + 其余默认值；
    /// 需要更多选项（无边框、尺寸范围、置顶等）用 [`Window::builder`]。
    pub fn new(
        title: &str,
        size: (u32, u32),
        flags: crate::flags::WindowFlags,
    ) -> Result<Self, WindowError> {
        Self::builder().title(title).size(size).flags(flags).build()
    }

    /// Returns a window builder (the typed-Rust form of SDL flags; see
    /// [`WindowBuilder`]).
    /// 返回窗口构造器（SDL flag 的 Rust 形态：类型化方法，见 [`WindowBuilder`]）。
    pub fn builder() -> WindowBuilder {
        WindowBuilder::new()
    }

    fn build(mut options: WindowOptions) -> Result<Self, WindowError> {
        if WINDOW_ALIVE.swap(true, Ordering::SeqCst) {
            return Err(WindowError::AlreadyExists);
        }
        // flags → options translation (flags only add capabilities;
        // HIDDEN/FULLSCREEN etc. are consumed by the backend).
        // flags → options 翻译(flag 只增能力;HIDDEN/FULLSCREEN 等由后端消费)
        use crate::flags::WindowFlags as F;
        if options.flags.contains(F::RESIZABLE) {
            options.resizable = true;
        }
        if options.flags.contains(F::NOFRAME) {
            options.borderless = true;
        }
        if options.flags.contains(F::ALWAYS_ON_TOP) {
            options.always_on_top = true;
        }
        if options.flags.contains(F::HIDDEN) {
            options.hidden = true;
        }
        match imp::WindowInner::new(&options) {
            Ok(inner) => {
                let (user_tx, user_rx) = channel();
                Ok(Self {
                    inner,
                    user_tx,
                    user_inbox: Mutex::new(Inbox {
                        rx: user_rx,
                        pending: VecDeque::new(),
                    }),
                })
            }
            Err(err) => {
                WINDOW_ALIVE.store(false, Ordering::SeqCst);
                Err(err)
            }
        }
    }

    /// Pulls one pending event; `None` when the queue is empty
    /// (non-blocking, SDL style).
    /// 拉取一个待处理事件；队列为空时返回 `None`（非阻塞，SDL 风格）。
    ///
    /// This also drains the platform message source internally (Win32
    /// `PeekMessageW`, macOS `nextEventMatchingMask`, …) — callers never
    /// need to (and should not) touch the platform loop themselves.
    /// 内部会顺带把平台消息源泵干（Win32 `PeekMessageW`、macOS
    /// `nextEventMatchingMask` 等），调用者不需要、也不应自己碰平台循环。
    ///
    /// User events that have already arrived ([`push_event`](Window::push_event)
    /// / [`EventProxy`]) are dispatched before this round's platform
    /// events: a `send` is always visible on the next `poll_event`. No
    /// global FIFO ordering versus platform events is promised; batched
    /// injections dispatch in order (platform events are delayed briefly,
    /// never lost).
    /// 已到达的用户自定义事件（[`push_event`](Window::push_event) /
    /// [`EventProxy`] 注入）先于本轮平台事件派发：`send` 之后下一次
    /// `poll_event` 必定可见；与平台事件的相对顺序不作全局 FIFO 承诺，
    /// 批量注入时按序逐条派发（期间平台事件短暂延迟、不丢失）。
    pub fn poll_event(&self) -> Option<Event> {
        let mut inbox = self.user_inbox.lock().expect("user-event lock poisoned");
        while let Ok(user) = inbox.rx.try_recv() {
            inbox.pending.push_back(user);
        }
        if let Some(user) = inbox.pending.pop_front() {
            return Some(Event::User(user));
        }
        self.inner.poll_event()
    }

    /// Injects a custom event (same-thread convenience; use
    /// [`event_proxy`](Window::event_proxy) for cross-thread). Dispatched
    /// as `Event::User`.
    /// 注入一条自定义事件（同线程便捷版；跨线程用
    /// [`event_proxy`](Window::event_proxy)）。经 `Event::User` 派发。
    pub fn push_event(&self, code: u32, payload: impl Any + Send + Sync) {
        let _ = self.user_tx.send(UserEvent::new(code, payload));
    }

    /// Returns a cross-thread event proxy: clone it and hand it to
    /// interpreter worker threads / arbitrary tasks to inject `Event::User`
    /// from any thread.
    /// 取一个跨线程事件代理：可 `Clone` 后交给解释器工作线程 / 任意任务，
    /// 在任意线程向主循环注入 [`Event::User`]。
    ///
    /// After the `Window` is destroyed, `send` returns
    /// `Err(EventProxyClosed)`.
    /// `Window` 销毁后 `send` 返回 `Err(EventProxyClosed)`。
    pub fn event_proxy(&self) -> EventProxy {
        EventProxy {
            tx: self.user_tx.clone(),
        }
    }

    /// The current client-area size (physical pixels).
    /// 当前客户区尺寸（物理像素）。
    pub fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    /// Sets the window title.
    /// 设置窗口标题。
    pub fn set_title(&self, title: &str) {
        self.inner.set_title(title);
    }

    /// Returns the raw platform handle for unsafe-bridging rendering
    /// backends (wgpu / softbuffer / native APIs).
    /// 取裸平台句柄，用于 unsafe 桥接渲染后端（wgpu / softbuffer / 原生 API）。
    ///
    /// One step further: with the `wgpu` feature enabled, `Window`
    /// implements the raw-window-handle traits and can be handed to wgpu
    /// with zero unsafe.
    /// 更进一步：启用 `wgpu` feature 后，`Window` 直接实现了
    /// raw-window-handle 的 trait，可零 unsafe 交给 wgpu。
    pub fn raw_handle(&self) -> RawWindowHandle {
        self.inner.raw_handle()
    }

    // ── Runtime window capabilities (SDL flag semantics; forwarded to the
    //    platform backend) / 运行期窗口能力（SDL flag 语义；转发给平台后端）

    /// Sets whether the user can resize the window.
    /// 设置是否可由用户调整大小。
    pub fn set_resizable(&self, resizable: bool) {
        self.inner.set_resizable(resizable)
    }

    /// Whether the user can resize the window.
    /// 是否可由用户调整大小。
    pub fn is_resizable(&self) -> bool {
        self.inner.is_resizable()
    }

    /// Sets the client-area size (physical pixels).
    /// 设置客户区尺寸（物理像素）。
    pub fn set_size(&self, size: (u32, u32)) {
        self.inner.set_size(size.0, size.1)
    }

    /// Enters / leaves borderless fullscreen (the pre-entry position and
    /// size are restored on exit).
    /// 进入 / 退出无边框全屏（退出时还原进入前位置与尺寸）。
    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.inner.set_fullscreen(fullscreen)
    }

    /// Whether the window is in fullscreen.
    /// 是否处于全屏。
    pub fn is_fullscreen(&self) -> bool {
        self.inner.is_fullscreen()
    }

    /// Sets / clears the borderless style.
    /// 设置 / 取消无边框。
    pub fn set_borderless(&self, borderless: bool) {
        self.inner.set_borderless(borderless)
    }

    /// Whether the window is borderless.
    /// 是否无边框。
    pub fn is_borderless(&self) -> bool {
        self.inner.is_borderless()
    }

    /// Grabs / releases the mouse (confined to the client area).
    /// 抓取 / 释放鼠标（限定在客户区内）。
    pub fn set_mouse_grabbed(&self, grabbed: bool) {
        self.inner.set_mouse_grabbed(grabbed)
    }

    /// Whether the mouse is grabbed.
    /// 鼠标是否被抓取。
    pub fn is_mouse_grabbed(&self) -> bool {
        self.inner.is_mouse_grabbed()
    }

    /// Relative mouse mode (approximate: grab + hidden cursor).
    /// 相对鼠标模式（近似实现：抓取 + 隐藏光标）。
    pub fn set_mouse_relative(&self, enable: bool) {
        self.inner.set_mouse_relative(enable)
    }

    /// Shows / hides the cursor.
    /// 显示 / 隐藏光标。
    pub fn set_cursor_visible(&self, visible: bool) {
        self.inner.set_cursor_visible(visible)
    }

    /// Whether the cursor is visible.
    /// 光标是否可见。
    pub fn is_cursor_visible(&self) -> bool {
        self.inner.is_cursor_visible()
    }

    /// Shows / hides the window.
    /// 显示 / 隐藏窗口。
    pub fn set_visible(&self, visible: bool) {
        self.inner.set_visible(visible)
    }

    /// Whether the window is visible.
    /// 窗口是否可见。
    pub fn is_visible(&self) -> bool {
        self.inner.is_visible()
    }

    /// Sets / clears always-on-top.
    /// 设置 / 取消窗口置顶。
    pub fn set_always_on_top(&self, top: bool) {
        self.inner.set_always_on_top(top)
    }

    /// Whether the window is always on top.
    /// 窗口是否置顶。
    pub fn is_always_on_top(&self) -> bool {
        self.inner.is_always_on_top()
    }

    /// Minimizes the window.
    /// 最小化窗口。
    pub fn minimize(&self) {
        self.inner.minimize()
    }

    /// Maximizes the window.
    /// 最大化窗口。
    pub fn maximize(&self) {
        self.inner.maximize()
    }

    /// Restores from minimized / maximized.
    /// 从最小化 / 最大化还原。
    pub fn restore(&self) {
        self.inner.restore()
    }

    /// Raises the window to the top of the Z order.
    /// 提升窗口到 Z 序顶部。
    pub fn raise(&self) {
        self.inner.raise()
    }

    /// Whether the window is minimized.
    /// 窗口是否最小化。
    pub fn is_minimized(&self) -> bool {
        self.inner.is_minimized()
    }

    /// Whether the window is maximized.
    /// 窗口是否最大化。
    pub fn is_maximized(&self) -> bool {
        self.inner.is_maximized()
    }

    /// Whether the window has input focus.
    /// 窗口是否持有输入焦点。
    pub fn has_input_focus(&self) -> bool {
        self.inner.has_input_focus()
    }

    /// Whether the window is fully occluded (no direct Win32 equivalent;
    /// always false).
    /// 窗口是否被完全遮挡（Win32 无直接等价，恒 false）。
    pub fn is_occluded(&self) -> bool {
        self.inner.is_occluded()
    }

    /// Grabs the keyboard (no window-level Win32 equivalent; always a
    /// no-op).
    /// 键盘抓取（Win32 无窗口级等价，恒 no-op）。
    pub fn set_keyboard_grabbed(&self, grab: bool) {
        self.inner.set_keyboard_grabbed(grab)
    }

    /// Sets the top-left corner of the window frame (screen coordinates,
    /// physical pixels).
    /// 设置窗口外框左上角位置（屏幕坐标，物理像素）。
    pub fn set_position(&self, position: (i32, i32)) {
        self.inner.set_position(position.0, position.1)
    }

    /// The top-left corner of the window frame (screen coordinates,
    /// physical pixels).
    /// 窗口外框左上角位置（屏幕坐标，物理像素）。
    pub fn position(&self) -> (i32, i32) {
        self.inner.position()
    }

    /// Centers the window on its monitor's work area.
    /// 居中于所在显示器工作区。
    pub fn center_on_screen(&self) {
        self.inner.center_on_screen()
    }

    /// The current title.
    /// 当前标题。
    pub fn title(&self) -> String {
        self.inner.title()
    }

    /// Sets the window opacity (0.0–1.0; 1.0 = fully opaque).
    /// 设置窗口不透明度（0.0~1.0；1.0 = 全不透明）。
    pub fn set_opacity(&self, opacity: f32) {
        self.inner.set_opacity(opacity)
    }

    /// The window opacity.
    /// 窗口不透明度。
    pub fn opacity(&self) -> f32 {
        self.inner.opacity()
    }

    /// The DPI scale factor (96 DPI = 1.0; Per-Monitor V2 aware).
    /// DPI 缩放比（96 DPI = 1.0；Per-Monitor V2 感知）。
    pub fn dpi_scale(&self) -> f32 {
        self.inner.dpi_scale()
    }

    /// High pixel density: the process declares Per-Monitor V2, so
    /// always true (same flag semantics as SDL).
    /// 高像素密度：进程声明 Per-Monitor V2，恒 true（SDL 同款 flag 语义）。
    pub fn high_pixel_density(&self) -> bool {
        self.inner.high_pixel_density()
    }

    /// (wasm builds only) The underlying `<canvas>` element.
    /// （仅 wasm 构建）底层 `<canvas>` 元素。
    ///
    /// Platform-specific API by explicit exception: it only exists on
    /// wasm32 builds; the desktop API is unaffected. Web rendering
    /// (including wgpu's Web backend) targets the canvas.
    /// 平台特有 API 的显式例外：只存在于 wasm32 构建中，桌面 API 不受影响。
    /// Web 渲染（含 wgpu 的 Web 后端）以 canvas 为目标。
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    pub fn canvas(&self) -> web_sys::HtmlCanvasElement {
        self.inner.canvas()
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        // `inner` destroys the platform window itself; only the global slot
        // is released here.
        // `inner` 自身负责销毁平台窗口，这里只释放全局占位。
        WINDOW_ALIVE.store(false, Ordering::SeqCst);
    }
}

/// A handle for injecting custom events across threads (the product of
/// [`Window::event_proxy`]).
/// 跨线程注入自定义事件的句柄（[`Window::event_proxy`] 的产物）。
///
/// `Clone` it and hand it to interpreter worker threads / arbitrary tasks;
/// after the `Window` is destroyed, [`send`](EventProxy::send) returns
/// `Err(EventProxyClosed)`. This is the window API's only cross-thread
/// entry point — `Window` methods themselves remain bound to the creating
/// thread.
/// `Clone` 后可交给解释器工作线程 / 任意任务；`Window` 销毁后
/// [`send`](EventProxy::send) 返回 `Err(EventProxyClosed)`。
/// 这是窗口 API 唯一的跨线程入口——`Window` 本身的方法仍限创建线程。
#[derive(Clone)]
pub struct EventProxy {
    tx: Sender<UserEvent>,
}

impl EventProxy {
    /// Injects a custom event into the main loop, dispatched as
    /// `Event::User`.
    /// 向主循环注入一条自定义事件，经 `Event::User` 派发。
    ///
    /// Callable from any thread; the event becomes visible on the next
    /// `poll_event`. The payload is fire-and-forget (dropped with `Err` if
    /// the channel is full / the window is destroyed).
    /// 任意线程可调用；事件在下一次 `poll_event` 可见。
    /// 载荷即发即弃（通道满 / 窗口已销毁时丢弃并返回 `Err`）。
    pub fn send(&self, code: u32, payload: impl Any + Send + Sync) -> Result<(), EventProxyClosed> {
        self.tx
            .send(UserEvent::new(code, payload))
            .map_err(|_| EventProxyClosed)
    }
}

impl std::fmt::Debug for EventProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventProxy").finish_non_exhaustive()
    }
}

/// The event proxy is defunct (the window was destroyed); the event was
/// not queued.
/// 事件代理已失效（窗口已销毁），事件未入队。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventProxyClosed;

impl std::fmt::Display for EventProxyClosed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "event proxy is defunct (window destroyed)")
    }
}

impl std::error::Error for EventProxyClosed {}

/// The resolved construction parameters handed to the platform backend
/// (collected by [`WindowBuilder`]). Adding an option = adding a field;
/// non-breaking for builder users.
/// 平台后端拿到的解析后构造参数（由 [`WindowBuilder`] 归集）。
/// 新增选项 = 新增字段；对 builder 用户非破坏性。
#[derive(Debug, Clone, Default)]
pub(crate) struct WindowOptions {
    pub(crate) title: String,
    /// Initial client size (physical pixels).
    /// 客户区初始尺寸（物理像素）。
    pub(crate) size: (u32, u32),
    pub(crate) resizable: bool,
    pub(crate) borderless: bool,
    pub(crate) min_size: Option<(u32, u32)>,
    pub(crate) max_size: Option<(u32, u32)>,
    pub(crate) always_on_top: bool,
    /// Per-pixel transparent window (creation-time; Windows =
    /// WS_EX_NOREDIRECTIONBITMAP + DWM direct composition — the swapchain
    /// alpha composites straight onto the desktop, desktop-pet form).
    /// 逐像素透明窗口（**创建期**；Windows 采用 WS_EX_NOREDIRECTIONBITMAP
    /// 加 DWM 直通合成——交换链 alpha 直接与桌面混合，桌宠/悬浮件形态。
    /// 运行期不可追溯）。
    pub(crate) transparent: bool,
    /// 分层桌宠窗（**创建期**；Windows = WS_EX_LAYERED +
    /// UpdateLayeredWindow 逐像素 alpha——帧经 update_layered 上屏，
    /// 交换链不参与显示。桌宠通道）。
    pub(crate) layered: bool,
    /// Creation flags (SDL semantics; backends translate them into
    /// platform behavior).
    /// 创建标志（SDL 语义；后端翻译为平台行为）。
    pub(crate) flags: crate::flags::WindowFlags,
    /// Visible-on-create = false (the HIDDEN flag or an explicit builder
    /// setting).
    /// 创建即可见 = false（HIDDEN flag 或 builder 显式设置）。
    pub(crate) hidden: bool,
}

/// The window builder. Adding a window option = adding a method;
/// non-breaking for existing callers.
/// 窗口构造器。新增窗口选项 = 新增方法，对既有调用者非破坏性。
///
/// ```ignore
/// let window = Window::builder()
///     .title("demo")
///     .size((800, 600))
///     .resizable(false)
///     .borderless(true)
///     .min_size((400, 300))
///     .build()?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct WindowBuilder {
    options: WindowOptions,
}

impl WindowBuilder {
    fn new() -> Self {
        Self::default()
    }

    /// The window title. Defaults to empty.
    /// 窗口标题。默认为空。
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.options.title = title.into();
        self
    }

    /// Initial client size (physical pixels). No default — always set it
    /// (the struct default is `(0, 0)`, and backends clamp that to `1`).
    /// 客户区初始尺寸（物理像素）。无默认值——务必设置（结构体默认为
    /// `(0, 0)`，后端会将其钳制为 `1`）。
    pub fn size(mut self, size: (u32, u32)) -> Self {
        self.options.size = size;
        self
    }

    /// Whether the user can resize the window (includes the maximize
    /// button). Defaults to `true`.
    /// 是否可由用户调整大小（含最大化按钮）。默认 `true`。
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.options.resizable = resizable;
        self
    }

    /// Borderless (no system title bar or frame). Defaults to `false`.
    /// 无边框（无系统标题栏与边框）。默认 `false`。
    pub fn borderless(mut self, borderless: bool) -> Self {
        self.options.borderless = borderless;
        self
    }

    /// Minimum client size (physical pixels), constraining user-driven
    /// resizes. Unbounded by default.
    /// 客户区最小尺寸（物理像素），约束用户拖拽调整。默认不限制。
    pub fn min_size(mut self, size: (u32, u32)) -> Self {
        self.options.min_size = Some(size);
        self
    }

    /// Maximum client size (physical pixels), constraining user-driven
    /// resizes. Unbounded by default.
    /// 客户区最大尺寸（物理像素），约束用户拖拽调整。默认不限制。
    pub fn max_size(mut self, size: (u32, u32)) -> Self {
        self.options.max_size = Some(size);
        self
    }

    /// Always on top. Defaults to `false`.
    /// 窗口置顶。默认 `false`。
    pub fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.options.always_on_top = always_on_top;
        self
    }

    /// Layered pet window (Windows: WS_EX_LAYERED + UpdateLayeredWindow
    /// per-pixel alpha — the desktop-pet channel; the frame is updated
    /// via [`Window::update_layered`], not the swapchain). Defaults to
    /// `false`.
    /// 分层桌宠窗（Windows：WS_EX_LAYERED + UpdateLayeredWindow 逐像素
    /// alpha 合成；帧经 [`Window::update_layered`] 上屏而非交换链）。
    /// 默认 `false`。
    pub fn layered(mut self, layered: bool) -> Self {
        self.options.layered = layered;
        self
    }

    /// Creation flags (SDL-style bit combination; when combined with the
    /// setters above, flags only fix the initial state).
    /// 创建标志（SDL 语义位组合；与各 setter 叠加时 flag 仅定初始状态）。
    pub fn flags(mut self, flags: crate::flags::WindowFlags) -> Self {
        self.options.flags = flags;
        // TRANSPARENT = 创建期逐像素透明（Windows = WS_EX_NOREDIRECTIONBITMAP；
        // 运行期不可追溯，故走独立 options 字段而非 post-create 位）
        self.options.transparent = flags.contains(crate::flags::WindowFlags::TRANSPARENT);
        // LAYERED = 分层桌宠窗（UpdateLayeredWindow 通道，创建期扩展样式）
        self.options.layered = flags.contains(crate::flags::WindowFlags::LAYERED);
        self
    }

    /// Creates the window. Subject to the single-window constraint (see
    /// [`Window::new`]).
    /// 创建窗口。受单窗口约束（见 [`Window::new`]）。
    pub fn build(self) -> Result<Window, WindowError> {
        Window::build(self.options)
    }
}
