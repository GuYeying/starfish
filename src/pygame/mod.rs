//! pygame 兼容层（契约见 `architecture/pygame层设计.md`，定稿 v1.0）。
//!
//! 接口形态 1:1 对齐 pygame，执行 GPU 化（管线见 [`render`]）。
//! 依赖双向规则：本层可直用 starfish 类型；starfish 永不感知本层。
//!
//! P1 已落地：[`Color`] / [`Rect`]（回收旧实现）/ [`locals`] 常量枢纽 /
//! [`math`] 薄封装（Vector2/3/4 + Quaternion/Mat 扩展）。
//! P2 已落地：[`render`] 底座（Texture/Screen/Surface/DrawTarget/Batch/
//! Camera/Material/BufferProxy）+ [`font`]（资源制备路线）。
//! P3 已落地：[`display`] / [`event`] / [`key`] / [`mouse`] / [`time`] /
//! [`draw`] / [`version`] API 面（pygame hello = `examples/pygame_hello.rs`）。
//! P4 已落地：[`image`]（load/load_from_bytes → Surface；save = v2 回读）。
//! P5 提前：[`transform`]（flip/scale/rotate/rotozoom，GPU 采样实现）；
//! 会话/深度/MRT 见 [`render`]（ADR-5 v1.2）。

pub mod color;
pub mod display;
pub mod draw;
#[cfg(feature = "font")]
pub mod font;
pub mod event;
pub mod gamepad;
pub mod image;
pub mod key;
pub mod locals;
pub mod mask;
pub mod math;
pub mod mouse;
pub mod rect;
pub mod render;
pub mod sndarray;
/// pygame.sprite 对应物（Godot 式重设计——池化 Group + 世代 id +
/// (z,y) 排序会话绘制；细案见 reference/）
pub mod sprite;
pub mod touch;
/// pygame.surface 对应物（Surface 宿主 = [`render::Surface`]——会话与
/// DrawTarget 语义所在地；此模块为 pygame 名发现性再导出）
pub mod surface {
    pub use crate::pygame::render::Surface;
}
pub mod time;
pub mod transform;
pub mod version;

pub use color::Color;
pub use event::Event;
pub use rect::Rect;
