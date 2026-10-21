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
