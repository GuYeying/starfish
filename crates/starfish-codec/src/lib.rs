//! starfish-codec：编解码层（**stdlite 定约**——字节变换家族）。
//!
//! 成员：hash（sha256——存档校验/防篡改）、zlib（flate2 压缩——存档
//! 体积）、base64（wasm data URI / 存档编码）。
//!
//! py 面 = pygame.codec（pygamest 绑定）。

pub mod base64;
pub mod hash;
pub mod zlib;
