//! 窗口与输入（winit 后端）
//!
//! 公共契约见 [`event`]（平台中立事件模型）；[`Window`] 由引擎创建注入。
//! 循环模型见 [`crate::base::app`]：引擎持循环，应用实现 [`crate::base::app::Application`]。

pub mod event;
mod window;

pub use event::{
    KeyCode, KeyModifiers, KeyboardState, MouseButton, MouseState, WindowEvent,
};
pub use window::Window;
