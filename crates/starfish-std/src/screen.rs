//! 跨平台屏幕/画布尺寸登记表（注册-读取模式，同 [`crate::debug`] 的
//! diag sink 家族）：平台窗口层写入，任意层读取。
//!
//! - 写入方：窗口所属层（starfish-we 嵌入 attach/尺寸变化回填；桌面
//!   窗口事件后续接入）；
//! - 读取方：脚本自适应 set_mode（经 pygame 绑定暴露）、诊断工具、
//!   宿主壳（dev 解释器把尺寸附进 sys.argv 的取数来源）。
//!
//! 未写入时读取 = None（调用方自行回退默认值）。

use std::sync::Mutex;

static SCREEN: Mutex<Option<(u32, u32)>> = Mutex::new(None);

/// 登记当前屏幕/画布尺寸（零值入参按无效忽略）
pub fn set_screen_size(width: u32, height: u32) {
    if width == 0 || height == 0 {
        return;
    }
    if let Ok(mut slot) = SCREEN.lock() {
        *slot = Some((width, height));
    }
}

/// 读取登记的屏幕尺寸（未登记 = None）
pub fn screen_size() -> Option<(u32, u32)> {
    *SCREEN.lock().unwrap_or_else(|p| p.into_inner())
}

/// 清除登记（窗口销毁等场景；可选）
pub fn clear_screen_size() {
    if let Ok(mut slot) = SCREEN.lock() {
        *slot = None;
    }
}
