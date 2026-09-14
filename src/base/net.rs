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

use std::collections::VecDeque;

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use js_sys::Object;

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

#[cfg(not(target_arch = "wasm32"))]
struct NativeTcp {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    to_net: Option<std::sync::mpsc::Sender<ToNet>>,
    from_net: std::sync::mpsc::Receiver<FromNet>,
    _handle: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
enum ToNet {
    Frame(Vec<u8>),
    Close,
}

#[cfg(not(target_arch = "wasm32"))]
enum FromNet {
    Frame(Vec<u8>),
}

impl TcpConn {
    /// 发起连接（立即返回；地址 `host:port` 或 `ws://host:port`）
    pub fn connect(addr: &str) -> Result<Self, NetError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Ok(Self {
                inner: Box::new(NativeTcp::spawn(addr.to_string())?),
            })
        }
        #[cfg(target_arch = "wasm32")]
        {
            Ok(Self {
                inner: Box::new(WebTcp::spawn(addr.to_string())?),
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

// ── 原生实现（后台线程 + std 阻塞流；含 Android/iOS——std::net 全支持）──

#[cfg(not(target_arch = "wasm32"))]
impl NativeTcp {
    fn spawn(addr: String) -> Result<Self, NetError> {
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
                eprintln!("starfish.net: 地址解析失败 {addr}: {e}");
                return;
            }
        };
        let stream = match TcpStream::connect_timeout(&resolved, std::time::Duration::from_secs(10))
        {
            Ok(s) => s,
            Err(e) => {
                eprintln!("starfish.net: 连接 {addr} 失败: {e}");
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

#[cfg(not(target_arch = "wasm32"))]
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

impl Drop for TcpConn {
    fn drop(&mut self) {
        self.close();
    }
}

// ── Web 实现（WebSocket，js_sys Reflect，稳定 API）────────────────

#[cfg(target_arch = "wasm32")]
mod web_ws {
    use super::*;
    use wasm_bindgen::JsCast;

    pub fn open(url: &str) -> Result<js_sys::Object, JsValue> {
        let global = js_sys::global();
        let ctor = js_sys::Reflect::get(&global, &"WebSocket".into())?;
        let ctor: js_sys::Function = ctor
            .dyn_into()
            .map_err(|_| JsValue::from_str("globalThis.WebSocket 不存在"))?;
        let args = js_sys::Array::new();
        args.push(&JsValue::from_str(url));
        let ws = js_sys::Reflect::construct(&ctor, &args)?;
        // 二进制帧收发（对齐 TCP 帧语义）
        js_sys::Reflect::set(&ws, &"binaryType".into(), &"arraybuffer".into())?;
        Ok(ws.into())
    }
}

#[cfg(target_arch = "wasm32")]
struct WebTcp {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    ws: Object,
    inbound: Rc<RefCell<VecDeque<Vec<u8>>>>,
    pending: Rc<RefCell<Vec<Vec<u8>>>>,
    _closures: Vec<Closure<dyn FnMut(JsValue)>>,
}

#[cfg(target_arch = "wasm32")]
impl Connection for WebTcp {
    fn state(&self) -> ConnState {
        // readyState 反射为浏览器权威；onopen/onclose 同时维护原子供 TcpConn 读取
        self.state_web()
    }
    fn send(&mut self, frame: &[u8]) -> Result<(), NetError> {
        self.send_ws(frame)
    }
    fn try_recv(&mut self) -> Option<Vec<u8>> {
        self.recv_ws()
    }
    fn close(&mut self) {
        self.close_ws()
    }
}

#[cfg(target_arch = "wasm32")]
impl WebTcp {
    fn state_web(&self) -> ConnState {
        match js_sys::Reflect::get(&self.ws, &"readyState".into())
            .ok()
            .and_then(|v| v.as_f64())
        {
            Some(1.0) => ConnState::Connected, // OPEN
            Some(0.0) => ConnState::Connecting, // CONNECTING
            _ => ConnState::Closed,
        }
    }

    fn send_ws(&mut self, data: &[u8]) -> Result<(), NetError> {
        if self.state_web() == ConnState::Closed {
            return Err(NetError::Backend("连接已关闭".into()));
        }
        let arr = js_sys::Uint8Array::from(data);
        let send: js_sys::Function = js_sys::Reflect::get(&self.ws, &"send".into())
            .ok()
            .and_then(|v| v.dyn_into().ok())
            .ok_or_else(|| NetError::Backend("WebSocket.send 缺失".into()))?;

        if self.state_web() == ConnState::Connected {
            send.call1(&self.ws, &arr)
                .map(|_| ())
                .map_err(|e| NetError::Backend(format!("send 失败: {e:?}")))
        } else {
            // Connecting：入队（onopen 冲刷）
            let mut pending = self.pending.borrow_mut();
            if pending.len() >= PENDING_CAP {
                return Err(NetError::Backend("发送缓冲已满".into()));
            }
            pending.push(data.to_vec());
            Ok(())
        }
    }

    fn recv_ws(&mut self) -> Option<Vec<u8>> {
        self.inbound.borrow_mut().pop_front()
    }

    fn close_ws(&mut self) {
        self.state
            .store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
        let close: js_sys::Function = match js_sys::Reflect::get(&self.ws, &"close".into())
            .ok()
            .and_then(|v| v.dyn_into().ok())
        {
            Some(f) => f,
            None => return,
        };
        let _ = close.call0(&self.ws);
    }

    fn spawn(addr: String) -> Result<Self, NetError> {
        let state = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(STATE_CONNECTING));
        // 地址归一：裸 host:port → ws://
        let url = if addr.starts_with("ws://") || addr.starts_with("wss://") {
            addr.clone()
        } else {
            format!("ws://{addr}")
        };

        let ws = web_ws::open(&url).map_err(|e| NetError::Backend(format!("{e:?}")))?;

        let inbound: Rc<RefCell<VecDeque<Vec<u8>>>> = Rc::new(RefCell::new(VecDeque::new()));
        let pending: Rc<RefCell<Vec<Vec<u8>>>> = Rc::new(RefCell::new(Vec::new()));

        let mut closures: Vec<Closure<dyn FnMut(JsValue)>> = Vec::new();

        // onopen：Connected + 冲刷积压
        {
            let ws = ws.clone();
            let pending = pending.clone();
            let state = state.clone();
            closures.push(Closure::<dyn FnMut(JsValue)>::new(move |_e: JsValue| {
                state.store(STATE_CONNECTED, std::sync::atomic::Ordering::Release);
                let send: js_sys::Function = match js_sys::Reflect::get(&ws, &"send".into())
                    .ok()
                    .and_then(|v| v.dyn_into().ok())
                {
                    Some(f) => f,
                    None => return,
                };
                for frame in pending.borrow_mut().drain(..) {
                    let arr = js_sys::Uint8Array::from(frame.as_slice());
                    let _ = send.call1(&ws, &arr);
                }
            }));
        }
        // onmessage：ArrayBuffer → Vec 入队
        {
            let inbound = inbound.clone();
            let state = state.clone();
            closures.push(Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                state.store(STATE_CONNECTED, std::sync::atomic::Ordering::Release);
                let Ok(event) = e.dyn_into::<web_sys::MessageEvent>() else {
                    return;
                };
                let Ok(buf) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
                    return;
                };
                inbound
                    .borrow_mut()
                    .push_back(js_sys::Uint8Array::new(&buf).to_vec());
            }));
        }
        // onclose / onerror：Closed
        for name in ["onclose", "onerror"] {
            let state = state.clone();
            closures.push(Closure::<dyn FnMut(JsValue)>::new(move |_e: JsValue| {
                state.store(STATE_CLOSED, std::sync::atomic::Ordering::Release);
            }));
            if let Err(e) = js_sys::Reflect::set(&ws, &name.into(), &closures.last().unwrap().as_ref()) {
                let _ = e;
            }
        }
        if let Err(e) = js_sys::Reflect::set(&ws, &"onopen".into(), &closures[0].as_ref()) {
            return Err(NetError::Backend(format!("{e:?}")));
        }
        if let Err(e) = js_sys::Reflect::set(&ws, &"onmessage".into(), &closures[1].as_ref()) {
            return Err(NetError::Backend(format!("{e:?}")));
        }

        Ok(Self {
            state,
            ws,
            inbound,
            pending,
            _closures: closures,
        })
    }

}

// ── UDP（原生；Web 显式不支持）────────────────────────────────

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

impl UdpSock {
    /// 绑定本地地址（如 "0.0.0.0:7777"；Web 返回 UnsupportedPlatform）
    pub fn bind(local: &str) -> Result<Self, NetError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = local;
            Err(NetError::UnsupportedPlatform)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let socket = std::net::UdpSocket::bind(local)
                .map_err(|e| NetError::Backend(format!("bind {local} 失败: {e}")))?;
            socket
                .set_nonblocking(true)
                .map_err(|e| NetError::Backend(format!("set_nonblocking 失败: {e}")))?;
            Ok(Self { socket })
        }
    }

    /// 发送到对端（非阻塞；缓冲满返回 WouldBlock 错误）
    pub fn send_to(&self, data: &[u8], peer: &str) -> Result<(), NetError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (data, peer);
            Err(NetError::UnsupportedPlatform)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let peer: std::net::SocketAddr = peer
                .parse()
                .map_err(|e| NetError::Backend(format!("对端地址无效: {e}")))?;
            self.socket
                .send_to(data, peer)
                .map(|_| ())
                .map_err(|e| NetError::Backend(format!("send_to 失败: {e}")))
        }
    }

    /// 收包（非阻塞；无包返回 None）
    pub fn try_recv_from(&self) -> Option<(Vec<u8>, std::net::SocketAddr)> {
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut buf = [0u8; 1500]; // MTU 尺寸常规报文
            match self.socket.recv_from(&mut buf) {
                Ok((n, peer)) => Some((buf[..n].to_vec(), peer)),
                Err(_) => None,
            }
        }
    }
}
