pub mod app;
pub mod resources;
pub mod yuv;
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
#[cfg(feature = "dialog")]
pub mod dialog;
#[cfg(feature = "net")]
pub mod net;
pub mod web;
pub mod window;
mod rt;
pub mod error;
