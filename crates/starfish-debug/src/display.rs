//! 呈现形态模拟（**创建时一次性**——窗口创建时读取，会话期不变）。
//!
//! 两形态**互斥**（一个设备同一时刻只有一种）：set 时互清、最后调用
//! 者生效——引擎读侧永远最多命中一个。能力仅桌面语义（wasm canvas
//! 形态由页面控制、移动端非 dev 目标平台）。

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static LETTERBOX_SIM: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static FULLSCREEN_SIM: AtomicBool = AtomicBool::new(false);

/// 窗口化形态模拟：物理钉定（逻辑画布恒 = 脚本 set_mode 请求，引擎
/// 等比居中映射——Android 窗口化行为在本机复现）。
/// 环境变量 `STARFISH_DESKTOP_LETTERBOX=宽x高` 为等价形式（显式 set
/// 优先）。**互斥**：清除全屏形态模拟。
pub fn set_letterbox_sim(width: u32, height: u32) {
    FULLSCREEN_SIM.store(false, Ordering::SeqCst);
    *LETTERBOX_SIM.lock().unwrap() = Some((width, height));
}

/// 引擎窗口创建时读取（显式 set 优先，环境变量垫底；引擎只读一次）
pub fn letterbox_sim() -> Option<(u32, u32)> {
    if let Some(v) = *LETTERBOX_SIM.lock().unwrap() {
        return Some(v);
    }
    static ENV: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    *ENV.get_or_init(|| {
        std::env::var("STARFISH_DESKTOP_LETTERBOX").ok().and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
    })
}

/// 全屏形态模拟：置位后脚本 set_mode 一律按 FULLSCREEN 旗标处理
/// （物理 = 显示器实测，逻辑 = 请求尺寸）。
/// **互斥**：清除 letterbox 物理钉定；传 `false` = 仅解除本形态。
pub fn set_fullscreen_sim(on: bool) {
    if on {
        *LETTERBOX_SIM.lock().unwrap() = None;
    }
    FULLSCREEN_SIM.store(on, Ordering::SeqCst);
}

/// 引擎 set_mode 时读取
pub fn fullscreen_sim() -> bool {
    FULLSCREEN_SIM.load(Ordering::SeqCst)
}
