//! pygame.net 对应物（**starfish 扩展域**：真 pygame 无 net 模块——
//! TCP/UDP 网络能力，包装 [`crate::base::net]`）。
//!
//! 权限：Android 首次建联时由 base 层隐式申请 `INTERNET`
//! （normal 权限，清单声明即授予，无感）。
//!
//! 模式：**同步轮询**（`send` / `try_recv` 非阻塞收发）——与引擎 poll
//! 模型同构：循环体里收发，永不阻塞帧。WebSocket（Web 平台）见
//! base::net 平台注释。

pub use crate::base::net::{ConnState, NetError, TcpConn, UdpSock};

/// 连接状态
pub use crate::base::net::Connection;
