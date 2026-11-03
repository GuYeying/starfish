//! 原生实现（桌面 + 移动——std::net 全支持）：TCP = 后台线程 + std 阻塞
//! 流 + 通道（每连接一线程，主线程阻塞不影响网络正确性）；UDP = 非阻塞
//! 轮询。由 lib.rs 按 `#[cfg(not(wasm32))] #[path]` 路由为 `mod imp`。

use super::{Connection, ConnState, NetError, FRAME_HEADER, MAX_FRAME, STATE_CLOSED, STATE_CONNECTED, STATE_CONNECTING};

// ── TCP（后台线程 + std 阻塞流）────────────────────────────────

pub(super) struct NativeTcp {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    to_net: Option<std::sync::mpsc::Sender<ToNet>>,
    from_net: std::sync::mpsc::Receiver<FromNet>,
    _handle: Option<std::thread::JoinHandle<()>>,
}

enum ToNet {
    Frame(Vec<u8>),
    Close,
}

enum FromNet {
    Frame(Vec<u8>),
}

impl NativeTcp {
    pub(super) fn spawn(addr: String) -> Result<Self, NetError> {
        let (to_net, out_rx) = std::sync::mpsc::channel::<ToNet>();
        let (from_tx, from_net) = std::sync::mpsc::channel::<FromNet>();
        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(STATE_CONNECTING));

        let addr_for_thread = addr.clone();
        let state_for_thread = state.clone();
        let handle = std::thread::Builder::new()
            .name(format!("starfish.tcp[{addr}]"))
            .spawn(move || {
                Self::run(addr_for_thread, state_for_thread, out_rx, from_tx);
            })
            .map_err(|e| NetError::Backend(format!("网络线程创建失败: {e}")))?;

        Ok(Self {
            state,
            to_net: Some(to_net),
            from_net,
            _handle: Some(handle),
        })
    }

    fn run(
        addr: String,
        state: std::sync::Arc<std::sync::atomic::AtomicU8>,
        out_rx: std::sync::mpsc::Receiver<ToNet>,
        from_tx: std::sync::mpsc::Sender<FromNet>,
    ) {
        use std::io::{Read, Write};
        use std::net::{TcpStream, ToSocketAddrs};

        // 握手：解析 + 10 秒连接超时
        let resolved = match addr.as_str().to_socket_addrs() {
            Ok(mut it) => match it.next() {
                Some(a) => a,
                None => {
                    state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
                    return;
                }
            },
            Err(e) => {
                state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
                starfish_debug::log_error!(format!("starfish.net: 地址解析失败 {addr}: {e}"));
                return;
            }
        };
        let stream = match TcpStream::connect_timeout(&resolved, std::time::Duration::from_secs(10))
        {
            Ok(s) => s,
            Err(e) => {
                starfish_debug::log_error!(format!("starfish.net: 连接 {addr} 失败: {e}"));
                state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
                return;
            }
        };
        let _ = stream.set_nodelay(true);
        let mut writer = match stream.try_clone() {
            Ok(w) => w,
            Err(_) => {
                state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
                return;
            }
        };
        let mut reader = stream;
        state.store(STATE_CONNECTED, std::sync::atomic::Ordering::Release);

        // 写线程：冲刷 Connecting 期间积压 + 后续帧（Close/断路即退出）
        let write_handle = std::thread::Builder::new()
            .name("starfish.tcp-tx".into())
            .spawn(move || {
                while let Ok(msg) = out_rx.recv() {
                    match msg {
                        ToNet::Frame(data) => {
                            let len = (data.len() as u32).to_be_bytes();
                            if writer.write_all(&len).is_err() || writer.write_all(&data).is_err() {
                                break;
                            }
                        }
                        ToNet::Close => break,
                    }
                }
            })
            .ok();

        // 读循环：长度前缀分帧
        loop {
            let mut header = [0u8; FRAME_HEADER];
            if reader.read_exact(&mut header).is_err() {
                break;
            }
            let len = u32::from_be_bytes(header) as usize;
            if len > MAX_FRAME {
                break; // 恶意/错位长度：断连保护
            }
            let mut frame = vec![0u8; len];
            if len > 0 && reader.read_exact(&mut frame).is_err() {
                break;
            }
            if from_tx.send(FromNet::Frame(frame)).is_err() {
                break; // 应用侧已丢弃连接
            }
        }

        state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
        if let Some(h) = write_handle {
            let _ = h.join();
        }
    }

    fn send_in(&mut self, data: &[u8]) -> Result<(), NetError> {
        if let Some(to_net) = &self.to_net {
            to_net
                .send(ToNet::Frame(data.to_vec()))
                .map_err(|_| NetError::Backend("连接已关闭".into()))
        } else {
            Err(NetError::Backend("连接已关闭".into()))
        }
    }

    fn recv_in(&mut self) -> Option<Vec<u8>> {
        match self.from_net.try_recv() {
            Ok(FromNet::Frame(frame)) => Some(frame),
            Err(_) => {
                if self.state.load(std::sync::atomic::Ordering::Acquire) == STATE_CLOSED {
                    self.to_net = None; // 断线后清发送端
                }
                None
            }
        }
    }

    fn close_in(&mut self) {
        self.state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
        if let Some(to_net) = self.to_net.take() {
            let _ = to_net.send(ToNet::Close);
        }
        // 读线程随流关闭自行退出；句柄随 NativeTcp drop 分离
        let _ = self._handle.take();
    }
}

impl Connection for NativeTcp {
    fn state(&self) -> ConnState {
        match self.state.load(std::sync::atomic::Ordering::Acquire) {
            STATE_CONNECTING => ConnState::Connecting,
            STATE_CONNECTED => ConnState::Connected,
            _ => ConnState::Closed,
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), NetError> {
        self.send_in(frame)
    }

    fn try_recv(&mut self) -> Option<Vec<u8>> {
        self.recv_in()
    }

    fn close(&mut self) {
        self.close_in()
    }
}

// ── UDP（非阻塞轮询；Web 不支持——stub 在 web.rs）──────────────

impl super::UdpSock {
    /// 绑定本地地址（如 "0.0.0.0:7777"）
    pub fn bind(local: &str) -> Result<Self, NetError> {
        // 隐式权限声明（同 TcpConn::connect——Android normal 权限瞬时通过）
        starfish_permission::ensure(starfish_permission::Permission::Internet);
        let socket = std::net::UdpSocket::bind(local)
            .map_err(|e| NetError::Backend(format!("bind {local} 失败: {e}")))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| NetError::Backend(format!("set_nonblocking 失败: {e}")))?;
        Ok(Self { socket })
    }

    /// 发送到对端（非阻塞；缓冲满返回 WouldBlock 错误）
    pub fn send_to(&self, data: &[u8], peer: &str) -> Result<(), NetError> {
        let peer: std::net::SocketAddr = peer
            .parse()
            .map_err(|e| NetError::Backend(format!("对端地址无效: {e}")))?;
        self.socket
            .send_to(data, peer)
            .map(|_| ())
            .map_err(|e| NetError::Backend(format!("send_to 失败: {e}")))
    }

    /// 启用广播发送（发现类探针需要）
    pub fn set_broadcast(&self, on: bool) -> Result<(), NetError> {
        self.socket
            .set_broadcast(on)
            .map_err(|e| NetError::Backend(format!("set_broadcast 失败: {e}")))
    }

    /// 收包（非阻塞；无包返回 None）
    pub fn try_recv_from(&self) -> Option<(Vec<u8>, std::net::SocketAddr)> {
        let mut buf = [0u8; 1500]; // MTU 尺寸常规报文
        match self.socket.recv_from(&mut buf) {
            Ok((n, peer)) => Some((buf[..n].to_vec(), peer)),
            Err(_) => None,
        }
    }
}
