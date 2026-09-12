pub mod app;
pub mod resources;
pub mod time;
pub mod render;
pub mod color;
// ── 可选特性模块（见 Cargo.toml [features]；默认全包含）──
#[cfg(feature = "gfx")]
pub mod gfx;
pub mod audio;
#[cfg(feature = "font")]
pub mod font;
#[cfg(feature = "video")]
pub mod video;
#[cfg(feature = "gamepad")]
pub mod gamepad;
pub mod web;
pub mod window;
mod rt;
pub mod error;
