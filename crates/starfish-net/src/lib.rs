//! base/net —— 网络接口（TCP 消息连接 / UDP 轮询）
//!
//! 设计定稿（用户决策）：
//! - **网络 IO 独立于主线程与本地 IO**：桌面/移动每条 TCP 连接一个后台线程
//!   （std 阻塞流 + 通道），主线程阻塞（弹窗/本地文件）不影响网络正确性——
//!   因此**不引入 tokio**（游戏客户端连接数极少，线程直连是标准做法，
//!   编译时间与二进制体积归零）
//! - **统一轮询式消息 API**：`send` / `try_recv`，TCP 以 4 字节大端长度前缀
//!   分帧（单帧上限 16 MB），与 Web 的 WebSocket 消息语义对齐——一套业务
//!   代码，cfg 切换底层传输
//! - **Web**：WebSocket（稳定 API，自持 js_sys Reflect 绑定，与视频/手柄同法）；
//!   **UDP 浏览器不支持——显式报 `UnsupportedPlatform`，不做静默替代**
//!   （与硬解唯一策略同一哲学）
//!
//! v1 范围：客户端（主动连接）。监听/服务端待需求落地。
//!
//! **模块布局（2026-11-03 分平台拆分）**：统一接口（错误/状态/帧常量/
//! Connection trait/TcpConn/UdpSock 壳）在本文件导出；平台实现按 cfg
//! 路由——`native.rs`（线程 TCP + 非阻塞 UDP）/ `web.rs`（WebSocket +
//! UDP 不支持 stub）二选一装载为 `mod imp`。

/// 网络错误
#[derive(Debug)]
pub enum NetError {
    /// 当前平台不支持该能力（如 Web 的 UDP）
    UnsupportedPlatform,
    /// 后端错误（含地址解析、连接、协议错误）
    Backend(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::UnsupportedPlatform => write!(f, "net: 该平台不支持此能力"),
            NetError::Backend(s) => write!(f, "net 后端错误: {s}"),
        }
    }
}
impl std::error::Error for NetError {}

/// 连接状态（`connect` 立即返回，握手在后台进行）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// 握手中（send 会入队，连接成功后自动冲刷）
    Connecting,
    /// 已连接（可收发）
    Connected,
    /// 已关闭/连接失败
    Closed,
}

const FRAME_HEADER: usize = 4;
/// 单帧上限 16 MB（防恶意/错误长度打爆内存）
const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Connecting 期间的发送缓冲帧数上限
const PENDING_CAP: usize = 512;

const STATE_CONNECTING: u8 = 0;
const STATE_CONNECTED: u8 = 1;
const STATE_CLOSED: u8 = 2;

// ── 平台实现路由（统一 `mod imp` 面）──────────────────────────

#[cfg(not(target_arch = "wasm32"))]
#[path = "native.rs"]
mod imp;

#[cfg(target_arch = "wasm32")]
#[path = "web.rs"]
mod imp;

// ── TCP 消息连接 ──────────────────────────────────────────────

/// 传输连接抽象——TCP(原生) / WebSocket(Web) / 未来传输的统一接口
///
/// 轮询式语义（对齐手柄/视频泵模式）：应用在 `frame` 里 `try_recv` 收帧、
/// `send` 发帧，主线程永不阻塞。
pub trait Connection {
    fn state(&self) -> ConnState;
    fn send(&mut self, frame: &[u8]) -> Result<(), NetError>;
    fn try_recv(&mut self) -> Option<Vec<u8>>;
    fn close(&mut self);
}

/// TCP 客户端连接（消息语义：4 字节大端长度前缀分帧，单帧上限 16 MB）
///
/// 原生 = 后台线程 + std 阻塞流；Web = WebSocket。`connect` 立即返回
/// （后台握手），应用在 `frame` 里轮询收发。
pub struct TcpConn {
    inner: Box<dyn Connection>,
}

impl TcpConn {
    /// 发起连接（立即返回；地址 `host:port` 或 `ws://host:port`）
    pub fn connect(addr: &str) -> Result<Self, NetError> {
        // 隐式权限声明（Android INTERNET = normal 权限：清单声明即安装时
        // 授予，此处瞬时通过无弹框；其他平台恒 true——纯语义声明，见
        // base::permission 的两类区分）
        starfish_permission::ensure(starfish_permission::Permission::Internet);
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(Self {
                inner: Box::new(imp::NativeTcp::spawn(addr.to_string())?),
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Self {
                inner: Box::new(imp::WebTcp::spawn(addr.to_string())?),
            })
        }
    }

    pub fn state(&self) -> ConnState {
        self.inner.state()
    }

    /// 发送一帧（Connecting 时入队缓冲，连接成功后自动冲刷；缓冲满报错）
    pub fn send(&mut self, data: &[u8]) -> Result<(), NetError> {
        if data.len() + FRAME_HEADER > MAX_FRAME {
            return Err(NetError::Backend(format!(
                "帧超限: {} 字节（上限 {MAX_FRAME}）",
                data.len()
            )));
        }
        self.inner.send(data)
    }

    /// 收一帧（非阻塞；无帧返回 None——下一帧再查）
    pub fn try_recv(&mut self) -> Option<Vec<u8>> {
        self.inner.try_recv()
    }

    pub fn close(&mut self) {
        self.inner.close();
    }
}

impl Drop for TcpConn {
    fn drop(&mut self) {
        self.close();
    }
}

// ── UDP（原生；Web 显式不支持——stub 在 web.rs）────────────────

/// UDP 套接字（非阻塞轮询式；原生平台，Web 不支持）
pub struct UdpSock {
    #[cfg(not(target_arch = "wasm32"))]
    socket: std::net::UdpSocket,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// 本地 echo 回环：验证连接状态机、帧化分帧/重组、多帧顺序
    #[test]
    fn tcp_frame_roundtrip_local_echo() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            loop {
                let mut header = [0u8; 4];
                if stream.read_exact(&mut header).is_err() {
                    break;
                }
                let len = u32::from_be_bytes(header) as usize;
                let mut buf = vec![0u8; len];
                if stream.read_exact(&mut buf).is_err() {
                    break;
                }
                let _ = stream.write_all(&header);
                let _ = stream.write_all(&buf);
            }
        });

        let mut conn = TcpConn::connect(&addr).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while conn.state() == ConnState::Connecting {
            assert!(std::time::Instant::now() < deadline, "连接超时");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(conn.state(), ConnState::Connected);

        // 三帧：小帧 / 64KB 大帧（跨 TCP 分段）/ 二进制含零
        let frames: Vec<Vec<u8>> = vec![
            b"hello".to_vec(),
            vec![7u8; 64 * 1024],
            b"\x00\x01\x02".to_vec(),
        ];
        for f in &frames {
            conn.send(f).unwrap();
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut got: Vec<Vec<u8>> = Vec::new();
        while got.len() < frames.len() {
            assert!(std::time::Instant::now() < deadline, "回显超时");
            if let Some(f) = conn.try_recv() {
                got.push(f);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(got, frames, "帧内容与顺序必须完全一致");
        conn.close();
        assert_eq!(conn.state(), ConnState::Closed);
    }

    #[test]
    fn udp_loopback_send_and_idle_recv() {
        let sock = UdpSock::bind("127.0.0.1:0").unwrap();
        let peer = "127.0.0.1:9";
        sock.send_to(b"ping", peer).unwrap(); // discard 端口，发送成功即可
        assert!(sock.try_recv_from().is_none(), "非阻塞空闲应收不到包");
    }

    #[test]
    fn udp_bind_invalid_errors() {
        assert!(UdpSock::bind("999.999.0.1:0").is_err());
    }
}
