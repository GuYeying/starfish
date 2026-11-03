//! Web 实现（wasm32）：TCP 语义 = WebSocket（稳定 API，自持 js_sys
//! Reflect 绑定）；UDP 浏览器不支持——显式报 `UnsupportedPlatform`，
//! 不做静默替代（与硬解唯一策略同一哲学）。由 lib.rs 按
//! `#[cfg(wasm32)] #[path]` 路由为 `mod imp`。

use super::{Connection, ConnState, NetError, PENDING_CAP, STATE_CLOSED, STATE_CONNECTED, STATE_CONNECTING};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use js_sys::Object;

// ── TCP（WebSocket，js_sys Reflect，稳定 API）──────────────────

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

pub(super) struct WebTcp {
    state: std::sync::Arc<std::sync::atomic::AtomicU8>,
    ws: Object,
    inbound: Rc<RefCell<VecDeque<Vec<u8>>>>,
    pending: Rc<RefCell<Vec<Vec<u8>>>>,
    _closures: Vec<Closure<dyn FnMut(JsValue)>>,
}

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

    pub(super) fn spawn(addr: String) -> Result<Self, NetError> {
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

// ── UDP（Web 显式不支持）──────────────────────────────────────

impl super::UdpSock {
    /// 绑定本地地址——Web 不支持
    pub fn bind(_local: &str) -> Result<Self, NetError> {
        starfish_permission::ensure(starfish_permission::Permission::Internet);
        Err(NetError::UnsupportedPlatform)
    }

    /// 发送到对端——Web 不支持
    pub fn send_to(&self, _data: &[u8], _peer: &str) -> Result<(), NetError> {
        Err(NetError::UnsupportedPlatform)
    }

    /// 收包——Web 不支持（恒无包）
    pub fn try_recv_from(&self) -> Option<(Vec<u8>, std::net::SocketAddr)> {
        None
    }
}
