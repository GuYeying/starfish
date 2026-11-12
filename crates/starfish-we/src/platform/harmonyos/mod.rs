//! HarmonyOS (OHOS) stub backend — **dispatch carve-out**.
//! HarmonyOS（OHOS）占位后端——**分流桩**。
//!
//! rustc metadata for `aarch64-unknown-linux-ohos` (empirically verified
//! 2026-11-11 via `rustc --print cfg`): `target_os = "linux"` +
//! `target_env = "ohos"`. Without this carve-out an OHOS build would
//! **silently take the linux-Wayland backend** (OHOS has no Wayland
//! compositor) — this module removes OHOS from the linux path explicitly
//! and stubs the whole `WindowInner` contract with explicit `Err`s
//! (站桩定约: never silent, always a message pointing at the path
//! forward).
//! rustc 对 `aarch64-unknown-linux-ohos` 的元数据（2026-11-11
//! `rustc --print cfg` 实测）：`target_os = "linux"` + `target_env =
//! "ohos"`——不分流时 OHOS 构建会**静默走 linux-Wayland 后端**（OHOS
//! 无 Wayland 合成器）。本模块把 OHOS 从 linux 路径显式摘出，整份
//! `WindowInner` 契约以显式 `Err` 站桩（站桩定约：绝不静默，文案指向
//! 后续路径）。
//!
//! Extension points for the real adapter (真适配层接入位) — see
//! `pygamest reference/新平台扩展指南（macOS-iOS-HarmonyOS）.md`:
//! - window/surface: `OH_NativeXComponent` + `OH_NativeWindow` →
//!   a new `RawWindowHandle::Ohos` variant (needs the full-match
//!   updates in `handle.rs` consumers + wgpu mapping) → wgpu GL/EGL
//!   backend (vendor patch)
//! - input: `NativeXComponent` callbacks → buffered queue drained by
//!   `poll_event` (same inversion as the android backend)
//! - window type: `WindowInner` gains real state; `new()` starts
//!   constructing (this stub's `WindowInner` is deliberately
//!   uninhabited — the type system itself encodes "no window can exist
//!   on OHOS yet").
//! - 窗口/表面：`OH_NativeXComponent` + `OH_NativeWindow` → 新增
//!   `RawWindowHandle::Ohos` 变体（连带 `handle.rs` 消费方全 match
//!   更新 + wgpu 映射）→ wgpu GL/EGL 后端（vendor patch）
//! - 输入：`NativeXComponent` 回调 → 缓冲队列 → `poll_event` 排空
//!   （与 android 后端同一反转模型）
//! - 窗口类型：`WindowInner` 获得真实状态，`new()` 开始构造（本桩的
//!   `WindowInner` 刻意不可构造——类型系统自身编码"OHOS 尚无窗口"）。

use crate::error::WindowError;
use crate::event::Event;
use crate::handle::RawWindowHandle;
use crate::window::WindowOptions;

/// OHOS stub window handle — deliberately uninhabited (`new` always
/// errs, so no window can ever exist; every `&self` method is compiled
/// as unreachable via `match`).
/// OHOS 站桩窗口句柄——刻意不可构造（`new` 恒 Err，窗口永不存活；
/// 全部 `&self` 方法经 `match` 编码为不可达）。
pub(crate) enum WindowInner {}

/// Stub error phrasing (显式 Err 定约：API 名 + 待建面 + 接入路径).
fn stub_err(api: &str) -> WindowError {
    WindowError::OsError(format!(
        "{api}: HarmonyOS 站桩预留——适配层待建（OH_NativeXComponent 窗口/\
         事件 + wgpu GL/EGL + OHAudio + AVPlayer），接入位见 pygamest \
         reference/新平台扩展指南（macOS-iOS-HarmonyOS）"
    ))
}

impl WindowInner {
    pub(crate) fn new(_options: &WindowOptions) -> Result<Self, WindowError> {
        Err(stub_err("new"))
    }

    pub(crate) fn poll_event(&self) -> Option<Event> {
        match *self {}
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        match *self {}
    }

    pub(crate) fn set_title(&self, _title: &str) {
        match *self {}
    }

    pub(crate) fn title(&self) -> String {
        match *self {}
    }

    pub(crate) fn raw_handle(&self) -> RawWindowHandle {
        match *self {}
    }

    pub(crate) fn set_resizable(&self, _resizable: bool) {
        match *self {}
    }

    pub(crate) fn is_resizable(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_size(&self, _width: u32, _height: u32) {
        match *self {}
    }

    pub(crate) fn set_fullscreen(&self, _fullscreen: bool) {
        match *self {}
    }

    pub(crate) fn is_fullscreen(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_borderless(&self, _borderless: bool) {
        match *self {}
    }

    pub(crate) fn is_borderless(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_mouse_grabbed(&self, _grabbed: bool) {
        match *self {}
    }

    pub(crate) fn is_mouse_grabbed(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_mouse_relative(&self, _enable: bool) {
        match *self {}
    }

    pub(crate) fn set_cursor_visible(&self, _visible: bool) {
        match *self {}
    }

    pub(crate) fn is_cursor_visible(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_visible(&self, _visible: bool) {
        match *self {}
    }

    pub(crate) fn is_visible(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_always_on_top(&self, _top: bool) {
        match *self {}
    }

    pub(crate) fn is_always_on_top(&self) -> bool {
        match *self {}
    }

    /// Click-through (feature "click-through" — mirrors the linux gate).
    /// 点击穿透（feature "click-through"——与 linux 门控镜像）。
    #[cfg(feature = "click-through")]
    pub(crate) fn set_click_through(&self, _enable: bool) -> Result<(), WindowError> {
        match *self {}
    }

    #[cfg(feature = "click-through")]
    pub(crate) fn is_click_through(&self) -> bool {
        match *self {}
    }

    /// Taskbar hiding (feature "skip-taskbar" — mirrors the linux gate).
    /// 任务栏隐藏（feature "skip-taskbar"——与 linux 门控镜像）。
    #[cfg(feature = "skip-taskbar")]
    pub(crate) fn set_skip_taskbar(&self, _enable: bool) -> Result<(), WindowError> {
        match *self {}
    }

    #[cfg(feature = "skip-taskbar")]
    pub(crate) fn is_skip_taskbar(&self) -> bool {
        match *self {}
    }

    /// System tray (feature "tray" — mirrors the linux gate).
    /// 系统托盘（feature "tray"——与 linux 门控镜像）。
    #[cfg(feature = "tray")]
    pub(crate) fn set_tray(&self, _spec: crate::tray::TraySpec) -> Result<(), WindowError> {
        match *self {}
    }

    #[cfg(feature = "tray")]
    pub(crate) fn remove_tray(&self) -> Result<(), WindowError> {
        match *self {}
    }

    pub(crate) fn minimize(&self) {
        match *self {}
    }

    pub(crate) fn maximize(&self) {
        match *self {}
    }

    pub(crate) fn restore(&self) {
        match *self {}
    }

    pub(crate) fn raise(&self) {
        match *self {}
    }

    pub(crate) fn is_minimized(&self) -> bool {
        match *self {}
    }

    pub(crate) fn is_maximized(&self) -> bool {
        match *self {}
    }

    pub(crate) fn has_input_focus(&self) -> bool {
        match *self {}
    }

    pub(crate) fn is_occluded(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_keyboard_grabbed(&self, _grab: bool) {
        match *self {}
    }

    pub(crate) fn set_text_input_visible(&self, _visible: bool) -> bool {
        match *self {}
    }

    pub(crate) fn is_text_input_visible(&self) -> bool {
        match *self {}
    }

    pub(crate) fn set_position(&self, _x: i32, _y: i32) {
        match *self {}
    }

    pub(crate) fn position(&self) -> (i32, i32) {
        match *self {}
    }

    pub(crate) fn center_on_screen(&self) {
        match *self {}
    }

    /// set_orientation：OHOS 站桩（XComponent 后端建成后接窗口方向）
    pub(crate) fn set_orientation(&self, _orientation: &str) -> Result<(), crate::error::WindowError> {
        Err(crate::error::WindowError::OsError(
            "set_orientation: HarmonyOS 适配层待建（XComponent 后端）".into(),
        ))
    }

    pub(crate) fn set_opacity(&self, _opacity: f32) {
        match *self {}
    }

    pub(crate) fn opacity(&self) -> f32 {
        match *self {}
    }

    pub(crate) fn dpi_scale(&self) -> f32 {
        match *self {}
    }

    pub(crate) fn high_pixel_density(&self) -> bool {
        match *self {}
    }
}
