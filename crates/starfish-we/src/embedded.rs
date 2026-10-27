//! 嵌入式宿主门面（**Android 专属门控**，2026-10-26 定约）：pygame-rs-dev
//! 等"嵌入式运行时 .so"导出给原生 Android 开发者的**最小暴露面**——
//!
//! - [`attach_surface`] / [`surface_resized`] / [`detach_surface`]：
//!   宿主 SurfaceView 生命周期回调的转发；
//! - [`inject_touch`]：宿主 View 的触摸事件注入（主手指 → 鼠标合成）。
//!
//! 设计约束（层级纪律）：starfish-we 的正式 API 仍是 [`crate::Window`]
//! （NativeActivity/glue 形态）；本门面只暴露嵌入模式的四个动作，不
//! 重导出窗口内部——嵌入运行时的引擎驱动、载荷分派、io 基准都在
//! 上层（pygame-rs-dev / pack 仓）。
//!
//! 线程：attach/detach 在宿主 UI 线程调用（JNI 回调线程）；引擎在
//! .so 自起线程驱动，帧渲染对本指针的使用由 GPU 驱动线程安全保证。

/// 注入宿主 SurfaceView 的 Surface（attach JNI 回调转发）。
///
/// # Safety
/// - `env`：有效的 JNIEnv 指针（调用线程已 attach JVM）；
/// - `surface`：有效的 `android.view.Surface` jobject。
///
/// 内部 `ANativeWindow_fromSurface` 获得引用；[`detach_surface`] 时释放。
pub unsafe fn attach_surface(
    env: *mut std::ffi::c_void,
    surface: *mut std::ffi::c_void,
) -> (u32, u32) {
    crate::platform::android::attach_surface_impl(env, surface)
}

/// SurfaceView 尺寸变化（changed 回调转发）
pub fn surface_resized(width: u32, height: u32) {
    crate::platform::android::surface_resized_impl(width, height);
}

/// SurfaceView 销毁（destroyed 回调转发；**比对清除**——`surface` 与
/// 当前槽位一致才清槽 + push CloseRequested → 引擎帧循环自然收口，
/// 与 NativeActivity 模式的 Destroy 语义一致；换场竞态里的陈旧
/// detach 被忽略，不连坐新场窗口）。
///
/// # Safety
/// - `env`：有效的 JNIEnv 指针（调用线程已 attach JVM）；
/// - `surface`：本次销毁回调对应的 `android.view.Surface` jobject。
pub unsafe fn detach_surface(env: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) {
    crate::platform::android::detach_surface_impl(env, surface);
}

/// 引擎完全停止后的窗口引用物理释放（壳收尾调用一次；精确释放 detach
/// 暂存的指针——不碰当前窗口，换场重叠期安全）
pub fn release_surface() {
    crate::platform::android::release_pending_external();
}

/// 触摸注入（action：0 = down，1 = move，2 = up；坐标 = 位图像素）
pub fn inject_touch(action: u8, x: f64, y: f64) {
    crate::platform::android::inject_touch_impl(action, x, y);
}
