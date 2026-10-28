//! Android 嵌入模式实现（[`crate::embedded`] 门面的后端）：外部注入的
//! Surface 状态机 + ANativeWindow JNI 桥 + 触摸/尺寸事件合成。
//!
//! 职责边界（与 [`super`]（glue 窗口后端）的切分）：本模块只管"外部
//! 注入窗口"的状态与桥接；glue（NativeActivity）路径的窗口/事件/生命
//! 周期仍归 [`super`]。两路径的汇合点：[`super::PENDING`] 事件队列
//! （嵌入注入与 glue 翻译共用）与 [`crate::window`] 单窗槽位。
//!
//! 移动端嵌入矩阵（门面文档同步）：
//! - Android ✓ 本模块（ANCO 上的鸿蒙经此形态运行）；
//! - iOS：预留——窗口对象/生命周期不同（CAMetalLayer/UIView），接入时
//!   新增平台后端，不复用 JNI 签名；
//! - HarmonyOS 原生：预留——XComponent 后端（当前 ANCO 经 Android）。

use std::sync::Mutex;

use super::PENDING;
use crate::event::{Event, MouseButton};
use crate::window::reset_single_window_slot;

/// 外部注入窗口：ANativeWindow 引用（fromSurface 获得 _acquire，detach
/// 时 release）+ 位图尺寸。
pub(super) struct ExternalWindow {
    pub(super) ptr: *mut std::ffi::c_void,
    pub(super) width: u32,
    pub(super) height: u32,
}

/// 当前注入窗口槽位（None = 未 attach；glue 路径不经此）。
/// 访问方：[`super`]（Window size/raw_handle/poll_event 短路）。
pub(crate) static EXTERNAL: Mutex<Option<ExternalWindow>> = Mutex::new(None);

// ANativeWindow 引用跨线程传递安全（Android 图形句柄线程无关，
// 释放走 detach 后的 pending-release 调用）
unsafe impl Send for ExternalWindow {}

/// 嵌入模式是否激活（glue 路径的短路开关）
pub fn external_active() -> bool {
    EXTERNAL.lock().unwrap_or_else(|p| p.into_inner()).is_some()
}

/// 嵌入宿主注入外部窗口（`ANativeWindow_fromSurface` 的返回引用；
/// 引用计数已 +1，detach 后由壳收尾经 [`release_pending_external`]
/// 归还）。
///
/// **attach 即新场开演**（进程一次契约下，每次 attach = 一轮新运行）：
/// 先清上一场残留事件（detach 落下的 CloseRequested / 旧 Resized / 触摸
/// ——否则新场首泵吃到旧场 CloseRequested 立即收口，"第二次运行黑屏"
/// 真机实证），再释放单窗槽位（窗口生命周期按"运行"而非"进程"计，
/// 镜像 [`super::android_init`] 的每场重置）。
///
/// # Safety
/// `ptr` 必须是有效的 `ANativeWindow*`（fromSurface 返回值）。
pub unsafe fn attach_external_window(ptr: *mut std::ffi::c_void, width: u32, height: u32) {
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

/// 嵌入宿主解除窗口（SurfaceView 销毁；无条件清槽 + push
/// CloseRequested → 引擎帧循环自然收口，与 NativeActivity 的 Destroy
/// 语义一致）。陈旧 detach（换场重叠期旧场销毁晚于新场 attach）由
/// **Java 层代数守卫**拦截——ANCO 实证：fromSurface 的指针比对不可靠
/// （同 Surface 未必返回同一 ANativeWindow，比对失败 = 合法 detach 被
/// 误忽略 = 引擎永不收口，声音不停 + 新场黑屏）。
///
/// # Safety
/// `env`/`surface` 必须是有效的 JNI 环境/Surface 句柄。
pub unsafe fn detach_external_window(env: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) {
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

/// 嵌入宿主注入触摸（主手指 → 鼠标事件合成，SDL 惯例——与 glue 输入
/// 翻译同语义）。action：0 = down，1 = move，2 = up。坐标 = 位图像素。
pub fn inject_external_touch(action: u8, x: f64, y: f64) {
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

/// 嵌入宿主的尺寸变化事件（Resized 事件入队）
pub fn inject_external_resized(width: u32, height: u32) {
    PENDING
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push_back(Event::Resized { width, height });
}

// ── JNI 桥（libandroid.so 原生符号直连）────────────────────────

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

/// [`crate::embedded::detach_surface`] 的委托目标：无条件清槽 + push
/// CloseRequested（引擎帧循环收口）。ANativeWindow 引用的物理释放由壳
/// 在引擎收口后回调 [`release_pending_external`]（防 present-after-
/// release OOB）。
///
/// # Safety
/// `env`/`surface` 必须是有效的 JNI 环境/Surface 句柄。
pub(crate) unsafe fn detach_surface_impl(
    env: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
) {
    unsafe { detach_external_window(env, surface) };
}

/// 嵌入窗口当前尺寸（None = 未 attach）。宿主壳建窗前的尺寸自查用
///（引擎表面初始尺寸跟随原生窗口实测值——满屏 1:1 的确定性来源）
pub(crate) fn external_size_impl() -> Option<(u32, u32)> {
    let slot = EXTERNAL.lock().unwrap_or_else(|p| p.into_inner());
    let ext = slot.as_ref()?;
    Some((ext.width, ext.height))
}

/// [`crate::embedded::inject_touch`] 的委托目标（action：0 down / 1 move / 2 up）
pub(crate) fn inject_touch_impl(action: u8, x: f64, y: f64) {
    inject_external_touch(action, x, y);
}
