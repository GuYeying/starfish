//! pygame.permission 对应物（**starfish 扩展域**：真 pygame 无 permission
//! 模块——运行时权限申请的统一入口，包装 [`crate::base::permission`]）。
//!
//! 平台语义（详见 base::permission 模块文档与 `关于权限.md`）：
//! - **Android**：dangerous 权限（如麦克风）弹系统授权框，**阻塞等待**
//!   至用户操作或 ~15s 超时；normal 权限（如网络）瞬时通过
//! - **Web**：同步 [`ensure`] 为委派语义（恒 true，授权发生在设备 API
//!   首次使用时）；[`ensure_async`] 对麦克风为真实授权
//!   （getUserMedia，授权流缓存待录音复用）
//! - **桌面**：无运行时权限模型，恒 true
//!
//! 约定：引擎模块（录音/网络）在自己的初始化路径上**隐式申请**——应用
//! 通常无需调用本模块；显式调用用于"提前授权"（避免首次使用时的弹框
//! 打断）。

pub use crate::base::permission::Permission;

/// 运行时权限申请（同步；Android 阻塞等待 / 桌面 Web 直通语义）
pub use crate::base::permission::ensure;

/// 异步统一授权入口（Web = 真实授权；其余平台桥接同步版）
pub use crate::base::permission::ensure_async;

/// 消费缓存的已授权媒体流（Web 麦克风授权成功后非空；录音复用）
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use crate::base::permission::take_cached_stream;
