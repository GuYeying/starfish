//! Win32 backend.
//! Win32 后端。
//!
//! Model: [`poll_event`](WindowInner::poll_event) drains this thread's
//! messages non-blockingly with `PeekMessageW`; `DispatchMessageW` feeds
//! [`wndproc`], which translates messages into the pending queue; one event
//! is returned per call. Control flow always stays with the caller.
//! 模型：[`poll_event`](WindowInner::poll_event) 用 `PeekMessageW` 非阻塞把
//! 本线程消息泵干，`DispatchMessageW` → [`wndproc`] 翻译消息，再从待发队列
//! 取一个返回。控制流始终在调用者手里。
//!
//! Conventions:
//! 约定：
//! - Sizes / coordinates are physical pixels (the process declares
//!   Per-Monitor V2 DPI awareness).
//!   尺寸 / 坐标均为物理像素（进程声明 Per-Monitor V2 DPI 感知）。
//! - `TextInput` goes through raw `WM_CHAR` passthrough; no IME
//!   composition (first-version agreement).
//!   `TextInput` 走 `WM_CHAR` 直通，不接 IME 组合过程（首版约定）。
//! - `WM_CLOSE` does not destroy the window: destruction is decided by
//!   dropping the [`Window`](crate::Window).
//!   `WM_CLOSE` 不直接销毁窗口：是否销毁由用户 drop [`Window`](crate::Window) 决定。
//!
//! Layering: all message→event translation lives in [`translate`]; this
//! module handles window creation, message pumping, state attachment and
//! system-default fallback.
//! 分层：消息 → 事件的翻译全部在 [`translate`]；
//! 本模块只负责建窗、泵消息、状态挂载与系统默认兜底。

mod translate;

use std::iter::once;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use std::collections::VecDeque;

use windows_sys::Win32::Foundation::{
    GetLastError, HMODULE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows_sys::Win32::Graphics::Gdi as gdi;
use windows_sys::Win32::System::LibraryLoader as lib;
use windows_sys::Win32::UI::HiDpi as hid;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

use crate::error::WindowError;
use crate::event::Event;
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

/// The window class name; registered once per process.
/// 窗口类名；进程内只注册一次。
const CLASS_NAME: &str = "starfish-we";

pub(crate) struct WindowInner {
    hwnd: HWND,
    /// The state block shared with [`wndproc`] via `GWLP_USERDATA`
    /// (boxed at creation, reclaimed in `Drop`).
    /// 经 `GWLP_USERDATA` 与 [`wndproc`] 共享的状态块（创建时 boxed，Drop 回收）。
    state: *mut WndState,
}

/// Per-window state shared by the window procedure and `poll_event`.
/// Single window + Win32 thread affinity: no synchronization needed.
/// 窗口过程与 `poll_event` 共享的每窗口状态。
/// 单窗口 + Win32 窗口线程亲和，无需同步。
struct WndState {
    hwnd: HWND,
    hinstance: HMODULE,
    /// Last known client size (physical pixels).
    /// 最近一次已知客户区尺寸（物理像素）。
    size: (u32, u32),
    /// Lower bound for user-driven client resizes.
    /// 用户拖拽调整的客户区尺寸下限。
    min_size: Option<(u32, u32)>,
    /// Upper bound for user-driven client resizes.
    /// 用户拖拽调整的客户区尺寸上限。
    max_size: Option<(u32, u32)>,
    /// Events translated by the wndproc, awaiting dispatch.
    /// wndproc 翻译出的待发事件。
    pending: VecDeque<Event>,
    /// Bitmask of held mouse buttons (see `translate::button_bit`),
    /// driving `SetCapture` / `ReleaseCapture`.
    /// 按下中的鼠标键位掩码（见 `translate::button_bit`），
    /// 驱动 `SetCapture` / `ReleaseCapture`。
    mouse_down: u8,
    // ── Runtime window state (SDL flag semantics on Win32) /
    //    运行期窗口状态（SDL flag 语义的 Win32 落地）──
    /// In fullscreen (placement saved on entry for restoration).
    /// 全屏中（进入时保存 placement 供退出还原）。
    fullscreen: bool,
    /// Window position and size before entering fullscreen.
    /// 进入全屏前的窗口位置与尺寸。
    saved_placement: Option<RECT>,
    /// Cursor visible (`ShowCursor` is a counter; managed here as a single
    /// show/hide by boolean).
    /// 光标可见（ShowCursor 为计数器，这里按布尔管理一次显/隐）。
    cursor_visible: bool,
    /// Mouse confined to the window client area (`ClipCursor`).
    /// 鼠标被捕获在窗口客户区内（ClipCursor）。
    mouse_grabbed: bool,
    /// Window opacity (1.0 = opaque; <1.0 requires `WS_EX_LAYERED`).
    /// 窗口不透明度（1.0 = 全不透明；<1.0 需 WS_EX_LAYERED）。
    opacity: f32,
}

impl WindowInner {
    pub(crate) fn new(options: &WindowOptions) -> Result<Self, WindowError> {
        // SAFETY: every FFI call passes documented-valid parameters; the
        // state block is reclaimed on all failure paths.
        // SAFETY: 各 FFI 调用均按文档传入有效参数；失败路径统一回收 state。
        unsafe { Self::create(options) }
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        let state = self.state;
        // SAFETY: state is valid for the window's lifetime and accessed
        // only on this thread (Win32 thread affinity).
        // SAFETY: state 在窗口存活期内有效，且仅本线程访问（Win32 线程亲和）。
        unsafe {
            // Drain this thread's message queue first; DispatchMessageW
            // feeds wndproc which appends to `pending`.
            // 先把本线程消息泵干；DispatchMessageW 经 wndproc 写入 pending。
            loop {
                let mut msg: wm::MSG = mem::zeroed();
                if wm::PeekMessageW(&mut msg, ptr::null_mut(), 0, 0, wm::PM_REMOVE) == 0 {
                    break;
                }
                if msg.message == wm::WM_QUIT {
                    // Someone asked the message thread to quit: report as a
                    // close request.
                    // 外部要求退出消息线程：按关闭请求上报。
                    (*state).pending.push_back(Event::CloseRequested);
                    continue;
                }
                // TranslateMessage produces WM_CHAR (the TextInput source).
                // TranslateMessage 产出 WM_CHAR（TextInput 的来源）。
                wm::TranslateMessage(&msg);
                wm::DispatchMessageW(&msg);
            }
            (*state).pending.pop_front()
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        // SAFETY: state is valid for the window's lifetime, this-thread only.
        // SAFETY: state 在窗口存活期内有效，且仅本线程访问。
        unsafe { (*self.state).size }
    }

    pub(crate) fn set_title(&self, title: &str) {
        let title_w: Vec<u16> = title.encode_utf16().chain(once(0)).collect();
        // SAFETY: title_w is NUL-terminated and outlives the call.
        // SAFETY: title_w 以 NUL 结尾，存活至调用返回。
        unsafe {
            wm::SetWindowTextW(self.hwnd, title_w.as_ptr());
        }
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        // SAFETY: state is valid for the window's lifetime, this-thread only.
        // SAFETY: state 在窗口存活期内有效，且仅本线程访问。
        let state = unsafe { &*self.state };
        RawWindowHandle::Win32 {
            hwnd: state.hwnd,
            hinstance: state.hinstance,
        }
    }

    // ── Runtime window capabilities (SDL flag semantics on Win32) /
    //    运行期窗口能力（SDL flag 语义的 Win32 落地）──────────────────

    /// Reads `GWL_STYLE` (WS_* constants are u32).
    /// GWL_STYLE 读样式（WS_* 常量为 u32）。
    fn style_raw(&self) -> u32 {
        // SAFETY: hwnd 在窗口存活期内有效。
        unsafe { wm::GetWindowLongW(self.hwnd, wm::GWL_STYLE) as u32 }
    }

    /// GWL_STYLE 写样式并使非客户区重算
    fn set_style_raw(&self, style: u32) {
        // SAFETY: hwnd 在窗口存活期内有效；FRAMECHANGED 使样式立即生效。
        unsafe {
            wm::SetWindowLongW(self.hwnd, wm::GWL_STYLE, style as i32);
            wm::SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                0,
                0,
                wm::SWP_NOMOVE
                    | wm::SWP_NOSIZE
                    | wm::SWP_NOZORDER
                    | wm::SWP_NOACTIVATE
                    | wm::SWP_FRAMECHANGED,
            );
        }
    }

    pub(crate) fn set_resizable(&self, resizable: bool) {
        let style = self.style_raw();
        let new_style = if resizable {
            style | (wm::WS_THICKFRAME | wm::WS_MAXIMIZEBOX)
        } else {
            style & !(wm::WS_THICKFRAME | wm::WS_MAXIMIZEBOX)
        };
        self.set_style_raw(new_style);
    }

    pub(crate) fn is_resizable(&self) -> bool {
        (self.style_raw() & wm::WS_THICKFRAME) != 0
    }

    pub(crate) fn set_fullscreen(&self, fullscreen: bool) {
        // SAFETY: hwnd 有效；进入全屏保存 placement，退出按 placement 还原。
        unsafe {
            let state = &mut *self.state;
            if fullscreen == state.fullscreen {
                return;
            }
            if fullscreen {
                let mut placement = wm::WINDOWPLACEMENT {
                    length: mem::size_of::<wm::WINDOWPLACEMENT>() as u32,
                    ..mem::zeroed()
                };
                wm::GetWindowPlacement(self.hwnd, &mut placement);
                state.saved_placement = Some(RECT {
                    left: placement.rcNormalPosition.left,
                    top: placement.rcNormalPosition.top,
                    right: placement.rcNormalPosition.right,
                    bottom: placement.rcNormalPosition.bottom,
                });
                state.fullscreen = true;

                // Strip decorations + fill the current monitor.
                // 去装饰 + 铺满所在显示器
                let style = wm::GetWindowLongW(self.hwnd, wm::GWL_STYLE) as u32;
                wm::SetWindowLongW(
                    self.hwnd,
                    wm::GWL_STYLE,
                    ((style & !wm::WS_OVERLAPPEDWINDOW) | wm::WS_POPUP) as i32,
                );
                let monitor = gdi::MonitorFromWindow(self.hwnd, gdi::MONITOR_DEFAULTTONEAREST);
                let mut mi = gdi::MONITORINFO {
                    cbSize: mem::size_of::<gdi::MONITORINFO>() as u32,
                    ..mem::zeroed()
                };
                if gdi::GetMonitorInfoW(monitor, &mut mi) != 0 {
                    let rc = mi.rcMonitor;
                    wm::SetWindowPos(
                        self.hwnd,
                        ptr::null_mut(),
                        rc.left,
                        rc.top,
                        rc.right - rc.left,
                        rc.bottom - rc.top,
                        wm::SWP_NOOWNERZORDER | wm::SWP_FRAMECHANGED,
                    );
                }
            } else {
                state.fullscreen = false;
                let style = wm::GetWindowLongW(self.hwnd, wm::GWL_STYLE) as u32;
                wm::SetWindowLongW(
                    self.hwnd,
                    wm::GWL_STYLE,
                    (style | wm::WS_OVERLAPPEDWINDOW) as i32,
                );
                if let Some(rc) = state.saved_placement.take() {
                    wm::SetWindowPos(
                        self.hwnd,
                        ptr::null_mut(),
                        rc.left,
                        rc.top,
                        rc.right - rc.left,
                        rc.bottom - rc.top,
                        wm::SWP_NOOWNERZORDER | wm::SWP_FRAMECHANGED,
                    );
                }
            }
        }
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        // SAFETY: state 在窗口存活期内有效。
        unsafe { (*self.state).fullscreen }
    }

    pub(crate) fn set_borderless(&self, borderless: bool) {
        let style = self.style_raw();
        let new_style = if borderless {
            (style & !(wm::WS_CAPTION | wm::WS_THICKFRAME)) | wm::WS_POPUP
        } else {
            (style & !wm::WS_POPUP) | wm::WS_CAPTION | wm::WS_THICKFRAME
        };
        self.set_style_raw(new_style);
    }

    pub(crate) fn is_borderless(&self) -> bool {
        (self.style_raw() & wm::WS_CAPTION) == 0
    }

    pub(crate) fn set_mouse_grabbed(&self, grabbed: bool) {
        // SAFETY: hwnd 有效；ClipCursor 限定在本窗口客户区（屏幕坐标）。
        unsafe {
            let state = &mut *self.state;
            state.mouse_grabbed = grabbed;
            if grabbed {
                let mut cr = mem::zeroed();
                wm::GetClientRect(self.hwnd, &mut cr);
                let tl = POINT {
                    x: cr.left,
                    y: cr.top,
                };
                let br = POINT {
                    x: cr.right,
                    y: cr.bottom,
                };
                let mut stl = tl;
                let mut sbr = br;
                gdi::ClientToScreen(self.hwnd, &mut stl);
                gdi::ClientToScreen(self.hwnd, &mut sbr);
                let clip = RECT {
                    left: stl.x,
                    top: stl.y,
                    right: sbr.x,
                    bottom: sbr.y,
                };
                wm::ClipCursor(&clip);
            } else {
                wm::ClipCursor(ptr::null());
            }
        }
    }

    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        // SAFETY: state is valid for the window's lifetime. / state 在窗口存活期内有效。
        unsafe { (*self.state).mouse_grabbed }
    }

    /// Relative mouse mode, v0 approximation: clip + hide cursor (a true
    /// raw-input mode awaits Raw Input integration).
    /// 相对鼠标模式 v0 近似：抓屏 + 隐藏光标（真·原始输入模式待接 Raw Input）。
    pub(crate) fn set_mouse_relative(&self, enable: bool) {
        self.set_mouse_grabbed(enable);
        self.set_cursor_visible(!enable);
    }

    pub(crate) fn set_cursor_visible(&self, visible: bool) {
        // SAFETY: ShowCursor 是显示计数器；按布尔一次显/隐，状态自管理。
        unsafe {
            let state = &mut *self.state;
            if state.cursor_visible == visible {
                return;
            }
            state.cursor_visible = visible;
            if visible {
                while wm::ShowCursor(1) < 0 {}
            } else {
                while wm::ShowCursor(0) >= 0 {}
            }
        }
    }

    pub(crate) fn is_cursor_visible(&self) -> bool {
        // SAFETY: state is valid for the window's lifetime. / state 在窗口存活期内有效。
        unsafe { (*self.state).cursor_visible }
    }

    pub(crate) fn set_visible(&self, visible: bool) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::ShowWindow(self.hwnd, if visible { wm::SW_SHOW } else { wm::SW_HIDE });
        }
    }

    pub(crate) fn is_visible(&self) -> bool {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe { wm::IsWindowVisible(self.hwnd) != 0 }
    }

    pub(crate) fn set_always_on_top(&self, top: bool) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::SetWindowPos(
                self.hwnd,
                if top {
                    wm::HWND_TOPMOST
                } else {
                    wm::HWND_NOTOPMOST
                },
                0,
                0,
                0,
                0,
                wm::SWP_NOMOVE | wm::SWP_NOSIZE | wm::SWP_NOACTIVATE,
            );
        }
    }

    pub(crate) fn is_always_on_top(&self) -> bool {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            let ex = wm::GetWindowLongW(self.hwnd, wm::GWL_EXSTYLE) as u32;
            (ex & wm::WS_EX_TOPMOST) != 0
        }
    }

    pub(crate) fn minimize(&self) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::ShowWindow(self.hwnd, wm::SW_MINIMIZE);
        }
    }

    pub(crate) fn maximize(&self) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::ShowWindow(self.hwnd, wm::SW_MAXIMIZE);
        }
    }

    pub(crate) fn restore(&self) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::ShowWindow(self.hwnd, wm::SW_RESTORE);
        }
    }

    pub(crate) fn raise(&self) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::SetWindowPos(
                self.hwnd,
                wm::HWND_TOP,
                0,
                0,
                0,
                0,
                wm::SWP_NOMOVE | wm::SWP_NOSIZE | wm::SWP_NOACTIVATE,
            );
            wm::SetForegroundWindow(self.hwnd);
        }
    }

    pub(crate) fn is_minimized(&self) -> bool {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe { wm::IsIconic(self.hwnd) != 0 }
    }

    pub(crate) fn is_maximized(&self) -> bool {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe { wm::IsZoomed(self.hwnd) != 0 }
    }

    pub(crate) fn has_input_focus(&self) -> bool {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe { wm::GetForegroundWindow() == self.hwnd }
    }

    /// Occlusion query: no direct Win32 equivalent, always false (same
    /// gap as SDL; documented).
    /// 遮挡查询：Win32 无直接等价，恒 false（SDL 同款差异，文档注明）。
    pub(crate) fn is_occluded(&self) -> bool {
        false
    }

    /// Keyboard grab: no window-level Win32 equivalent (SetFocus is a
    /// thread-scoped concept), always a no-op.
    /// 键盘抓取：Win32 无窗口级等价（SetFocus 为线程内概念），恒 no-op。
    pub(crate) fn set_keyboard_grabbed(&self, _grab: bool) {}

    pub(crate) fn position(&self) -> (i32, i32) {
        // SAFETY: hwnd 有效；GetWindowRect = 外框左上（屏幕坐标）。
        unsafe {
            let mut rect = mem::zeroed();
            wm::GetWindowRect(self.hwnd, &mut rect);
            (rect.left, rect.top)
        }
    }

    pub(crate) fn set_position(&self, x: i32, y: i32) {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe {
            wm::SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                x,
                y,
                0,
                0,
                wm::SWP_NOSIZE | wm::SWP_NOZORDER | wm::SWP_NOACTIVATE,
            );
        }
    }

    /// Centers the window on its monitor's work area.
    /// 居中于所在显示器工作区。
    pub(crate) fn center_on_screen(&self) {
        // SAFETY: hwnd 有效；MonitorFromWindow/GetMonitorInfoW 均为查询。
        unsafe {
            let mut rect = mem::zeroed();
            wm::GetWindowRect(self.hwnd, &mut rect);
            let win_w = rect.right - rect.left;
            let win_h = rect.bottom - rect.top;
            let monitor = gdi::MonitorFromWindow(self.hwnd, gdi::MONITOR_DEFAULTTONEAREST);
            let mut mi = gdi::MONITORINFO {
                cbSize: mem::size_of::<gdi::MONITORINFO>() as u32,
                ..mem::zeroed()
            };
            if gdi::GetMonitorInfoW(monitor, &mut mi) != 0 {
                let rc = &mi.rcWork;
                let x = rc.left + (rc.right - rc.left - win_w) / 2;
                let y = rc.top + (rc.bottom - rc.top - win_h) / 2;
                wm::SetWindowPos(
                    self.hwnd,
                    ptr::null_mut(),
                    x,
                    y,
                    0,
                    0,
                    wm::SWP_NOSIZE | wm::SWP_NOZORDER | wm::SWP_NOACTIVATE,
                );
            }
        }
    }

    pub(crate) fn title(&self) -> String {
        // SAFETY: 先查长度（含 NUL），再取文本。
        unsafe {
            let len = wm::GetWindowTextLengthW(self.hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            wm::GetWindowTextW(self.hwnd, buf.as_mut_ptr(), len + 1);
            String::from_utf16_lossy(&buf[..len as usize])
        }
    }

    pub(crate) fn set_opacity(&self, opacity: f32) {
        // SAFETY: hwnd 有效；透明度 <1.0 需 WS_EX_LAYERED + 分层属性。
        unsafe {
            // 透明创建窗（WS_EX_NOREDIRECTIONBITMAP）不适用 LAYERED——
            // 分层路径与 DWM 直通合成冲突（会打断逐像素透明）。透明窗的
            // 整体透明度 v1 忽略（绘制侧 set_alpha 调制承载）。
            let ex = wm::GetWindowLongW(self.hwnd, wm::GWL_EXSTYLE) as u32;
            if ex & wm::WS_EX_NOREDIRECTIONBITMAP != 0 {
                return;
            }
            let state = &mut *self.state;
            state.opacity = opacity.clamp(0.0, 1.0);
            if state.opacity < 1.0 {
                wm::SetWindowLongW(self.hwnd, wm::GWL_EXSTYLE, (ex | wm::WS_EX_LAYERED) as i32);
                let alpha = (state.opacity * 255.0) as u8;
                wm::SetLayeredWindowAttributes(self.hwnd, 0, alpha, wm::LWA_ALPHA);
            } else {
                wm::SetLayeredWindowAttributes(self.hwnd, 0, 255, wm::LWA_ALPHA);
                wm::SetWindowLongW(self.hwnd, wm::GWL_EXSTYLE, (ex & !wm::WS_EX_LAYERED) as i32);
            }
        }
    }

    pub(crate) fn opacity(&self) -> f32 {
        // SAFETY: state is valid for the window's lifetime. / state 在窗口存活期内有效。
        unsafe { (*self.state).opacity }
    }

    /// DPI scale factor (follows the current monitor under Per-Monitor V2
    /// awareness).
    /// DPI 缩放比（Per-Monitor V2 感知下跟随所在显示器）。
    pub(crate) fn dpi_scale(&self) -> f32 {
        // SAFETY: hwnd is valid for the window's lifetime. / hwnd 在窗口存活期内有效。
        unsafe { hid::GetDpiForWindow(self.hwnd) as f32 / 96.0 }
    }

    /// High pixel density: the process declares Per-Monitor V2, so always
    /// true (same flag semantics as SDL).
    /// 高像素密度：进程声明 Per-Monitor V2，恒 true（SDL 同款 flag 语义）。
    pub(crate) fn high_pixel_density(&self) -> bool {
        true
    }

    pub(crate) fn set_size(&self, width: u32, height: u32) {
        // SAFETY: hwnd 有效；客户区尺寸 → 外框尺寸后 SetWindowPos。
        unsafe {
            let style = wm::GetWindowLongW(self.hwnd, wm::GWL_STYLE) as u32;
            let ex_style = wm::GetWindowLongW(self.hwnd, wm::GWL_EXSTYLE) as u32;
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            wm::AdjustWindowRectEx(&mut rect, style, 0, ex_style);
            let (x, y) = self.position();
            wm::SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                x,
                y,
                rect.right - rect.left,
                rect.bottom - rect.top,
                0,
            );
        }
    }
}

// SAFETY: `Sync` exists only to satisfy renderer bounds (wgpu's
// `create_surface(&window)` and friends require shared references); they
// only read handle values and never dereference window data. The methods
// of this type remain bound to the creating thread (see `Window` docs) —
// cross-thread calls violate the contract.
// SAFETY: `Sync` 仅为满足渲染后端的 bound（wgpu `create_surface(&window)`
// 等要求共享引用）；它们只读取句柄值，不解引用窗口数据。本类型的
// 方法仍受创建线程约束（见 `Window` 文档），跨线程调用属违反契约。
unsafe impl Sync for WindowInner {}

impl Drop for WindowInner {
    fn drop(&mut self) {
        // SAFETY: hwnd / state are valid for the window's lifetime; destroy
        // the window first, then reclaim the state block.
        // SAFETY: hwnd / state 均在窗口存活期内；先销毁窗口，再回收状态块。
        unsafe {
            wm::DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.state));
        }
    }
}

impl WindowInner {
    unsafe fn create(options: &WindowOptions) -> Result<Self, WindowError> {
        // SAFETY: see the comments at each FFI call.
        // SAFETY: 见各 FFI 调用处注释。
        unsafe {
            // Keep coordinates in physical pixels (Per-Monitor V2); failure
            // usually means it was declared already — ignore.
            // 让坐标停留在物理像素（Per-Monitor V2）；失败多半是已声明过，忽略。
            let _ =
                hid::SetProcessDpiAwarenessContext(hid::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

            let hinstance = lib::GetModuleHandleW(ptr::null());
            if hinstance.is_null() {
                return Err(WindowError::OsError("GetModuleHandleW failed".into()));
            }
            Self::register_class_once(hinstance)?;

            let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(once(0)).collect();
            let title_w: Vec<u16> = options.title.encode_utf16().chain(once(0)).collect();
            let (width, height) = options.size;

            // SDL flags → Win32 style bits: resizable = THICKFRAME +
            // MAXIMIZEBOX; borderless = WS_POPUP (keep THICKFRAME for a
            // resizable borderless edge).
            // SDL flag → Win32 样式位：resizable = THICKFRAME + MAXIMIZEBOX，
            // borderless = WS_POPUP（可拖边时保留 THICKFRAME）。
            let mut style = wm::WS_OVERLAPPEDWINDOW;
            if !options.resizable {
                style &= !(wm::WS_THICKFRAME | wm::WS_MAXIMIZEBOX);
            }
            if options.borderless {
                style = wm::WS_POPUP
                    | if options.resizable {
                        wm::WS_THICKFRAME
                    } else {
                        0
                    };
            }

            let state = Box::into_raw(Box::new(WndState {
                hwnd: ptr::null_mut(),
                hinstance,
                size: (width, height),
                min_size: options.min_size,
                max_size: options.max_size,
                pending: VecDeque::new(),
                mouse_down: 0,
                fullscreen: false,
                saved_placement: None,
                cursor_visible: true,
                mouse_grabbed: false,
                opacity: 1.0,
            }));

            // Client size → outer frame size, so the client area exactly
            // equals the requested value.
            // 客户区 size → 外框尺寸，使客户区精确等于请求值。
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            if wm::AdjustWindowRectEx(&mut rect, style, 0, 0) == 0 {
                drop(Box::from_raw(state));
                return Err(WindowError::OsError(format!(
                    "AdjustWindowRectEx failed: {}",
                    GetLastError()
                )));
            }

            // lpCreateParams carries the state pointer; wndproc attaches it
            // on WM_NCCREATE.
            // lpCreateParams 携带 state 指针，WM_NCCREATE 时由 wndproc 挂上窗口。
            // TRANSPARENT：WS_EX_NOREDIRECTIONBITMAP——去掉 GDI 重定向位图，
            // DWM 直接合成交换链内容（交换链 alpha 预乘 → 逐像素透明，
            // 桌宠/悬浮件形态。GDI 绘制在此类窗口不可用——纯 wgpu 渲染无碍）
            let mut ex_style = 0u32;
            if options.transparent {
                ex_style |= wm::WS_EX_NOREDIRECTIONBITMAP;
            }
            let hwnd = wm::CreateWindowExW(
                ex_style,
                class_name.as_ptr(),
                title_w.as_ptr(),
                style,
                wm::CW_USEDEFAULT,
                wm::CW_USEDEFAULT,
                rect.right - rect.left,
                rect.bottom - rect.top,
                ptr::null_mut(),
                ptr::null_mut(),
                hinstance,
                state.cast(),
            );
            if hwnd.is_null() {
                drop(Box::from_raw(state));
                return Err(WindowError::OsError(format!(
                    "CreateWindowExW failed: {}",
                    GetLastError()
                )));
            }

            if options.always_on_top {
                // SAFETY: hwnd is valid; SWP_NOMOVE | SWP_NOSIZE make the
                // remaining parameters ignored.
                // SAFETY: hwnd 有效；SWP_NOMOVE | SWP_NOSIZE 使其余参数被忽略。
                wm::SetWindowPos(
                    hwnd,
                    wm::HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    wm::SWP_NOMOVE | wm::SWP_NOSIZE | wm::SWP_NOACTIVATE,
                );
            }

            // HIDDEN flag: created but not shown yet (shown later via
            // set_visible(true)).
            // HIDDEN flag：创建但先不显示（运行期 set_visible(true) 再上屏）。
            if !options.hidden {
                wm::ShowWindow(hwnd, wm::SW_SHOW);
            }
            let this = Self { hwnd, state };

            // Apply post-create flags (the runtime method family works
            // directly on the creating thread).
            // post-create flag 应用（运行期方法族在创建线程上直接可用）
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
    }

    unsafe fn register_class_once(hinstance: HMODULE) -> Result<(), WindowError> {
        // SAFETY: all RegisterClassExW parameters are valid static /
        // process-lifetime data.
        // SAFETY: RegisterClassExW 参数均为有效静态/进程级数据。
        unsafe {
            static REGISTERED: AtomicBool = AtomicBool::new(false);
            if REGISTERED.swap(true, Ordering::Relaxed) {
                return Ok(());
            }
            let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(once(0)).collect();
            let wc = wm::WNDCLASSEXW {
                cbSize: mem::size_of::<wm::WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance,
                hIcon: ptr::null_mut(),
                hCursor: ptr::null_mut(),
                // No background erasing: rendering is the user's job (wgpu
                // surface); avoids flicker at creation.
                // 不擦背景：渲染由用户负责（wgpu surface），避免建窗闪烁。
                hbrBackground: ptr::null_mut(),
                lpszMenuName: ptr::null(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: ptr::null_mut(),
            };
            if wm::RegisterClassExW(&wc) == 0 {
                REGISTERED.store(false, Ordering::Relaxed);
                return Err(WindowError::OsError(format!(
                    "RegisterClassExW failed: {}",
                    GetLastError()
                )));
            }
            Ok(())
        }
    }
}

/// The window procedure: state attachment + size cache + event translation
/// (see [`translate`]) + system-default fallback. Same thread as
/// `poll_event`.
/// 窗口过程：状态挂载 + 尺寸缓存 + 事件翻译（见 [`translate`]）+ 系统默认兜底。
/// 与 `poll_event` 同线程。
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: every FFI call's parameters come from the system callback
    // itself; hwnd is valid for the duration of the callback.
    // SAFETY: 所有 FFI 调用的参数均来自系统回调本身，hwnd 在回调期内有效。
    unsafe {
        if msg == wm::WM_NCCREATE {
            // CREATESTRUCTW.lpCreateParams is the state pointer we passed
            // in; attach it to the window.
            // CREATESTRUCTW.lpCreateParams 是我们传入的 state 指针，挂到窗口上。
            let cs = &*(lparam as *const wm::CREATESTRUCTW);
            let state = cs.lpCreateParams as *mut WndState;
            wm::SetWindowLongPtrW(hwnd, wm::GWLP_USERDATA, state as isize);
            (*state).hwnd = hwnd;
            return wm::DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = wm::GetWindowLongPtrW(hwnd, wm::GWLP_USERDATA) as *mut WndState;
        if state.is_null() {
            // Messages before NCCREATE (e.g. WM_GETMINMAXINFO): no state
            // available yet.
            // NCCREATE 之前的消息（如 WM_GETMINMAXINFO）：尚无状态可用。
            return wm::DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = &mut *state;

        // The size cache is always maintained (size() must be correct even
        // in window-only builds).
        // 尺寸缓存始终维护（仅 window、无事件时 size() 也要正确）。
        if msg == wm::WM_SIZE {
            cache_size(state, wparam, lparam);
        }

        // Event translation: Some = handled, return the result to the
        // system directly.
        // 事件翻译：返回 Some 即已处理，直接把结果交还系统。
        if let Some(result) = translate::handle_message(state, hwnd, msg, wparam, lparam) {
            return result;
        }

        match msg {
            // Destruction is decided by the user dropping the Window; only
            // the system default destruction is intercepted here.
            // 是否销毁由用户 drop Window 决定；这里只拦下系统默认销毁。
            wm::WM_CLOSE => 0,
            // Size-range constraints for user-driven resizes (builder's
            // min_size / max_size).
            // 用户拖拽调整的尺寸范围约束（builder 的 min_size / max_size）。
            wm::WM_GETMINMAXINFO => {
                let mmi = &mut *(lparam as *mut wm::MINMAXINFO);
                if let Some((w, h)) = state.min_size {
                    mmi.ptMinTrackSize = POINT {
                        x: w as i32,
                        y: h as i32,
                    };
                }
                if let Some((w, h)) = state.max_size {
                    mmi.ptMaxTrackSize = POINT {
                        x: w as i32,
                        y: h as i32,
                    };
                }
                0
            }
            _ => wm::DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

/// Extracts the client size from `WM_SIZE`'s lParam into the cache (not
/// updated while minimized).
/// 从 `WM_SIZE` 的 lParam 提取客户区尺寸写入缓存（最小化时不更新）。
fn cache_size(state: &mut WndState, wparam: WPARAM, lparam: LPARAM) {
    let width = (lparam & 0xFFFF) as u16 as u32;
    let height = ((lparam >> 16) & 0xFFFF) as u16 as u32;
    if wparam as u32 != wm::SIZE_MINIMIZED {
        state.size = (width, height);
    }
}
