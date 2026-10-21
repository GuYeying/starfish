//! pygame.io 对应物（**starfish 扩展域**：真 pygame 无 io 模块——游戏
//! 数据读写的统一入口，包装 [`starfish_io`]）。
//!
//! 路径语义：相对路径锚定 [`base_dir`](starfish_io::base_dir)
//! （桌面 = 可执行文件旁 / Web = POST 后端锚点 / Android = files 目录），
//! 三平台行为一致。
//!
//! 异步说明：与 [`crate::display::set_mode`] 同款"教程差异"——
//! Web 的 fetch 模型要求 await；桌面/Android 虽为同步实现，签名统一
//! async（应用体本就是 async，零额外成本）。

pub use starfish_io::{
    exists, read, read_text, write, write_text, IoError,
};

/// 数据基目录（相对路径锚点；未设置 = None）
/// 原生平台专用（Web 无基目录概念——写走 POST 后端）。
#[cfg(not(target_arch = "wasm32"))]
pub use starfish_io::{base_dir, set_base_dir};
