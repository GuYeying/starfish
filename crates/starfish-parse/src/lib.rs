//! starfish-parse：结构化数据解析层（**stdlite 定界**——数据序列化格式：
//! 人可写、机可读、纯结构化数据的存档/配置/关卡数据面）。
//!
//! 界限定约（2026-11-24 用户定夺）：
//! - **收**：json / toml（后续可加 csv / ini）——`read → parse → 数据结构`
//!   通用通用
//! - **不收**：资产格式（建模 glTF / 图像 / 音频 / Spine 骨骼）——格式即
//!   资产语义，归领域加载器（glTF 语法上是 JSON 但用途是建模 = 陷阱案例）
//! - **不收**：codec 字节变换（hash/zlib/base64）——归 starfish-codec
//!
//! py 面 = pygame.parse（pygamest 绑定）。

pub mod json;
pub mod toml;
