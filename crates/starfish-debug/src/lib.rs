//! starfish-debug —— dev 诊断枢纽（两大模块，**创建时一次性**定约）。
//!
//! - [`display`]：呈现形态模拟（letterbox 钉定 / 全屏形态）——窗口创建
//!   时读取，会话期不变；
//! - [`log`]：统一日志（文件 / wasm 远程 / 直出三 sink + 级别 + 第三方
//!   log crate 桥接捕获 wgpu/egui/rapier 等外部输出）。
//!
//! 定约：宿主/dev 工具在**程序创建前**调用 set_*/init；引擎读取一次，
//! 会话期内不变。无任何运行时变更接口（运行时改尺寸有呈现态竞态实测
//! 坑），dev 换形态 = 重启换参。
//!
//! 生产面：纯配置槽 + 诊断管道——无事件监听、无常驻线程；随包发布
//! 无害，不调用 = 引擎零行为差异（log 宏未初始化自动退化控制台输出，
//! 不丢消息）。

pub mod display;
pub mod log;

// 形态模拟双接口根路径重导（引擎读侧简写；模块路径仍可用）
pub use display::{fullscreen_sim, letterbox_sim, set_fullscreen_sim, set_letterbox_sim};
