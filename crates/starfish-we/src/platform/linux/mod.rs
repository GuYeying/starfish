//! Linux backend — **Wayland-only** (X11 dropped by design decision).
//! Linux 后端——**仅 Wayland**（设计决策放弃 X11）。
//!
//! Built on `smithay-client-toolkit` 0.21 + `wayland-client` 0.31 (system
//! + dlopen backend: real `wl_display*`/`wl_surface*` pointers for wgpu,
//! no link-time libwayland). Same inversion as the other OS-owned-entry
//! platforms: Wayland callbacks translate into a buffer queue that
//! `poll_event` drains non-blockingly.
//! 基于 `smithay-client-toolkit` 0.21 + `wayland-client` 0.31（system +
//! dlopen 后端：为 wgpu 提供真实 `wl_display*`/`wl_surface*` 指针，免链接期
//! libwayland 依赖）。与"系统掌管入口"类平台相同的反转：Wayland 回调把
//! 事件翻译进缓冲队列，`poll_event` 非阻塞排空。
//!
//! Event pump (all steps non-blocking; init-time `roundtrip` is the only
//! blocking point, mirroring Windows pumping messages in `new`):
//! 事件泵（各步均非阻塞；仅 init 的 `roundtrip` 阻塞——与 Windows 在
//! `new` 里泵消息同级）：
//! `flush` → `dispatch_pending` → `prepare_read`+`read`（WouldBlock =
//! idle）→ `dispatch_pending` → repeat synthesis → pop.
//!
//! Keyboard: sctk hands us RAW evdev codes
//! (`translate::evdev_to_keycode`) and xkb-composed UTF-8; compositors
//! don't repeat keys, so repeats are synthesized client-side from
//! `RepeatInfo`.
//! 键盘：sctk 交给我们的裸 evdev 码（见 `translate::evdev_to_keycode`）
//! 与 xkb 合成 UTF-8；合成器不重复按键，重复由客户端按 `RepeatInfo` 合成。
//!
//! Coordinates: configure sizes are LOGICAL; physical = logical ×
//! integer scale. All public sizes/coords are physical.
//! 坐标：configure 尺寸是逻辑像素；物理 = 逻辑 × 整数 scale。公共
//! 尺寸/坐标一律物理像素。
//!
//! sctk 0.21 wiring (the 0.21 breaking change: all per-module
//! `delegate_*!` macros were replaced by one `delegate_dispatch2!`):
//! sctk 0.21 装配（0.21 破坏性变更：各模块 `delegate_*!` 宏合并为单一的
//! `delegate_dispatch2!`）：
//! `delegate_registry!` + `impl ProvidesRegistryState` +
//! `delegate_dispatch2!` — see the bottom of this file.

use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use sctk_adwaita::{AdwaitaFrame, FrameConfig};
use smithay_client_toolkit::reexports::csd_frame::{
    CursorIcon, DecorationsFrame, FrameAction, FrameClick, ResizeEdge as CsdResizeEdge,
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_toplevel::ResizeEdge as XdgResizeEdge;
use smithay_client_toolkit::subcompositor::SubcompositorState;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::csd_frame::WindowState,
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{
            KeyEvent, KeyboardHandler, Keysym, Modifiers as SctkModifiers, RawModifiers, RepeatInfo,
        },
        pointer::{
            PointerData, PointerEvent, PointerEventKind, PointerHandler, ThemeSpec, ThemedPointer,
        },
    },
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell, XdgSurface,
            window::{
                DecorationMode, Window as SctkWindow, WindowConfigure, WindowDecorations,
                WindowHandler,
            },
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use wayland_client::{
    Connection, Proxy as _, QueueHandle,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

use crate::error::WindowError;
use crate::event::{Event, KeyCode as K, Modifiers};
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

pub(crate) mod translate;

// ── Wayland state ───────────────────────────────────────────────────

/// Everything the sctk handlers touch. Handlers run inside
/// `dispatch_pending(&mut WaylandState)` on the creating thread, so the
/// per-event buffer queue lives right here (no static needed — unlike
/// the Android backend, there are no cross-thread callbacks).
/// sctk handler 触及的全部状态。handler 都在创建线程的
/// `dispatch_pending(&mut WaylandState)` 内运行，所以逐事件缓冲队列直接
/// 放这里（无需 static——与 Android 后端不同，这里没有跨线程回调）。
struct WaylandState {
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    /// Kept as an `Arc` — the CSD frame needs a clone of it.
    /// 以 `Arc` 持有——CSD 装饰框需要它的克隆。
    compositor: Arc<CompositorState>,
    shm: Shm,
    window: SctkWindow,
    pool: SlotPool,
    /// Adwaita client-side decorations (Some means drawn as subsurfaces).
    /// Adwaita 客户端装饰（Some 表示以子表面绘制）。
    frame: Option<AdwaitaFrame<WaylandState>>,
    /// The CSD frame draws through subsurfaces created from this.
    /// CSD 装饰框经此创建子表面。
    subcompositor: Arc<SubcompositorState>,
    /// Pointer with a cursor theme — drives the resize-edge cursor icons
    /// the frame requests.
    /// 带光标主题的指针——响应装饰框请求的缩放边缘光标。
    themed_pointer: Option<ThemedPointer<()>>,
    /// Title text mirror for the frame (set before the frame exists).
    /// 装饰框的标题文本镜像（框创建前先行保存）。
    title: String,
    /// The buffer currently attached to the surface; reset on every
    /// configure (size changed) — SlotPool double-buffers the
    /// not-yet-released previous frame.
    /// 当前挂到 surface 的缓冲；每次 configure（尺寸变化）重置——
    /// SlotPool 对未释放的上一帧自动双缓冲。
    buffer: Option<Buffer>,
    /// Surface size in LOGICAL pixels (configure's `new_size`, or the
    /// requested size when the compositor sends `None`).
    /// surface 逻辑像素尺寸（configure 的 `new_size`；合成器发 `None`
    /// 时保持请求尺寸）。
    logical_w: u32,
    logical_h: u32,
    /// Integer output scale. Physical = logical × scale.
    /// 整数输出缩放。物理 = 逻辑 × scale。
    scale: i32,
    /// True until the compositor's first configure arrives (drives the
    /// initial draw + first `Resized`/`RedrawRequested`).
    /// 首个 configure 到达前为 true（驱动首次绘制与首条
    /// `Resized`/`RedrawRequested`）。
    first_configure: bool,
    fullscreen: bool,
    maximized: bool,
    /// Decoration mode as last configured (Client ⇒ borderless).
    /// 最近一次 configure 的装饰模式（Client ⇒ 无边框）。
    borderless: bool,
    /// Keyboard focus on our surface (wl_keyboard enter/leave).
    /// 键盘焦点在本 surface 上（wl_keyboard enter/leave）。
    keyboard_focus: bool,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    /// Snapshot of the last `update_modifiers` (event-time semantics).
    /// 最近一次 `update_modifiers` 的快照（事件时语义）。
    modifiers: SctkModifiers,
    /// Compositor repeat config; drives client-side repeat synthesis —
    /// compositors don't repeat keys themselves.
    /// 合成器重复配置；驱动客户端侧重复合成——合成器自己不重复按键。
    repeat_info: RepeatInfo,
    /// Armed client-side repeat state (disarmed on release/leave).
    /// 已装填的客户端重复状态（松键/失焦时解除）。
    repeat: Option<RepeatSynth>,
    /// Translated events waiting for `poll_event`.
    /// 等待 `poll_event` 取走的已翻译事件。
    pending: VecDeque<Event>,
}

/// Client-side key-repeat synthesizer state.
/// 客户端按键重复合成器状态。
struct RepeatSynth {
    raw_code: u32,
    key: K,
    text: Option<String>,
    next_at: Instant,
    interval: Duration,
}

/// Modifier evdev codes — repeating these is meaningless noise for text
/// hosts.
/// 修饰键 evdev 码——对文本宿主而言重复它们只是噪音。
const MODIFIER_CODES: [u32; 8] = [42, 54, 29, 97, 56, 100, 125, 126];

impl WaylandState {
    /// Fills the whole surface with the backdrop color and presents it.
    /// The library doesn't render — this just makes the window VISIBLE
    /// (wgpu hosts present their own frames into the same surface).
    /// 用底色填满整个 surface 并呈现。本库不渲染——这一步只是让窗口
    /// **可见**（wgpu 宿主往同一 surface 呈现自己的帧）。
    fn draw_fill(&mut self) {
        let (w, h) = self.physical_size();
        let stride = w as i32 * 4;
        let buffer = self.buffer.get_or_insert_with(|| {
            // Out-of-memory is unrecoverable for a windowing library; the
            // Windows backend panics on equivalent allocation failures too.
            // 内存耗尽对窗口库不可恢复；Windows 后端对同类分配失败同样 panic。
            self.pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                .expect("create wayland shm buffer / 创建 Wayland shm 缓冲失败")
                .0
        });
        let canvas = match self.pool.canvas(buffer) {
            Some(canvas) => canvas,
            None => {
                // The compositor hasn't released the previous frame yet —
                // SlotPool double-buffering fallback (same as the sctk
                // example).
                // 合成器尚未释放上一帧——SlotPool 双缓冲兜底（同 sctk 示例）。
                let (second, canvas) = self
                    .pool
                    .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
                    .expect("create wayland shm buffer / 创建 Wayland shm 缓冲失败");
                *buffer = second;
                canvas
            }
        };
        // ARGB8888 in little-endian memory = byte order [B, G, R, A].
        // ARGB8888 在小端内存中 = 字节序 [B, G, R, A]。
        canvas.chunks_exact_mut(4).for_each(|chunk| {
            chunk.copy_from_slice(&[0x2E, 0x2A, 0x2A, 0xFF]);
        });
        // Draw the CSD decorations — their subsurfaces sync with this
        // commit.
        // 绘制 CSD 装饰——其子表面与本提交同步。
        if let Some(frame) = self.frame.as_mut() {
            if frame.is_dirty() && !frame.is_hidden() {
                frame.draw();
            }
        }
        self.window
            .wl_surface()
            .damage_buffer(0, 0, w as i32, h as i32);
        let _ = self.window.set_buffer_scale(self.scale.max(1) as u32);
        buffer
            .attach_to(self.window.wl_surface())
            .expect("buffer attach / 缓冲挂接失败");
        self.window.commit();
    }

    /// Physical size = logical × scale (our public pixel convention).
    /// 物理尺寸 = 逻辑 × scale（本库公共像素约定）。
    fn physical_size(&self) -> (u32, u32) {
        let s = self.scale.max(1) as u32;
        (self.logical_w * s, self.logical_h * s)
    }

    /// Snapshot of the current modifier set (our type from sctk's).
    /// 当前修饰键快照（从 sctk 类型转到本库类型）。
    fn current_modifiers(&self) -> Modifiers {
        Modifiers {
            shift: self.modifiers.shift,
            ctrl: self.modifiers.ctrl,
            alt: self.modifiers.alt,
            meta: self.modifiers.logo,
        }
    }
}

// ── WindowInner ─────────────────────────────────────────────────────

/// The Linux platform window implementation (Wayland).
/// Linux 平台窗口实现（Wayland）。
///
/// All methods run on the creating thread (Wayland + sctk requirement);
/// the Mutex exists to keep `&self` methods mutating the shared core and
/// to make the type `Sync` for renderer bounds — the same trade-off as
/// the other backends.
/// 所有方法在创建线程上调用（Wayland + sctk 的要求）；Mutex 是为了让
/// `&self` 方法能改动共享核心并满足渲染端的 `Sync` bound——与其他后端
/// 同一取舍。
pub(crate) struct WindowInner {
    core: Mutex<WaylandCore>,
}

struct WaylandCore {
    conn: Connection,
    queue: wayland_client::EventQueue<WaylandState>,
    state: WaylandState,
    /// The title we last set (round-trips through `title()`).
    /// 最近设置的标题（供 `title()` 往返）。
    title: String,
    /// `resizable` is an illusion on xdg (min/max-size idiom) — we track
    /// the requested value plus the min/max to restore.
    /// xdg 上 `resizable` 是种假象（min/max-size 惯用法）——记录请求值
    /// 与待恢复的 min/max。
    resizable: bool,
    min_size: Option<(u32, u32)>,
    max_size: Option<(u32, u32)>,
    visible: bool,
    /// A fatal connection/protocol error surfaces as exactly one
    /// `CloseRequested` (hosts exit cleanly; the Android exit-contract
    /// philosophy).
    /// 致命连接/协议错误转化为恰好一条 `CloseRequested`（宿主干净退出；
    /// 与 Android 退出契约同一哲学）。
    dead: bool,
}

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        // 1. Connect to the compositor (WAYLAND_DISPLAY / WAYLAND_SOCKET).
        // 1. 连接合成器（WAYLAND_DISPLAY / WAYLAND_SOCKET）。
        let conn = Connection::connect_to_env().map_err(|err| {
            WindowError::OsError(
                format!(
                    "failed to connect to a Wayland compositor (WAYLAND_DISPLAY not set?) / 无法连接 Wayland 合成器（WAYLAND_DISPLAY 未设置？）: {err}"
                )
                .into(),
            )
        })?;

        // 2. Registry enumeration — the first (blocking) roundtrip.
        // 2. 注册表枚举——第一次（阻塞）往返。
        let (globals, mut queue) = registry_queue_init::<WaylandState>(&conn).map_err(|err| {
            WindowError::OsError(
                format!("wayland registry init failed / Wayland 注册表初始化失败: {err}").into(),
            )
        })?;
        let qh = queue.handle();

        // 3. Bind the globals we cannot live without.
        // 3. 绑定不可或缺的全局。
        let compositor = Arc::new(CompositorState::bind(&globals, &qh).map_err(|_| {
            WindowError::OsError("wl_compositor not available / wl_compositor 不可用".into())
        })?);
        let subcompositor = Arc::new(
            SubcompositorState::bind(compositor.wl_compositor().clone(), &globals, &qh).map_err(
                |_| {
                    WindowError::OsError(
                        "wl_subcompositor not available / wl_subcompositor 不可用".into(),
                    )
                },
            )?,
        );
        let xdg_shell = XdgShell::bind(&globals, &qh).map_err(|_| {
            WindowError::OsError(
                "xdg_wm_base (xdg-shell) not available / xdg_wm_base（xdg-shell）不可用".into(),
            )
        })?;
        let shm = Shm::bind(&globals, &qh)
            .map_err(|_| WindowError::OsError("wl_shm not available / wl_shm 不可用".into()))?;

        // 4. Surface → xdg window.
        // 4. Surface → xdg 窗口。
        let surface = compositor.create_surface(&qh);
        let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);

        // 5. Apply options (all sizes here are logical — scale is 1 until
        //    the first output attach).
        // 5. 应用选项（此处尺寸均为逻辑像素——首个 output 附着前 scale=1）。
        if !options.title.is_empty() {
            window.set_title(options.title.clone());
        }
        window.set_app_id("starfish-we");
        let (req_w, req_h) = (options.size.0.max(1), options.size.1.max(1));
        if let Some((min_w, min_h)) = options.min_size {
            window.set_min_size(Some((min_w, min_h)));
        }
        if let Some((max_w, max_h)) = options.max_size {
            window.set_max_size(Some((max_w, max_h)));
        }
        // xdg has no "resizable" request: a fixed-size window is the
        // min=max idiom (the compositor then refuses interactive resizes).
        // xdg 没有"可调"请求：固定尺寸窗口 = min=max 惯用法（合成器随后
        // 拒绝交互式调整）。
        if !options.resizable {
            window.set_min_size(Some((req_w, req_h)));
            window.set_max_size(Some((req_w, req_h)));
        }
        // Borderless is best-effort: compositors implementing
        // xdg-decoration honor the Client-mode request; others ignore it.
        // 无边框是 best-effort：实现 xdg-decoration 的合成器会尊重
        // Client 模式请求；其余忽略。
        if options.borderless {
            window.request_decoration_mode(Some(DecorationMode::Client));
        }
        // transparent/layered: documented no-ops on Wayland v1 (no opaque
        // region is set, so per-pixel alpha works with a client-rendered
        // transparent buffer anyway).
        // transparent/layered：Wayland v1 文档化 no-op（不设 opaque
        // region，客户端渲染透明缓冲时逐像素 alpha 本就可用）。

        // 6. Pixel pool for the visibility backdrop.
        // 6. 可见性底色的像素池。
        let pool = SlotPool::new((4 * req_w * req_h) as usize, &shm).map_err(|err| {
            WindowError::OsError(
                format!("wl_shm pool creation failed / wl_shm 池创建失败: {err}").into(),
            )
        })?;

        let mut state = WaylandState {
            registry_state: RegistryState::new(&globals),
            seat_state: SeatState::new(&globals, &qh),
            output_state: OutputState::new(&globals, &qh),
            compositor: compositor.clone(),
            subcompositor: subcompositor.clone(),
            shm,
            window,
            pool,
            frame: None,
            themed_pointer: None,
            title: options.title.clone(),
            buffer: None,
            logical_w: req_w,
            logical_h: req_h,
            scale: 1,
            first_configure: true,
            fullscreen: false,
            maximized: false,
            borderless: options.borderless,
            keyboard_focus: false,
            keyboard: None,
            modifiers: SctkModifiers {
                ctrl: false,
                alt: false,
                shift: false,
                caps_lock: false,
                logo: false,
                num_lock: false,
            },
            repeat_info: RepeatInfo::Disable,
            repeat: None,
            pending: VecDeque::new(),
        };

        // 7. Map the window: an initial EMPTY commit → the compositor
        //    replies with the first configure (which draws the backdrop
        //    and queues Resized/RedrawRequested). Hidden windows stay
        //    uncommitted (unmapped) until `set_visible(true)`.
        // 7. 映射窗口：初始**空 commit** → 合成器回首个 configure（其中
        //    画底色并入队 Resized/RedrawRequested）。HIDDEN 窗口保持未
        //    提交（不映射），直到 `set_visible(true)`。
        let hidden = options.hidden;
        if !hidden {
            state.window.commit();
            // Bounded roundtrips: the configure handler runs inside these
            // and flips `first_configure`. A compositor that never
            // configures just leaves the requested size in place.
            // 有界往返：configure 回调在其中运行并翻转 `first_configure`。
            // 从不 configure 的合成器则保持请求尺寸。
            for _ in 0..3 {
                queue.roundtrip(&mut state).map_err(|err| {
                    WindowError::OsError(
                        format!("wayland roundtrip failed / Wayland 往返失败: {err}").into(),
                    )
                })?;
                if !state.first_configure {
                    break;
                }
            }
        }
        let visible = !hidden;

        Ok(Self {
            core: Mutex::new(WaylandCore {
                conn,
                queue,
                state,
                title: options.title.clone(),
                resizable: options.resizable,
                min_size: options.min_size,
                max_size: options.max_size,
                visible,
                dead: false,
            }),
        })
    }

    fn lock_core(&self) -> MutexGuard<'_, WaylandCore> {
        // Poison tolerance: a panic in a handler must not wedge the
        // window; clear the queue on the way in (Android's pattern).
        // 毒锁容忍：handler 内的 panic 不能永久卡死窗口；进入时清队列
        // （Android 的手法）。
        self.core.lock().unwrap_or_else(|poisoned| {
            let mut guard = poisoned.into_inner();
            guard.state.pending.clear();
            guard
        })
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        let mut core = self.lock_core();
        if core.dead {
            return core.state.pending.pop_front();
        }
        // Destructure once: MutexGuard's Deref borrows the WHOLE core, so
        // `core.queue` + `&mut core.state` in the same expression would
        // double-borrow. Plain field access on `&mut *core` is disjoint.
        // 一次性解构：MutexGuard 的 Deref 借用的是**整个** core，同一
        // 表达式里 `core.queue` 配 `&mut core.state` 会双重借用；对
        // `&mut *core` 直接字段访问才是互不相交的。
        let WaylandCore {
            conn,
            queue,
            state,
            dead,
            ..
        } = &mut *core;

        // 1) Flush outbound requests; errors resurface in read/dispatch
        //    below (a full socket buffer is transient, not fatal).
        // 1) 冲刷出站请求；错误会在下方 read/dispatch 复现（socket 缓冲满
        //    属暂时性，非致命）。
        let _ = conn.flush();
        let _ = queue.flush();
        // 2) Dispatch already-buffered events.
        // 2) 派发已缓冲事件。
        if queue.dispatch_pending(state).is_err() {
            return mark_dead(state, dead);
        }
        // 3) Try to read more from the socket — an Err here is fatal (a
        //    WouldBlock-style idle read is handled inside wayland-client
        //    and surfaces as Ok(0)); `None` means events were already
        //    buffered (step 2 dispatched them).
        // 3) 尝试从 socket 再读——此处 Err 即致命（WouldBlock 类的空闲
        //    读在 wayland-client 内部已被消化为 Ok(0)）；`None` 表示已有
        //    事件缓冲（第 2 步已派发）。
        let read = queue.prepare_read().map(|guard| guard.read());
        if matches!(read, Some(Err(_))) {
            return mark_dead(state, dead);
        }
        // 4) Dispatch what step 3 read.
        // 4) 派发第 3 步读到的事件。
        if read.is_some() && queue.dispatch_pending(state).is_err() {
            return mark_dead(state, dead);
        }
        // 5) Client-side key-repeat synthesis.
        // 5) 客户端按键重复合成。
        synthesize_repeat(state);
        // 6) One event out.
        // 6) 取走一条事件。
        state.pending.pop_front()
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.lock_core().state.physical_size()
    }

    pub(crate) fn set_title(&self, title: &str) {
        let mut core = self.lock_core();
        core.title = title.to_string();
        core.state.title = title.to_string();
        core.state.window.set_title(title.to_string());
        if let Some(frame) = core.state.frame.as_mut() {
            frame.set_title(title.to_string());
        }
    }

    pub(crate) fn title(&self) -> String {
        self.lock_core().title.clone()
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        let core = self.lock_core();
        RawWindowHandle::Wayland {
            // SAFETY: the `system` wayland backend guarantees real
            // libwayland pointers; validity = the Window's lifetime.
            // SAFETY：`system` wayland 后端保证这是真实 libwayland 指针；
            // 有效性 = Window 的存活期。
            display: core.conn.backend().display_ptr() as *mut std::ffi::c_void,
            surface: core.state.window.wl_surface().id().as_ptr() as *mut std::ffi::c_void,
        }
    }

    pub(crate) fn set_resizable(&self, resizable: bool) {
        let mut core = self.lock_core();
        core.resizable = resizable;
        // The min/max idiom: restoring = clear both bounds (None).
        // min/max 惯用法：恢复 = 清空两个边界（None）。
        let (min, max) = if resizable {
            (core.min_size, core.max_size)
        } else {
            let size = core.state.physical_size();
            (Some(size), Some(size))
        };
        core.state.window.set_min_size(min);
        core.state.window.set_max_size(max);
    }

    pub(crate) fn is_resizable(&self) -> bool {
        self.lock_core().resizable
    }

    pub(crate) fn set_size(&self, width: u32, height: u32) {
        let mut core = self.lock_core();
        if core.dead {
            return;
        }
        // Advisory on tiling compositors (documented): we redraw our
        // buffer at the requested size; the toplevel frame may ignore it.
        // 平铺合成器下属咨询性操作（已文档化）：按请求尺寸重绘缓冲；
        // toplevel 边框可能无视。
        core.state.logical_w = width.max(1);
        core.state.logical_h = height.max(1);
        core.state.buffer = None;
        core.state.draw_fill();
        let (w, h) = core.state.physical_size();
        core.state.pending.push_back(Event::Resized {
            width: w,
            height: h,
        });
    }

    pub(crate) fn set_fullscreen(&self, fullscreen: bool) {
        let core = self.lock_core();
        if fullscreen {
            core.state.window.set_fullscreen(None);
        } else {
            core.state.window.unset_fullscreen();
        }
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        self.lock_core().state.fullscreen
    }

    pub(crate) fn set_borderless(&self, borderless: bool) {
        let mut core = self.lock_core();
        core.state.borderless = borderless;
        // Best-effort: honored by compositors implementing xdg-decoration.
        // Best-effort：实现 xdg-decoration 的合成器会尊重。
        core.state
            .window
            .request_decoration_mode(Some(if borderless {
                DecorationMode::Client
            } else {
                DecorationMode::Server
            }));
    }

    pub(crate) fn is_borderless(&self) -> bool {
        self.lock_core().state.borderless
    }

    pub(crate) fn set_mouse_grabbed(&self, _grabbed: bool) {
        // Pointer confinement (zwp_pointer_constraints_v1) is a follow-up.
        // 指针约束（zwp_pointer_constraints_v1）属后续项。
    }

    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        false
    }

    pub(crate) fn set_mouse_relative(&self, _enable: bool) {
        // zwp_relative_pointer_v1 is a follow-up.
        // zwp_relative_pointer_v1 属后续项。
    }

    pub(crate) fn set_cursor_visible(&self, _visible: bool) {
        // Wayland cursors require a cursor surface/shape protocol — v1
        // documented limitation (the OS default cursor always shows).
        // Wayland 光标需要 cursor surface/shape 协议——v1 文档化限制
        // （系统默认光标始终显示）。
    }

    pub(crate) fn is_cursor_visible(&self) -> bool {
        true
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        let mut core = self.lock_core();
        if core.dead || core.visible == visible {
            return;
        }
        core.visible = visible;
        if visible {
            core.state.draw_fill();
        } else {
            // Unmap: attach no buffer and commit.
            // 解映射：挂空缓冲并提交。
            core.state.window.wl_surface().attach(None, 0, 0);
            core.state.window.commit();
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.lock_core().visible
    }

    pub(crate) fn set_always_on_top(&self, _top: bool) {
        // No xdg equivalent (xdg-activation is about focus tokens).
        // xdg 无对应请求（xdg-activation 管的是焦点令牌）。
    }

    pub(crate) fn is_always_on_top(&self) -> bool {
        false
    }

    pub(crate) fn minimize(&self) {
        self.lock_core().state.window.set_minimized();
    }

    pub(crate) fn maximize(&self) {
        self.lock_core().state.window.set_maximized();
    }

    pub(crate) fn restore(&self) {
        let core = self.lock_core();
        core.state.window.unset_maximized();
        core.state.window.unset_fullscreen();
    }

    pub(crate) fn raise(&self) {
        // No xdg raise request; the compositor decides stacking.
        // xdg 无 raise 请求；堆叠由合成器决定。
    }

    pub(crate) fn is_minimized(&self) -> bool {
        // No reliable signal reaches clients; documented approximation.
        // 客户端拿不到可靠信号；文档化近似。
        false
    }

    pub(crate) fn is_maximized(&self) -> bool {
        self.lock_core().state.maximized
    }

    pub(crate) fn has_input_focus(&self) -> bool {
        self.lock_core().state.keyboard_focus
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

    pub(crate) fn set_position(&self, _x: i32, _y: i32) {
        // Compositor-decided on Wayland.
        // Wayland 上由合成器决定。
    }

    pub(crate) fn position(&self) -> (i32, i32) {
        (0, 0)
    }

    pub(crate) fn center_on_screen(&self) {}

    pub(crate) fn set_opacity(&self, _opacity: f32) {}

    pub(crate) fn opacity(&self) -> f32 {
        1.0
    }

    pub(crate) fn dpi_scale(&self) -> f32 {
        self.lock_core().state.scale as f32
    }

    pub(crate) fn high_pixel_density(&self) -> bool {
        true
    }
}

/// Converts a fatal connection error into exactly one `CloseRequested`,
/// then hands the caller whatever is queued.
/// 把致命连接错误转化为恰好一条 `CloseRequested`，随后交还队列中已有
/// 的事件。
fn mark_dead(state: &mut WaylandState, dead: &mut bool) -> Option<Event> {
    if !*dead {
        *dead = true;
        state.repeat = None;
        state.pending.clear();
        state.pending.push_back(Event::CloseRequested);
    }
    state.pending.pop_front()
}

/// Pushes `KeyDown{repeat:true}` + `TextInput` for every due repeat tick
/// (bounded per poll so a starved host can't amass an event avalanche).
/// 每个到期的重复节拍推一条 `KeyDown{repeat:true}` + `TextInput`
/// （单次 poll 有界，避免被饿死的宿主积压事件雪崩）。
fn synthesize_repeat(state: &mut WaylandState) {
    let now = Instant::now();
    let due = state.repeat.as_mut().map_or(0, |rep| {
        let mut n = 0;
        while rep.next_at <= now && n < 32 {
            rep.next_at += rep.interval;
            n += 1;
        }
        n
    });
    for _ in 0..due {
        let Some(rep) = &state.repeat else { break };
        state.pending.push_back(Event::KeyDown {
            key: rep.key,
            modifiers: state.current_modifiers(),
            repeat: true,
        });
        if let Some(text) = &rep.text {
            state
                .pending
                .push_back(Event::TextInput { text: text.clone() });
        }
    }
}

// ── sctk handlers (callbacks → PENDING translations) ────────────────
// ── sctk handler（回调 → PENDING 翻译）────────────────────────────────

impl CompositorHandler for WaylandState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        if self.scale != new_factor {
            self.scale = new_factor;
            if let Some(frame) = self.frame.as_mut() {
                frame.set_scaling_factor(f64::from(new_factor));
            }
            self.buffer = None;
            let (w, h) = self.physical_size();
            self.pending.push_back(Event::Resized {
                width: w,
                height: h,
            });
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
        // 90°-rotated outputs are a documented v1 non-goal.
        // 90° 旋转输出是文档化的 v1 非目标。
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        // Frame callbacks drive continuous rendering; this library has no
        // render loop (hosts tick + present themselves / wgpu-auto).
        // frame 回调驱动连续渲染；本库没有渲染循环（宿主自行 tick +
        // 呈现 / wgpu-auto）。
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for WaylandState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl WindowHandler for WaylandState {
    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &SctkWindow) {
        // Title-bar close button / compositor-initiated close.
        // 标题栏关闭按钮 / 合成器发起的关闭。
        self.pending.push_back(Event::CloseRequested);
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _window: &SctkWindow,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        // A new size may mean a new buffer; SlotPool handles the
        // not-yet-released previous frame via its double-buffer fallback.
        // 新尺寸可能意味着新缓冲；SlotPool 以双缓冲兜底处理未释放的
        // 上一帧。
        self.buffer = None;
        let old = (self.logical_w, self.logical_h);

        // Server-side decorations (KWin & co.) draw the frame themselves.
        // Client mode (Mutter lacks xdg-decoration) uses our Adwaita frame.
        // 服务端装饰（KWin 等）由合成器绘制；Client 模式（Mutter 无
        // xdg-decoration）则用我们的 Adwaita 框。
        let (content_w, content_h) = if configure.decoration_mode == DecorationMode::Client {
            let frame = self.frame.get_or_insert_with(|| {
                let mut frame = AdwaitaFrame::new(
                    &self.window,
                    &self.shm,
                    self.compositor.clone(),
                    self.subcompositor.clone(),
                    qh.clone(),
                    FrameConfig::auto(),
                )
                .expect("create Adwaita CSD frame / 创建 Adwaita CSD 装饰框失败");
                frame.set_title(self.title.clone());
                frame.set_scaling_factor(f64::from(self.scale.max(1)));
                frame
            });
            frame.set_hidden(false);
            frame.update_state(configure.state);
            frame.update_wm_capabilities(configure.capabilities);
            let (cw, ch) = match configure.new_size {
                (Some(w), Some(h)) => frame.subtract_borders(w, h),
                _ => (
                    NonZeroU32::new(self.logical_w),
                    NonZeroU32::new(self.logical_h),
                ),
            };
            let cw = cw.unwrap_or(NonZeroU32::new(1).unwrap());
            let ch = ch.unwrap_or(NonZeroU32::new(1).unwrap());
            frame.resize(cw, ch);
            // Exclude the titlebar strip from the toplevel geometry.
            // 把标题栏区域从 toplevel 几何中排除。
            let (x, y) = frame.location();
            let (outer_w, outer_h) = frame.add_borders(cw.get(), ch.get());
            self.window
                .xdg_surface()
                .set_window_geometry(x, y, outer_w as i32, outer_h as i32);
            (cw.get(), ch.get())
        } else {
            if let Some(frame) = self.frame.as_mut() {
                frame.set_hidden(true);
            }
            let w = configure
                .new_size
                .0
                .map(|v| v.get())
                .unwrap_or(self.logical_w);
            let h = configure
                .new_size
                .1
                .map(|v| v.get())
                .unwrap_or(self.logical_h);
            self.window
                .xdg_surface()
                .set_window_geometry(0, 0, w as i32, h as i32);
            (w, h)
        };

        self.logical_w = content_w;
        self.logical_h = content_h;
        self.fullscreen = configure.state.contains(WindowState::FULLSCREEN);
        self.maximized = configure.state.contains(WindowState::MAXIMIZED);
        self.borderless = configure.decoration_mode == DecorationMode::Client;

        self.draw_fill();

        let changed = (self.logical_w, self.logical_h) != old;
        if self.first_configure || changed {
            let (w, h) = self.physical_size();
            self.pending.push_back(Event::Resized {
                width: w,
                height: h,
            });
        }
        if self.first_configure {
            self.first_configure = false;
            self.pending.push_back(Event::RedrawRequested);
        }
    }
}

impl SeatHandler for WaylandState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                // `get_keyboard` (not `get_keyboard_with_repeat`) — the
                // repeat helper is calloop-gated and we synthesize
                // repeats ourselves.
                // 用 `get_keyboard`（非 `get_keyboard_with_repeat`）——
                // repeat 辅助器被 calloop 门控，重复我们自行合成。
                self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
            }
            Capability::Pointer if self.themed_pointer.is_none() => {
                // Themed pointer: the frame's resize-edge cursor icons
                // need a cursor surface + theme to render through.
                // 主题指针：装饰框的缩放边缘光标需要 cursor surface +
                // 主题才能渲染。
                let cursor_surface = self.compositor.create_surface(qh);
                self.themed_pointer = self
                    .seat_state
                    .get_pointer_with_theme::<WaylandState, ()>(
                        qh,
                        &seat,
                        self.shm.wl_shm(),
                        cursor_surface,
                        ThemeSpec::default(),
                    )
                    .ok();
            }
            _ => {}
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Keyboard => {
                if let Some(keyboard) = self.keyboard.take() {
                    keyboard.release();
                }
                self.repeat = None;
                if self.keyboard_focus {
                    self.keyboard_focus = false;
                    self.pending.push_back(Event::FocusLost);
                }
            }
            Capability::Pointer => {
                if let Some(pointer) = self.themed_pointer.take() {
                    pointer.pointer().release();
                }
            }
            _ => {}
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {
    }
}

impl KeyboardHandler for WaylandState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw: &[u32],
        _keysyms: &[Keysym],
    ) {
        if self.window.wl_surface() == surface && !self.keyboard_focus {
            self.keyboard_focus = true;
            self.pending.push_back(Event::FocusGained);
        }
    }

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {
        if self.window.wl_surface() == surface && self.keyboard_focus {
            self.keyboard_focus = false;
            self.repeat = None;
            self.pending.push_back(Event::FocusLost);
        }
    }

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        let key = translate::evdev_to_keycode(event.raw_code);
        let text = event.utf8.filter(|t| !t.is_empty());
        self.pending.push_back(Event::KeyDown {
            key,
            modifiers: self.current_modifiers(),
            repeat: false,
        });
        if let Some(text) = &text {
            self.pending
                .push_back(Event::TextInput { text: text.clone() });
        }
        // Arm the client-side repeat (modifier keys don't repeat).
        // 装填客户端重复（修饰键不重复）。
        self.repeat = match &self.repeat_info {
            RepeatInfo::Repeat { rate, delay } if !MODIFIER_CODES.contains(&event.raw_code) => {
                Some(RepeatSynth {
                    raw_code: event.raw_code,
                    key,
                    text,
                    next_at: Instant::now() + Duration::from_millis(u64::from(*delay)),
                    interval: Duration::from_secs_f64(1.0 / f64::from(rate.get())),
                })
            }
            _ => None,
        };
    }

    fn repeat_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        // wl_keyboard v10 compositors that repeat themselves.
        // 自行重复的 wl_keyboard v10 合成器。
        self.pending.push_back(Event::KeyDown {
            key: translate::evdev_to_keycode(event.raw_code),
            modifiers: self.current_modifiers(),
            repeat: true,
        });
        if let Some(text) = event.utf8.filter(|t| !t.is_empty()) {
            self.pending.push_back(Event::TextInput { text });
        }
    }

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        event: KeyEvent,
    ) {
        if self
            .repeat
            .as_ref()
            .is_some_and(|r| r.raw_code == event.raw_code)
        {
            self.repeat = None;
        }
        self.pending.push_back(Event::KeyUp {
            key: translate::evdev_to_keycode(event.raw_code),
            modifiers: self.current_modifiers(),
        });
    }

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: SctkModifiers,
        _raw_modifiers: RawModifiers,
        _layout: u32,
    ) {
        self.modifiers = modifiers;
    }
}

impl PointerHandler for WaylandState {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _qh: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        use PointerEventKind::*;
        for event in events {
            let (x, y) = event.position;
            let on_content = event.surface == *self.window.wl_surface();
            let scale = self.scale.max(1);
            match event.kind {
                Enter { .. } => {
                    // The frame tracks hover over its own subsurfaces
                    // (edges/buttons) and reports the matching cursor icon.
                    // 装饰框会追踪其子表面（边缘/按钮）上的悬停并给出对应
                    // 光标图标。
                    if let Some(icon) = self.frame.as_mut().and_then(|frame| {
                        frame.click_point_moved(Duration::ZERO, &event.surface.id(), x, y)
                    }) {
                        if let Some(tp) = self.themed_pointer.as_mut() {
                            let _ = tp.set_cursor(conn, icon);
                        }
                    }
                    if on_content {
                        self.pending.push_back(Event::MouseMoved {
                            x: x * f64::from(scale),
                            y: y * f64::from(scale),
                        });
                    }
                }
                Leave { .. } => {
                    if !on_content {
                        if let Some(frame) = self.frame.as_mut() {
                            frame.click_point_left();
                        }
                    }
                }
                Motion { time } => {
                    if let Some(icon) = self.frame.as_mut().and_then(|frame| {
                        frame.click_point_moved(
                            Duration::from_millis(u64::from(time)),
                            &event.surface.id(),
                            x,
                            y,
                        )
                    }) {
                        if let Some(tp) = self.themed_pointer.as_mut() {
                            let _ = tp.set_cursor(conn, icon);
                        }
                    }
                    if on_content {
                        self.pending.push_back(Event::MouseMoved {
                            x: x * f64::from(scale),
                            y: y * f64::from(scale),
                        });
                    }
                }
                Press {
                    button,
                    serial,
                    time,
                }
                | Release {
                    button,
                    serial,
                    time,
                } => {
                    let pressed = matches!(event.kind, Press { .. });
                    if on_content {
                        // Content press/release: plain mouse events.
                        // 内容区按下/松开：普通鼠标事件。
                        self.pending.push_back(if pressed {
                            Event::MouseDown {
                                button: translate::mouse_button(button),
                            }
                        } else {
                            Event::MouseUp {
                                button: translate::mouse_button(button),
                            }
                        });
                    } else if let Some(action) = self.frame.as_mut().and_then(|frame| {
                        frame.on_click(
                            Duration::from_millis(u64::from(time)),
                            frame_click(button),
                            pressed,
                        )
                    }) {
                        self.frame_action(pointer, serial, action);
                    }
                }
                Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    if !on_content {
                        continue;
                    }
                    // wl vertical axis: positive = scroll DOWN; our
                    // convention is positive = UP, so negate. Horizontal
                    // already matches (positive = right).
                    // wl 垂直轴：正 = 向下滚；本库约定正 = 向上，取负。
                    // 水平轴本就一致（正 = 向右）。
                    if horizontal.value120 != 0 || horizontal.discrete != 0 {
                        self.pending.push_back(Event::MouseWheel {
                            delta_x: translate::wheel_delta(
                                horizontal.value120,
                                horizontal.discrete,
                            ),
                            delta_y: 0.0,
                        });
                    }
                    if vertical.value120 != 0 || vertical.discrete != 0 {
                        self.pending.push_back(Event::MouseWheel {
                            delta_x: 0.0,
                            delta_y: -translate::wheel_delta(vertical.value120, vertical.discrete),
                        });
                    }
                }
            }
        }
    }
}

/// Evdev left/right buttons → the frame's click kinds; other buttons
/// don't participate in decoration interactions.
/// evdev 左/右键 → 装饰框点击类型；其余按键不参与装饰交互。
fn frame_click(button: u32) -> FrameClick {
    match button {
        0x111 => FrameClick::Alternate,
        _ => FrameClick::Normal,
    }
}

impl WaylandState {
    /// Executes a CSD frame action (close / minimize / maximize / drag /
    /// resize) using the seat + serial of the originating click.
    /// 用发起点击的 seat + serial 执行 CSD 装饰框动作（关闭/最小化/
    /// 最大化/拖拽/缩放）。
    fn frame_action(&mut self, pointer: &wl_pointer::WlPointer, serial: u32, action: FrameAction) {
        let seat = pointer
            .data::<PointerData<()>>()
            .map(|data| data.seat().clone());
        match action {
            FrameAction::Close => self.pending.push_back(Event::CloseRequested),
            FrameAction::Minimize => self.window.set_minimized(),
            FrameAction::Maximize => self.window.set_maximized(),
            FrameAction::UnMaximize => self.window.unset_maximized(),
            FrameAction::ShowMenu(x, y) => {
                if let Some(seat) = &seat {
                    self.window.show_window_menu(seat, serial, (x, y));
                }
            }
            FrameAction::Resize(edge) => {
                if let Some(seat) = &seat {
                    let edge = match edge {
                        CsdResizeEdge::None => XdgResizeEdge::None,
                        CsdResizeEdge::Top => XdgResizeEdge::Top,
                        CsdResizeEdge::Bottom => XdgResizeEdge::Bottom,
                        CsdResizeEdge::Left => XdgResizeEdge::Left,
                        CsdResizeEdge::Right => XdgResizeEdge::Right,
                        CsdResizeEdge::TopLeft => XdgResizeEdge::TopLeft,
                        CsdResizeEdge::TopRight => XdgResizeEdge::TopRight,
                        CsdResizeEdge::BottomLeft => XdgResizeEdge::BottomLeft,
                        CsdResizeEdge::BottomRight => XdgResizeEdge::BottomRight,
                        _ => return,
                    };
                    self.window.resize(seat, serial, edge);
                }
            }
            FrameAction::Move => {
                if let Some(seat) = &seat {
                    self.window.move_(seat, serial);
                }
            }
            _ => {}
        }
    }
}

impl ShmHandler for WaylandState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

// ── sctk 0.21 delegate wiring ───────────────────────────────────────

delegate_registry!(WaylandState);

impl ProvidesRegistryState for WaylandState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState,];
}

smithay_client_toolkit::delegate_dispatch2!(WaylandState);

impl Drop for WindowInner {
    fn drop(&mut self) {
        let Ok(core) = self.core.lock() else {
            return;
        };
        if core.dead {
            return;
        }
        // Best-effort surface destroy; closing the connection makes the
        // compositor reclaim everything else.
        // Best-effort 销毁 surface；连接关闭后合成器会回收其余对象。
        let _ = core.state.window.wl_surface().destroy();
    }
}
