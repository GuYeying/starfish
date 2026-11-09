//! pygame.dialog 对应物（**starfish 扩展域**：真 pygame 无 dialog 模块——
//! 原生文件选择/保存，包装 [`starfish_dialog]`）。
//!
//! 模式：**非阻塞发起 + 每帧轮询**（`try_result`）——主线程全程保持事件
//! 循环（Android SAF 选择器是独立 Activity，覆盖期间会触发本应用生命
//! 周期流转，阻塞式等待会让事件泵停摆；轮询式从根上消除）。
//!
//! ```ignore
//! // 绑定层注入 pygame 命名空间后，Python 侧：
//! job = pygame.dialog.pick_file_start("选择", [("任意", ["*"])])
//! # 每帧：
//! res = job.try_result()
//! # res = 已选文件 / None=进行中 / 取消
//! ```
//!
//! Web 平台：文件**选择**由页面 `<input type="file">` 驱动（浏览器模型），
//! 本模块仅提供**保存**（下载触发）。

pub use starfish_dialog::{DialogError, PickedFile};

/// 非阻塞发起"保存文件"（内容先行写入；用户确认路径后落盘）
///
/// 原生平台专用（Web 的保存走 `pygame.io.write` + 页面下载策略）。
#[cfg(not(target_arch = "wasm32"))]
pub use starfish_dialog::{save_bytes_start, SaveJob};

/// 非阻塞发起"选择文件"，返回可每帧轮询的任务句柄
///
/// - `title`：对话框标题（平台支持时显示）
/// - `filters`：`(说明, &[扩展名])` 序列，如 `&[("图片", &["png", "jpg"])]`
#[cfg(not(target_arch = "wasm32"))]
pub use starfish_dialog::{pick_file_start, Filter, PickJob};

// ── 文本输入对话框桥（Android IME 组合方案，2026-11-09 定约）──────
//
// 点击输入框 → 宿主弹出**原生 EditText 对话框**（预填当前内容——
// 删除/粘贴/中文全部是系统 EditText 原生能力）→ 完成后整体回填
// （TextDialogDone 事件 → 脚本覆盖自己的输入框内容）。
//
// 职责切分：本模块只持**桥函数槽**（宿主启动期注册一次——桥内做
// JNI 上叫原生对话框）；Python 面 `pygame.key.start_text_dialog`
// 触发请求；结果经 [`crate::event`] 以 TextDialogDone 事件回投。
// 未接桥（桌面/无宿主形态）= 请求返回 false（跨平台脚本安全 no-op）。

type TextDialogBridge = std::sync::Arc<dyn Fn(String) + Send + Sync>;

static TEXT_DIALOG_BRIDGE: std::sync::Mutex<Option<TextDialogBridge>> =
    std::sync::Mutex::new(None);

/// 注册文本对话框桥（宿主启动期调用一次；重复注册 = 覆盖）。
/// 桥闭包由宿主实现：弹出原生 EditText 对话框（预填 `current`），
/// 用户完成后回投 [`text_dialog_done`]。
pub fn set_text_dialog_bridge(f: TextDialogBridge) {
    *TEXT_DIALOG_BRIDGE.lock().unwrap_or_else(|p| p.into_inner()) = Some(f);
}

/// 请求弹出文本对话框（`current` = 预填的当前内容）。返回是否已接桥
/// （false = 当前形态无对话框能力——跨平台脚本可据此跳过）。
pub fn request_text_dialog(current: String) -> bool {
    match TEXT_DIALOG_BRIDGE.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        Some(bridge) => {
            bridge(current);
            true
        }
        None => false,
    }
}

static TEXT_DIALOG_RESULTS: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

/// 对话框完成回投（宿主 JNI 回调调用，**UI 线程任意时点安全**——
/// Mutex 跨线程槽承接；引擎线程 [`crate::event::pump`] 时 drain，
/// 以 TextDialogDone 事件入队。取消 = 宿主不回投）。
pub fn text_dialog_done(text: String) {
    TEXT_DIALOG_RESULTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .push(text);
}

/// 引擎线程 pump 钩子（[`crate::event::pump`] 每帧调用）：取走完成
/// 文本 → TextDialogDone 事件入 USER_PENDING（下一帧 pygame.event 可见）。
pub(crate) fn drain_text_dialog_results() -> Vec<String> {
    TEXT_DIALOG_RESULTS
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .drain(..)
        .collect()
}
