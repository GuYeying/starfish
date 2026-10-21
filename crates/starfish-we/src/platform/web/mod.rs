//! Web (wasm32-unknown-unknown) backend.
//! Web (wasm32-unknown-unknown) 后端。
//!
//! The browser is callback-driven — the inverse of the poll model: DOM
//! event listeners **buffer events into a queue** and `poll_event` drains
//! it; the main loop's pacing is driven by the caller (interpreter tick /
//! `setTimeout`). A `requestAnimationFrame` loop only sets a redraw flag
//! (coalesced, never accumulated) so wgpu users poll exactly one
//! `RedrawRequested` per frame.
//! 浏览器是回调驱动的，与 poll 模型相反：DOM 事件监听器把事件
//! **缓冲进队列**，`poll_event` 排空；主循环节奏由调用者驱动
//! （解释器 tick / `setTimeout`）。`requestAnimationFrame` 循环只置
//! 一个重绘标志（合并，不堆积），使 wgpu 用户每帧轮询到一次
//! `RedrawRequested`。
//!
//! The window entity is a `<canvas>`: looked up by id `starfish-we`,
//! else the page's first canvas, otherwise one is created filling the
//! page. **Page is the size authority** (2026-10-18 回退定型): CSS =
//! bitmap = logical size, `devicePixelRatio` fully out of the engine
//! picture — window sizing belongs to the page (HTML/CSS/JS); the
//! engine only mirrors whatever layout the page gives the canvas
//! (ResizeObserver + window resize, dedup by CSS rect; a physical
//! `Resized` is reported on bitmap change). Browser zoom is a pure view
//! transform — zero engine action. Event coordinates: CSS client
//! coordinates − canvas origin (1:1). Keys follow the W3C
//! `KeyboardEvent.code`, same origin as [`KeyCode`] — a direct mapping.
//! Closing the page ends everything: the Web has no `CloseRequested`
//! semantics.
//! 窗口实体是 `<canvas>`：按 id `starfish-we` 查找，其次取页面
//! 第一个 canvas，都没有则自建并铺满页面。
//! **页面即尺寸权威**（2026-10-18 回退定型）：CSS = 位图 = 逻辑尺寸，
//! devicePixelRatio 完全不进引擎——窗口大小控制权 100% 归页面
//! （HTML/CSS/JS），引擎只镜像页面给 canvas 的布局（ResizeObserver
//! + window resize，按 CSS rect 去重；位图变化上报物理 `Resized`）。
//! 浏览器缩放是纯视图变换——引擎零动作。
//! 事件坐标：CSS 客户坐标 − canvas 原点（1:1）。
//! 键盘走 W3C `KeyboardEvent.code`，与 [`KeyCode`] 命名同源，直接映射。
//! 页面关闭即结束，Web 没有 `CloseRequested` 语义。

use std::cell::RefCell;
use std::rc::Rc;

use std::collections::VecDeque;

use wasm_bindgen::prelude::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Document, HtmlCanvasElement, KeyboardEvent, MouseEvent, WheelEvent};

use crate::error::WindowError;
use crate::event::{Event, KeyCode, Modifiers, MouseButton, TouchPhase};
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

thread_local! {
    /// Single window (wasm is single-threaded): the live window's state.
    /// 单窗口（wasm 单线程）：存活窗口的状态。
    static STATE: RefCell<Option<Rc<WebState>>> = const { RefCell::new(None) };
    /// The rAF redraw loop's self-reference handle; dropping it stops the
    /// scheduling.
    /// rAF 重绘循环的自引用句柄；drop 即停止调度。
    static FRAME_LOOP: RefCell<Option<Closure<dyn FnMut()>>> = const { RefCell::new(None) };
    /// The most recent `requestAnimationFrame` id (for cancellation on
    /// window drop — an already-queued frame must not fire into a released
    /// closure, or the browser throws "closure ... after being dropped").
    /// 最近一次 `requestAnimationFrame` 的 id（窗口 drop 时取消——已排队
    /// 的帧不得打进已释放的闭包，否则浏览器抛 "closure ... dropped"）。
    static RAF_HANDLE: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// A DOM event listener that **detaches itself on drop**: the browser
/// keeps the JS function reference alive independently, so releasing the
/// Rust-side `Closure` without `removeEventListener` leaves a live
/// listener pointing at freed state — every subsequent DOM event of that
/// type throws "closure invoked recursively or after being dropped".
/// DOM 事件监听器，**drop 时自动解除**：浏览器独立持有 JS 函数引用，
/// 只释放 Rust 侧 `Closure` 而不移除监听，会让存活的监听指向已释放的
/// 状态——此后该类型的每个 DOM 事件都抛
/// "closure invoked recursively or after being dropped"。
struct Listener {
    target: web_sys::EventTarget,
    ty: &'static str,
    closure: Closure<dyn FnMut(JsValue)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = self.target.remove_event_listener_with_callback(
            self.ty,
            self.closure.as_ref().unchecked_ref(),
        );
    }
}

/// The context shared by DOM closures (canvas + event channel).
/// Closures capture only this — never `WebState` — avoiding Rc cycles.
/// DOM 闭包共享的上下文（画布 + 事件通道）。
/// 闭包只捕获它，不捕获 `WebState`，避免 Rc 循环。
#[derive(Clone)]
struct Ctx {
    canvas: HtmlCanvasElement,
    shared: Rc<RefCell<Shared>>,
    /// 上次同步时的 CSS 布尔矩形（布局去重键——rect 变 = 页面布局
    /// 变化；纯 zoom（dpr 变、rect 不变）不动作：CSS/位图恒 = 逻辑
    /// 尺寸，zoom 是纯视图变换，渲染零扰动）
    last_css: Rc<std::cell::Cell<(f64, f64)>>,
}

struct WebState {
    canvas: HtmlCanvasElement,
    /// The event channel between DOM callbacks and `poll_event`.
    /// DOM 回调与 `poll_event` 之间的事件通道。
    shared: Rc<RefCell<Shared>>,
    /// Holds the listeners while alive; dropping detaches them from the
    /// DOM (see [`Listener`]).
    /// 存活期内持有监听器；drop 即从 DOM 解除（见 [`Listener`]）。
    _listeners: Vec<Listener>,
    /// Layout-level size watcher (`ResizeObserver`) — catches flex/grid
    /// reflow that never fires a window `resize`. Disconnected on drop
    /// (see [`ResizeWatch`]).
    /// 布局级尺寸观察者（`ResizeObserver`）——捕捉不触发 window
    /// `resize` 的 flex/grid 重排。drop 时 disconnect（见
    /// [`ResizeWatch`]）。
    _resize_observer: Option<ResizeWatch>,
}

impl Drop for WebState {
    fn drop(&mut self) {
        self._resize_observer = None;
    }
}

/// The `ResizeObserver` plus its kept-alive callback: the JS observer
/// holds the function reference, so the Rust-side `Closure` must outlive
/// it — dropping this struct disconnects first, then releases.
/// `ResizeObserver` 与其保活回调的打包：JS 观察者持有函数引用，Rust 侧
/// `Closure` 必须活得更久——drop 时先 disconnect 再释放。
struct ResizeWatch {
    _callback: Closure<dyn FnMut(js_sys::Array, web_sys::ResizeObserver)>,
    observer: web_sys::ResizeObserver,
}

impl Drop for ResizeWatch {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

struct Shared {
    /// Events translated from DOM callbacks, awaiting dispatch.
    /// DOM 回调翻译出的待发事件。
    pending: VecDeque<Event>,
    /// The redraw request set by rAF and consumed by poll (coalesced /
    /// deduplicated).
    /// rAF 置位、poll 消费的重绘请求（合并去重）。
    redraw: bool,
}

pub(crate) struct WindowInner {
    state: Rc<WebState>,
}

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        let document = Self::document()?;
        let canvas = Self::acquire_canvas(&document)?;
        canvas.set_tab_index(0);

        Self::sync_canvas_size(&canvas); // The return value doesn't matter here: the first sync always sizes the canvas
        // 返回值无关紧要：首同步必装配尺寸

        let mut state = Rc::new(WebState {
            canvas: canvas.clone(),
            shared: Rc::new(RefCell::new(Shared {
                pending: VecDeque::new(),
                redraw: false,
            })),
            _listeners: Vec::new(),
            _resize_observer: None,
        });

        // Closures capture only Ctx (canvas + channel), so listeners can be
        // attached before back-filling the state.
        // 闭包只捕获 Ctx（canvas + 通道），因此可以先挂监听再回填 state。
        let (listeners, observer) = Self::attach_listeners(&Ctx {
            canvas: canvas.clone(),
            shared: state.shared.clone(),
            last_css: std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0))),
        })?;
        if let Some(inner) = Rc::get_mut(&mut state) {
            inner._listeners = listeners;
            inner._resize_observer = observer;
        }

        if STATE.with_borrow_mut(|slot| slot.replace(state).is_some()) {
            // Window::build's global guard already blocks a second window;
            // this is just a safety net.
            // Window::build 的全局守卫已挡住第二窗口；此处仅兜底。
            return Err(WindowError::AlreadyExists);
        }
        Self::start_frame_loop(&document);
        let _ = options; // Title goes through document.title semantics; size is canvas-driven
        Ok(Self {
            // The slot.replace above consumed the Rc; take it back.
            // 上面 slot.replace 消耗了 Rc，这里重新取回。
            state: STATE.with_borrow(|slot| {
                slot.clone()
                    .expect("STATE was just written / STATE 刚刚写入")
            }),
        })
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        let mut shared = self.state.shared.borrow_mut();
        if let Some(event) = shared.pending.pop_front() {
            return Some(event);
        }
        if shared.redraw {
            shared.redraw = false;
            return Some(Event::RedrawRequested);
        }
        None
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        Self::measure(&self.state.canvas)
    }

    pub(crate) fn set_title(&self, title: &str) {
        if let Ok(document) = Self::document() {
            document.set_title(title);
        }
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        // The canvas element's JsValue object pointer (same source as
        // wgpu's Web-backend WebCanvas path).
        // canvas 元素的 JsValue 对象指针（wgpu Web 后端 WebCanvas 路径同源）
        let canvas: &HtmlCanvasElement = &self.state.canvas;
        let js: &JsValue = canvas;
        RawWindowHandle::Web {
            canvas: js as *const JsValue as *mut std::ffi::c_void,
        }
    }

    // ── Runtime window capabilities (the Web column of the platform
    //    matrix) / 运行期窗口能力（平台矩阵：Web 列）────────────────
    // What the browser cannot do = warn-once no-op; what it can maps
    // directly onto DOM/canvas. warn-once uses a thread_local set (once
    // per key, no per-frame spam).
    // 浏览器做不到的 = warn-once no-op；能做的直接映射 DOM/canvas。
    // warn-once 用 thread_local 集合（每 key 一次，不随帧刷屏）。

    fn warn_once(key: &'static str, msg: &str) {
        thread_local! {
            static WARNED: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
        }
        WARNED.with(|set| {
            let mut set = set.borrow_mut();
            if !set.contains(&key) {
                set.push(key);
                web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(msg));
            }
        });
    }

    /// Style-writing helper (`canvas.style`).
    /// 样式写入辅助（canvas.style）。
    fn set_canvas_style(&self, prop: &str, value: &str) {
        self.state.canvas.style().set_property(prop, value).ok();
    }

    pub(crate) fn set_resizable(&self, _resizable: bool) {
        // The browser window's size is managed by the user / page layout:
        // canvas offers no resize handle. Silent no-op (by design, not a
        // misuse — pygame set_mode(RESIZABLE) is a normal call).
        // 浏览器窗口尺寸由用户/页面管理：canvas 不提供 resize 把手。
        // 设计性 no-op（pygame set_mode(RESIZABLE) 是正常调用），静默。
    }

    pub(crate) fn is_resizable(&self) -> bool {
        false
    }

    /// Sets the client size (physical-pixel semantics, matching desktop).
    ///
    /// Writes both representations directly: the attribute = physical
    /// pixels, the inline style = physical/dpr CSS pixels. Deliberately
    /// does NOT go through [`Self::sync_canvas_size`] (which reads layout
    /// and multiplies by dpr — the physical parameter would be scaled a
    /// second time).
    /// 设置客户区尺寸（物理像素语义，与桌面约定一致）。
    ///
    /// 直接写两份：属性 = 物理像素，内联样式 = 物理/dpr 的 CSS 像素。
    /// 不走 [`Self::sync_canvas_size`]（那会读布局再乘 dpr，物理参数
    /// 被二次缩放）。
    pub(crate) fn set_size(&self, width: u32, height: u32) {
        // **Web 尺寸定约（2026-10-18 回退定型）**：CSS = 位图 = 逻辑
        // 尺寸（w×h），devicePixelRatio 完全不参与——窗口大小控制权
        // 100% 在页面（HTML/CSS/JS），浏览器缩放是纯视图变换（渲染
        // 零扰动），任意缩放档位打开/往返，canvas 的逻辑内容尺寸恒
        // = w×h。高 DPI 屏由浏览器上采样显示。
        let w = width.max(1);
        let h = height.max(1);
        let style = self.state.canvas.style();
        style.set_property("width", &format!("{w}px")).ok();
        style.set_property("height", &format!("{h}px")).ok();
        self.state.canvas.set_width(w);
        self.state.canvas.set_height(h);
        // wgpu Web 表面自动跟随 canvas 属性，而引擎侧 Screen（swapchain
        // + 深度纹理）停在旧尺寸——不补发 Resized，应用侧永不重建 → 深度
        // 与颜色附件脱节。Windows 侧 set_size 经 WM_SIZE→wndproc 自动推
        // Resized，web 缺这一环，此处补上（尺寸 = canvas 位图属性，
        // 与 swapchain 严格一致）。
        self.state
            .shared
            .borrow_mut()
            .pending
            .push_back(Event::Resized {
                width: w,
                height: h,
            });
    }

    /// Fullscreen is a v0 no-op (Element::requestFullscreen integration is
    /// a follow-up).
    /// 全屏 v0 no-op（Element::requestFullscreen 接入为后续项）。
    pub(crate) fn set_fullscreen(&self, _fullscreen: bool) {
        Self::warn_once(
            "fullscreen",
            "[starfish-we] web: FULLSCREEN not supported in v0 (requestFullscreen is a follow-up) / v0 暂不支持(后续接 requestFullscreen)",
        );
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        false
    }

    pub(crate) fn set_borderless(&self, _borderless: bool) {
        // Browser windows have no decoration concept: no-op.
        // 浏览器窗口无装饰概念：no-op
    }

    pub(crate) fn is_borderless(&self) -> bool {
        false
    }

    /// Mouse grab is a v0 no-op (Pointer Lock API integration is a
    /// follow-up).
    /// 鼠标抓取 v0 no-op（Pointer Lock API 接入为后续项）。
    pub(crate) fn set_mouse_grabbed(&self, grabbed: bool) {
        Self::warn_once(
            "mouse_grabbed",
            "[starfish-we] web: MOUSE_GRABBED not supported in v0 (Pointer Lock is a follow-up) / v0 暂不支持(后续接 Pointer Lock)",
        );
        let _ = grabbed;
    }

    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        false
    }

    pub(crate) fn set_mouse_relative(&self, _enable: bool) {
        Self::warn_once(
            "mouse_relative",
            "[starfish-we] web: relative mouse not supported in v0 / 相对鼠标 v0 暂不支持",
        );
    }

    pub(crate) fn set_cursor_visible(&self, visible: bool) {
        self.set_canvas_style("cursor", if visible { "default" } else { "none" });
    }

    pub(crate) fn is_cursor_visible(&self) -> bool {
        self.state
            .canvas
            .style()
            .get_property_value("cursor")
            .map(|v| v != "none")
            .unwrap_or(true)
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        self.set_canvas_style("visibility", if visible { "visible" } else { "hidden" });
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.state
            .canvas
            .style()
            .get_property_value("visibility")
            .map(|v| v != "hidden")
            .unwrap_or(true)
    }

    pub(crate) fn set_always_on_top(&self, _top: bool) {
        Self::warn_once(
            "always_on_top",
            "[starfish-we] web: ALWAYS_ON_TOP does not apply; ignored / 不适用,忽略",
        );
    }

    pub(crate) fn is_always_on_top(&self) -> bool {
        false
    }

    pub(crate) fn minimize(&self) {
        Self::warn_once(
            "minimize",
            "[starfish-we] web: minimize does not apply; ignored / 不适用,忽略",
        );
    }

    pub(crate) fn maximize(&self) {
        Self::warn_once(
            "maximize",
            "[starfish-we] web: maximize does not apply; ignored / 不适用,忽略",
        );
    }

    pub(crate) fn restore(&self) {}

    pub(crate) fn raise(&self) {
        Self::warn_once(
            "raise",
            "[starfish-we] web: raise does not apply; ignored / 不适用,忽略",
        );
    }

    pub(crate) fn is_minimized(&self) -> bool {
        // Page hidden ≈ minimized (visibilitychange semantics).
        // 页面隐藏 ≈ 最小化（visibilitychange 语义）
        web_sys::window()
            .and_then(|w| w.document())
            .map(|d| d.visibility_state() == web_sys::VisibilityState::Hidden)
            .unwrap_or(false)
    }

    pub(crate) fn is_maximized(&self) -> bool {
        false
    }

    pub(crate) fn has_input_focus(&self) -> bool {
        web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.has_focus().ok())
            .unwrap_or(false)
    }

    pub(crate) fn center_on_screen(&self) {
        // The browser manages window position: no-op.
        // 浏览器管理窗口位置：no-op
    }

    pub(crate) fn is_occluded(&self) -> bool {
        false
    }

    pub(crate) fn set_keyboard_grabbed(&self, _grab: bool) {}

    pub(crate) fn set_position(&self, _x: i32, _y: i32) {
        Self::warn_once(
            "position",
            "[starfish-we] web: window positioning does not apply; ignored / 窗口定位不适用,忽略",
        );
    }

    pub(crate) fn position(&self) -> (i32, i32) {
        (0, 0)
    }

    pub(crate) fn title(&self) -> String {
        Self::document().map(|d| d.title()).unwrap_or_default()
    }

    pub(crate) fn set_opacity(&self, opacity: f32) {
        let v = opacity.clamp(0.0, 1.0);
        self.set_canvas_style("opacity", &format!("{v}"));
    }

    pub(crate) fn opacity(&self) -> f32 {
        self.state
            .canvas
            .style()
            .get_property_value("opacity")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .unwrap_or(1.0)
    }

    /// The DPI scale factor (`devicePixelRatio`; the basis of the
    /// physical-pixel convention).
    /// DPI 缩放比（devicePixelRatio；物理像素约定的基准）。
    pub(crate) fn dpi_scale(&self) -> f32 {
        web_sys::window()
            .map(|w| w.device_pixel_ratio() as f32)
            .unwrap_or(1.0)
    }

    /// Bitmap is 1:1 with CSS pixels, no dpr supersampling — false by
    /// definition.
    /// 位图与 CSS 像素 1:1，无 dpr 超采样——恒 false。
    pub(crate) fn high_pixel_density(&self) -> bool {
        false
    }

    /// The underlying `<canvas>` (wasm builds only; used by the Web
    /// rendering path).
    /// 底层 `<canvas>`（仅 wasm 构建，供 Web 渲染路径使用）。
    pub(crate) fn canvas(&self) -> HtmlCanvasElement {
        self.state.canvas.clone()
    }
}

impl Drop for WindowInner {
    fn drop(&mut self) {
        // Cancel the already-queued frame FIRST (it would otherwise fire
        // into the closure being released below), then release closures.
        // The listeners detach via `Listener::drop` as the Rc hits zero.
        // 先取消已排队的帧（否则它会打进下面即将释放的闭包），再释放
        // 闭包；监听器随 Rc 归零经 `Listener::drop` 自动解除。
        if let Some(w) = web_sys::window() {
            let id = RAF_HANDLE.with(|h| h.replace(0));
            if id != 0 {
                let _ = w.cancel_animation_frame(id);
            }
        }
        STATE.with_borrow_mut(|slot| *slot = None);
        FRAME_LOOP.with_borrow_mut(|cb| *cb = None);
    }
}

impl WindowInner {
    fn document() -> Result<Document, WindowError> {
        web_sys::window().and_then(|w| w.document()).ok_or_else(|| {
            WindowError::OsError(
                "browser environment unavailable (no document) / 浏览器环境不可用".into(),
            )
        })
    }

    fn acquire_canvas(document: &Document) -> Result<HtmlCanvasElement, WindowError> {
        let found = document
            .get_element_by_id("starfish-we")
            .or_else(|| document.query_selector("canvas").ok().flatten());
        let canvas: HtmlCanvasElement = match found {
            Some(el) => el.dyn_into().map_err(|_| {
                WindowError::OsError(
                    "the element with id=starfish-we is not a <canvas> / id=starfish-we 的元素不是 <canvas>"
                        .into(),
                )
            })?,
            None => {
                let canvas: HtmlCanvasElement = document
                    .create_element("canvas")
                    .map_err(|_| WindowError::OsError("failed to create canvas / 创建 canvas 失败".into()))?
                    .dyn_into()
                    .map_err(|_| WindowError::OsError("the element is not a canvas / 元素不是 canvas".into()))?;
                canvas
                    .style()
                    .set_css_text("position:fixed;inset:0;width:100vw;height:100vh;display:block;");
                let body = document
                    .body()
                    .ok_or_else(|| WindowError::OsError("no body element / 无 body 元素".into()))?;
                body.append_child(&canvas)
                    .map_err(|_| WindowError::OsError("failed to mount canvas / 挂载 canvas 失败".into()))?;
                canvas
            }
        };
        Ok(canvas)
    }

    /// The window's physical size = the bitmap (width/height attributes)
    /// — bitmap pixels are physical pixels under the physical-parity
    /// convention.
    /// 窗口物理尺寸 = 位图（width/height 属性）——物理等大定约下位图
    /// 像素即物理像素。
    fn measure(canvas: &HtmlCanvasElement) -> (u32, u32) {
        (canvas.width().max(1), canvas.height().max(1))
    }

    /// Synchronizes the canvas resolution (width/height attributes) to
    /// **CSS size × dpr** — bitmap pixels are physical pixels (physical
    /// parity with desktop windows; 1 bitmap px = 1 device px).
    /// 把画布分辨率（width/height 属性）同步为 **CSS 尺寸 × dpr**——
    /// 位图像素即物理像素（与桌面窗口物理等大同清晰度）。
    ///
    /// Returns whether the attributes actually changed (for resize
    /// deduplication).
    /// 返回属性是否真的变化（供 resize 去重）。
    ///
    /// **The inline CSS size must also be pinned** (only when the inline
    /// style has no explicit width/height): without a CSS size, a canvas's
    /// layout size is derived from its width/height attributes — writing
    /// only the attributes creates a positive feedback loop: the next
    /// round reads the already-enlarged attribute as the client size,
    /// multiplies by dpr and writes it back, growing by ×dpr on every
    /// resize event. Measured in practice: DevTools docking / window
    /// zooming hit 22763×17073 within 14 rounds, blew past the max texture
    /// limit (8192) and permanently black-screened the whole
    /// depth/swapchain chain. Pinning the style decouples the two: the
    /// attribute only governs render density (physical pixels), the style
    /// only governs layout size (CSS pixels) — the loop is gone. CSS
    /// itself is never rewritten here: layout authority lives with the
    /// page (and engine `set_size`, which writes CSS = logical/dpr).
    /// **必须同时钉住内联 CSS 尺寸**（仅当内联样式未定宽高时）：canvas
    /// 无 CSS 尺寸时布局尺寸由 width/height 属性决定，只写属性不钉样式
    /// 会形成正反馈——每轮把放大后的属性当 client 尺寸再乘 dpr 写回，
    /// 实测 DevTools 停靠 / 窗口缩放连发 resize 时 14 轮即
    /// 22763×17073，爆掉 max texture limit（8192）后全链黑屏。钉住样式
    /// 后属性只管渲染密度（物理像素），样式只管布局尺寸（CSS 像素）。
    /// CSS 本身这里从不改写：布局权威在页面（及引擎 `set_size`——它写
    /// CSS = 逻辑/dpr）。
    fn sync_canvas_size(canvas: &HtmlCanvasElement) -> bool {
        let style = canvas.style();
        // An explicit inline width/height (e.g. the auto-created canvas's
        // 100vw/100vh) means layout and attributes are already decoupled —
        // don't overwrite (preserving the page's responsive layout
        // semantics).
        // 内联样式已定宽高（如自建画布的 100vw/100vh）= 布局与属性已解耦，
        // 不覆盖（保留页面的响应式布局语义）。
        let inline = |prop: &str| {
            style
                .get_property_value(prop)
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false)
        };
        let rect = canvas.get_bounding_client_rect();
        let css_w = rect.width().round().max(1.0);
        let css_h = rect.height().round().max(1.0);
        if !inline("width") {
            style.set_property("width", &format!("{css_w}px")).ok();
        }
        if !inline("height") {
            style.set_property("height", &format!("{css_h}px")).ok();
        }
        // 位图 = CSS 布局尺寸（1:1，dpr 不参与）
        let width = css_w as u32;
        let height = css_h as u32;
        let changed = canvas.width() != width || canvas.height() != height;
        canvas.set_width(width);
        canvas.set_height(height);
        changed
    }

    /// The rAF loop: sets the redraw flag once per frame. The
    /// self-reference handle lives in [`FRAME_LOOP`]; clearing it on window
    /// drop stops the loop (the closure does not hold itself — no leak).
    /// rAF 循环：每帧置一次重绘标志。自引用句柄放在 [`FRAME_LOOP`]，
    /// 窗口 drop 时清空即停摆（闭包不持有自身，无泄漏）。
    fn start_frame_loop(document: &Document) -> () {
        let _ = document;
        let Some(js_window) = web_sys::window() else {
            return;
        };
        let shared = STATE.with_borrow(|slot| slot.as_ref().expect("STATE 已写入").shared.clone());
        let closure = Closure::wrap(Box::new(move || {
            shared.borrow_mut().redraw = true;
            if let Some(w) = web_sys::window() {
                FRAME_LOOP.with_borrow(|cb| {
                    if let Some(cb) = cb.as_ref() {
                        // Record the handle so window drop can cancel the
                        // queued frame (Listener-style hygiene for rAF).
                        // 记录句柄供窗口 drop 取消已排队帧（rAF 版监听
                        // 卫生，同 Listener）。
                        if let Ok(id) = w.request_animation_frame(cb.as_ref().unchecked_ref()) {
                            RAF_HANDLE.with(|h| h.set(id));
                        }
                    }
                });
            }
        }) as Box<dyn FnMut()>);
        if let Ok(id) = js_window.request_animation_frame(closure.as_ref().unchecked_ref()) {
            RAF_HANDLE.with(|h| h.set(id));
        }
        FRAME_LOOP.with_borrow_mut(|slot| *slot = Some(closure));
    }

    // ---------------- Event listeners and translation (event feature
    //                  only) / 事件监听与翻译（仅 event feature）---------

    fn attach_listeners(ctx: &Ctx) -> Result<(Vec<Listener>, Option<ResizeWatch>), WindowError> {
        let js_window = web_sys::window().ok_or_else(|| {
            WindowError::OsError("browser environment unavailable / 浏览器环境不可用".into())
        })?;
        let document = Self::document()?;
        let canvas: &web_sys::EventTarget = &ctx.canvas;
        let mut listeners = Vec::new();

        // Keyboard (window level).
        // 键盘（window 级）。
        for (ty, down) in [("keydown", true), ("keyup", false)] {
            let ctx = ctx.clone();
            listeners.push(add_listener(js_window.as_ref(), ty, move |v| {
                on_key(&ctx, v, down)
            })?);
        }
        // Mouse (canvas level).
        // 鼠标（canvas 级）。
        for ty in ["mousedown", "mousemove", "mouseup"] {
            let ctx = ctx.clone();
            listeners.push(add_listener(canvas, ty, move |v| on_mouse(&ctx, v))?);
        }
        // Wheel.
        // 滚轮。
        {
            let ctx = ctx.clone();
            listeners.push(add_listener(canvas, "wheel", move |v| on_wheel(&ctx, v))?);
        }
        // Focus.
        // 焦点。
        for (ty, gained) in [("focus", true), ("blur", false)] {
            let ctx = ctx.clone();
            listeners.push(add_listener(canvas, ty, move |_| {
                ctx.shared.borrow_mut().pending.push_back(if gained {
                    Event::FocusGained
                } else {
                    Event::FocusLost
                });
            })?);
        }
        // Touch (with primary-finger mouse synthesis).
        // 触摸（含主手指合成鼠标）。
        for ty in ["touchstart", "touchmove", "touchend", "touchcancel"] {
            let ctx = ctx.clone();
            listeners.push(add_listener(canvas, ty, move |v| on_touch(&ctx, v, ty))?);
        }
        // Window resize: same dedup/sync/report body as the
        // ResizeObserver (kept for browsers/paths where layout changes
        // without an observer entry).
        // window resize：与 ResizeObserver 共用去重/同步/上报体（保留
        // 以覆盖观察者条目之外的布局变化路径）。
        {
            let ctx = ctx.clone();
            listeners.push(add_listener(js_window.as_ref(), "resize", move |_| {
                Self::report_layout_change(&ctx);
            })?);
        }
        // Page visibility → lifecycle.
        // 页面可见性 → 生命周期。
        {
            let ctx = ctx.clone();
            listeners.push(add_listener(
                document.as_ref(),
                "visibilitychange",
                move |_| {
                    let hidden = Self::document()
                        .is_ok_and(|d| d.visibility_state() == web_sys::VisibilityState::Hidden);
                    ctx.shared.borrow_mut().pending.push_back(if hidden {
                        Event::Suspended
                    } else {
                        Event::Resumed
                    });
                },
            )?);
        }

        // Layout-level size watcher: ResizeObserver on the canvas —
        // flex/grid reflow, DevTools docking, style changes from the page
        // all land here without a window `resize`. Same dedup/sync/report
        // body as the window-resize listener.
        // 布局级尺寸观察：canvas 上的 ResizeObserver——flex/grid 重排、
        // DevTools 停靠、页面改样式都会落到这里而不经 window `resize`。
        // 与 window-resize 监听共用去重/同步/上报体。
        let obs_ctx = ctx.clone();
        let obs_callback = Closure::wrap(
            Box::new(
                move |_entries: js_sys::Array, _observer: web_sys::ResizeObserver| {
                    Self::report_layout_change(&obs_ctx);
                },
            ) as Box<dyn FnMut(js_sys::Array, web_sys::ResizeObserver)>,
        );
        let observer = web_sys::ResizeObserver::new(obs_callback.as_ref().unchecked_ref())
            .map_err(|_| {
                WindowError::OsError(
                    "ResizeObserver creation failed / ResizeObserver 创建失败".into(),
                )
            })?;
        observer.observe(&ctx.canvas);
        let resize_watch = Some(ResizeWatch {
            _callback: obs_callback,
            observer,
        });

        Ok((listeners, resize_watch))
    }

    /// Shared body of the window-resize listener and the ResizeObserver:
    /// **only layout changes act**（CSS rect 变）——位图重同步为布局
    /// 尺寸（1:1）并上报物理 `Resized`。纯 zoom（dpr 变、rect 不变）
    /// 零动作：CSS/位图恒 = 逻辑尺寸，zoom 是纯视图变换。
    /// window-resize 监听与 ResizeObserver 的共用体：**只有布局变化
    /// 才动作**（CSS rect 变）——位图重同步为布局尺寸（1:1）并上报
    /// 物理 `Resized`。纯 zoom 零动作。
    fn report_layout_change(ctx: &Ctx) {
        let rect = ctx.canvas.get_bounding_client_rect();
        let css_key = (rect.width().round(), rect.height().round());
        if css_key == ctx.last_css.get() {
            return; // 布局没变（含纯 zoom）：渲染零扰动
        }
        ctx.last_css.set(css_key);
        if Self::sync_canvas_size(&ctx.canvas) {
            let (width, height) = Self::measure(&ctx.canvas);
            ctx.shared
                .borrow_mut()
                .pending
                .push_back(Event::Resized { width, height });
        }
    }
}

fn add_listener<F>(
    target: &web_sys::EventTarget,
    ty: &'static str,
    handler: F,
) -> Result<Listener, WindowError>
where
    F: FnMut(JsValue) + 'static,
{
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(JsValue)>);
    target
        .add_event_listener_with_callback(ty, closure.as_ref().unchecked_ref())
        .map_err(|_| {
            WindowError::OsError(format!(
                "addEventListener({ty}) failed / addEventListener({ty}) 失败"
            ))
        })?;
    Ok(Listener {
        target: target.clone(),
        ty,
        closure,
    })
}

/// Event coordinates: CSS client coordinates − canvas origin (bitmap is
/// 1:1 with CSS pixels, dpr not involved).
/// 事件坐标：CSS 客户坐标 − canvas 原点（位图与 CSS 像素 1:1，dpr
/// 不参与）。
fn canvas_coords(ctx: &Ctx, client_x: f64, client_y: f64) -> (f64, f64) {
    let rect = ctx.canvas.get_bounding_client_rect();
    (client_x - rect.left(), client_y - rect.top())
}

fn on_key(ctx: &Ctx, value: JsValue, down: bool) {
    let Some(ev) = value.dyn_ref::<KeyboardEvent>() else {
        return;
    };
    if ev.is_composing() {
        return; // No IME composition in the first version (agreement); final text only.
        // IME 组合过程首版不接（约定），只收最终文本。
    }
    let modifiers = Modifiers {
        shift: ev.shift_key(),
        ctrl: ev.ctrl_key(),
        alt: ev.alt_key(),
        meta: ev.meta_key(),
    };
    // Unrecognized codes carry no extra information (Other only takes
    // native numeric key codes).
    // 未识别的 code 不携带更多信息（Other 只收原生数字键码）。
    let key = KeyCode::from_w3c_code(&ev.code()).unwrap_or(KeyCode::Other(0));
    let mut shared = ctx.shared.borrow_mut();
    if down {
        // A single printable character → TextInput (W3C `key`; no IME
        // composition).
        // 可打印单字符 → TextInput（W3C `key`；无 IME 组合）。
        let text = ev.key();
        let mut chars = text.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            if !c.is_control() {
                shared.pending.push_back(Event::TextInput {
                    text: c.to_string(),
                });
            }
        }
        shared.pending.push_back(Event::KeyDown {
            key,
            modifiers,
            repeat: ev.repeat(),
        });
    } else {
        shared.pending.push_back(Event::KeyUp { key, modifiers });
    }
}

fn on_mouse(ctx: &Ctx, value: JsValue) {
    let Some(ev) = value.dyn_ref::<MouseEvent>() else {
        return;
    };
    let (x, y) = canvas_coords(ctx, ev.client_x() as f64, ev.client_y() as f64);
    let mut shared = ctx.shared.borrow_mut();
    match ev.type_().as_str() {
        "mousemove" => shared.pending.push_back(Event::MouseMoved { x, y }),
        "mousedown" | "mouseup" => {
            // W3C button: 0 left / 1 middle / 2 right / 3 back / 4 forward.
            // W3C button：0 左 / 1 中 / 2 右 / 3 后退 / 4 前进。
            let button = match ev.button() {
                0 => MouseButton::Left,
                1 => MouseButton::Middle,
                2 => MouseButton::Right,
                3 => MouseButton::Back,
                4 => MouseButton::Forward,
                n => MouseButton::Other(n as u8),
            };
            let event = if ev.type_() == "mousedown" {
                Event::MouseDown { button }
            } else {
                Event::MouseUp { button }
            };
            shared.pending.push_back(event);
        }
        _ => {}
    }
}

fn on_wheel(ctx: &Ctx, value: JsValue) {
    let Some(ev) = value.dyn_ref::<WheelEvent>() else {
        return;
    };
    // deltaMode: 0 pixels / 1 line / 2 pages — normalized to a pixel
    // scale.
    // deltaMode：0 像素 / 1 行 / 2 页，统一折算成像素量纲。
    let unit = match ev.delta_mode() {
        1 => 16.0,
        2 => web_sys::window()
            .and_then(|w| w.inner_height().ok())
            .and_then(|h| h.as_f64())
            .unwrap_or(800.0),
        _ => 1.0,
    };
    // The browser scrolls positive-down; this crate's convention is
    // positive-up — negate.
    // 浏览器滚动向下为正；本 crate 约定正值向上，取反。
    ctx.shared
        .borrow_mut()
        .pending
        .push_back(Event::MouseWheel {
            delta_x: -ev.delta_x() * unit,
            delta_y: -ev.delta_y() * unit,
        });
}

fn on_touch(ctx: &Ctx, value: JsValue, ty: &str) {
    let Some(ev) = value.dyn_ref::<web_sys::TouchEvent>() else {
        return;
    };
    let phase = match ty {
        "touchstart" => TouchPhase::Started,
        "touchmove" => TouchPhase::Moved,
        "touchend" => TouchPhase::Ended,
        _ => TouchPhase::Cancelled,
    };
    let touches = ev.changed_touches();
    let mut shared = ctx.shared.borrow_mut();
    for i in 0..touches.length() {
        let Some(t) = touches.get(i) else {
            continue;
        };
        let (x, y) = canvas_coords(ctx, t.client_x() as f64, t.client_y() as f64);
        let finger = t.identifier() as u64;
        shared.pending.push_back(Event::Touch {
            phase,
            finger,
            x,
            y,
        });
        // The primary finger (identifier 0) is synthesized into mouse
        // events (SDL convention).
        // 主手指（identifier 0）合成鼠标事件（SDL 惯例）。
        if t.identifier() == 0 {
            let mouse = match phase {
                TouchPhase::Started => Event::MouseDown {
                    button: MouseButton::Left,
                },
                TouchPhase::Moved => Event::MouseMoved { x, y },
                TouchPhase::Ended | TouchPhase::Cancelled => Event::MouseUp {
                    button: MouseButton::Left,
                },
            };
            shared.pending.push_back(mouse);
        }
    }
}
