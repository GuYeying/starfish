//! 平台驱动器垫片：[`crate::app_entry!`] 宏的基础设施。
//!
//! 宏在**用户 crate** 展开，宏体内的驱动器路径必须是 `$crate::` 形态——
//! 用户 crate 并不依赖 pollster / wasm-bindgen-futures，因此驱动器由本
//! 模块以星曲自身依赖再导出（平台差异在本模块 cfg 收敛，宏体零 cfg 分
//! 歧路径全走 `$crate::base::app::…`）。
//!
//! 与 [`super::window::next_frame`] 的分工：`next_frame` 是**应用体里**
//! 的帧拍原语（Web rAF yield）；本模块是**入口处**的顶层驱动器
//! （怎么把 async 应用体跑到该平台上）。

// 桌面/Android：阻塞驱动（wasm 无阻塞模型，不参与编译）。
#[cfg(not(target_arch = "wasm32"))]
pub use pollster::block_on;

// Web：非阻塞调度（LocalSet 语义，不要求 Future: Send——
// 持 Window/canvas 等 !Send 值跨 await 合法）。
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub use wasm_bindgen_futures::spawn_local;
