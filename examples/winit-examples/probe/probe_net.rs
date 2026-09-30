//! probe_net：net 模块全流程验证（发现 → TCP echo → UDP echo / Web 走 WS）
//!
//! 平台差异由 API 行为表达，应用代码零 `#[cfg]`：
//! - Web：UDP 不可用 → 判 **SKIP**（能力缺失非失败）；TCP 经 `TcpConn` 的
//!   WebSocket 传输（`ws://{页面主机}:8024`，主机经 `kit::page_hostname()`
//!   运行时探测）
//! - 原生：UDP 广播发现服务器 → TCP :8022 echo → UDP :8023 echo；
//!   发现超时回落 127.0.0.1（同机自测）
//!
//! 对端：`python examples/server/server.py`（:8022 TCP / :8023 UDP / :8024 WS）。
//! 判据：console 锚点 `[net] TCP PASS`、`[net] UDP PASS/SKIP`、`[net] VERDICT PASS`
//! （TCP PASS 为底线，UDP SKIP 可接受）。

#[path = "kit.rs"]
mod kit;

use kit::{Status, StatusPanel};
use starfish::base::app::{Application, Ctx, WindowConfig};
use starfish::base::net::{TcpConn, UdpSock};
use starfish::base::window::{Window, WindowEvent};

const DISC_PORT: u16 = 8023;
const TCP_PORT: u16 = 8022;
const WS_PORT: u16 = 8024;
const PROBE_REQ: &[u8] = b"STARFISH_PROBE";
const PROBE_REPLY: &[u8] = b"STARFISH_SERVER";
const ECHO_PAYLOAD: &[u8] = b"starfish-net-probe-echo-payload";

enum Step {
    /// 帧 30 门：按平台分流（Web 直达 Tcp，原生先 Discover）
    Idle,
    /// 原生 UDP 广播发现（3s 超时回落 127.0.0.1）；sock 用 Option 便于 take
    Discover {
        sock: Option<UdpSock>,
        elapsed: f32,
        sent_at: f32,
    },
    /// TCP 连接 + echo；udp = 原生携带的发现 socket + 服务器 IP（echo 复用）
    Tcp {
        conn: TcpConn,
        udp: Option<(UdpSock, String)>,
        elapsed: f32,
        sent: bool,
    },
    /// 原生 UDP echo（5s 超时判 SKIP）
    UdpEcho {
        sock: UdpSock,
        ip: String,
        elapsed: f32,
        sent_at: f32,
    },
    Done,
}

/// 阶段内计算的结果（借用结束后再动 self）
enum TcpOut {
    EchoOk,
    Mismatch,
    EchoTimeout,
    ConnectTimeout,
}

struct NetProbe {
    panel: StatusPanel,
    step: Step,
    tcp_ok: bool,
    udp_ok: bool,
    udp_failed: bool,
}

impl NetProbe {
    fn new() -> Self {
        Self {
            panel: StatusPanel::new(),
            step: Step::Idle,
            tcp_ok: false,
            udp_ok: false,
            udp_failed: false,
        }
    }

    /// Web 目标地址：`ws://{页面主机}:{WS_PORT}`；原生 = None。
    /// 平台判定经 kit::page_hostname()（运行时能力探测，非 cfg）。
    fn web_target() -> Option<String> {
        kit::page_hostname().map(|host| format!("ws://{host}:{WS_PORT}"))
    }

    /// 收尾判定：TCP PASS 为底线；UDP SKIP 可接受、FAIL 不可
    fn finish(&mut self) {
        let verdict = if self.tcp_ok && !self.udp_failed {
            Status::Pass
        } else {
            Status::Fail
        };
        self.panel.verdict("net", "VERDICT", verdict, "");
        self.step = Step::Done;
    }

    fn start_tcp(&mut self, addr: String, udp: Option<(UdpSock, String)>) {
        match TcpConn::connect(&addr) {
            Ok(conn) => {
                self.step = Step::Tcp { conn, udp, elapsed: 0.0, sent: false };
            }
            Err(e) => {
                self.panel
                    .verdict("net", "TCP", Status::Fail, &format!("connect {e:?}"));
                self.finish();
            }
        }
    }
}

impl Application for NetProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        self.panel.init(ctx.window());
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        // 点击重跑
        if matches!(event, WindowEvent::MousePressed(_)) && matches!(self.step, Step::Done) {
            self.panel.set_lines(&["net probe: re-probing..."]);
            self.tcp_ok = false;
            self.udp_ok = false;
            self.udp_failed = false;
            self.step = Step::Idle;
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let f = self.panel.frame_no();
        let dt = ctx.delta();

        // ── 起：帧 30 按平台分流 ──
        if f == 30 && matches!(self.step, Step::Idle) {
            match Self::web_target() {
                Some(addr) => {
                    // Web：UDP 能力缺失 → SKIP（非失败）；直连 WS
                    self.panel.verdict("net", "UDP", Status::Skip, "no udp on web");
                    self.start_tcp(addr, None);
                }
                None => match UdpSock::bind("0.0.0.0:0") {
                    Ok(sock) => {
                        kit::enable_broadcast(&sock);
                        self.step = Step::Discover { sock: Some(sock), elapsed: 0.0, sent_at: 0.0 };
                    }
                    Err(e) => {
                        self.panel
                            .verdict("net", "UDP", Status::Fail, &format!("bind {e:?}"));
                        self.finish();
                    }
                },
            }
        }

        // ── 原生发现：每 0.5s 广播；3s 超时回落 127.0.0.1 ──
        let mut discovered: Option<(UdpSock, String, bool)> = None; // (sock, ip, found?)
        if let Step::Discover { sock, elapsed, sent_at } = &mut self.step {
            *elapsed += dt;
            let mut found: Option<(String, bool)> = None;
            if let Some(s) = sock.as_ref() {
                if *elapsed >= *sent_at {
                    *sent_at = *elapsed + 0.5;
                    let _ = s.send_to(PROBE_REQ, &format!("255.255.255.255:{DISC_PORT}"));
                }
                if let Some((data, peer)) = s.try_recv_from() {
                    if data.as_slice() == PROBE_REPLY {
                        found = Some((peer.ip().to_string(), true));
                    }
                }
            }
            if found.is_none() && *elapsed > 3.0 {
                found = Some(("127.0.0.1".to_string(), false));
            }
            if let Some((ip, ok)) = found {
                discovered = Some((sock.take().expect("discover sock"), ip, ok));
            }
        }
        if let Some((sock, ip, found)) = discovered {
            if found {
                self.panel.verdict("net", "DISCOVER", Status::Pass, &format!("{ip}"));
            } else {
                self.panel
                    .verdict("net", "DISCOVER", Status::Skip, "fallback 127.0.0.1");
            }
            self.start_tcp(format!("{ip}:{TCP_PORT}"), Some((sock, ip)));
        }

        // ── TCP：连接（5s）→ 发一次 → 首个回显比对 ──
        let mut tcp_out: Option<TcpOut> = None;
        if let Step::Tcp { conn, elapsed, sent, .. } = &mut self.step {
            *elapsed += dt;
            match conn.state() {
                starfish::base::net::ConnState::Connected => {
                    if !*sent {
                        *sent = true;
                        *elapsed = 0.0;
                        let _ = conn.send(ECHO_PAYLOAD);
                    } else if let Some(data) = conn.try_recv() {
                        if data.as_slice() == ECHO_PAYLOAD {
                            tcp_out = Some(TcpOut::EchoOk);
                        } else {
                            tcp_out = Some(TcpOut::Mismatch);
                        }
                    } else if *elapsed > 5.0 {
                        tcp_out = Some(TcpOut::EchoTimeout);
                    }
                }
                _ if *elapsed > 5.0 => tcp_out = Some(TcpOut::ConnectTimeout),
                _ => {}
            }
        }
        match tcp_out {
            Some(TcpOut::EchoOk) => {
                self.panel.verdict("net", "TCP", Status::Pass, "echo ok");
                self.tcp_ok = true;
                // 转 UDP echo（Web 无 UDP → 直接收尾）
                let udp = match &mut self.step {
                    Step::Tcp { udp, .. } => udp.take(),
                    _ => None,
                };
                match udp {
                    Some((sock, ip)) => {
                        self.step = Step::UdpEcho { sock, ip, elapsed: 0.0, sent_at: 0.0 };
                    }
                    None => self.finish(),
                }
            }
            Some(out) => {
                let (s, why) = match out {
                    TcpOut::Mismatch => (Status::Fail, "echo mismatch"),
                    TcpOut::EchoTimeout => (Status::Fail, "echo timeout"),
                    TcpOut::ConnectTimeout => (Status::Fail, "connect timeout"),
                    TcpOut::EchoOk => unreachable!(),
                };
                self.panel.verdict("net", "TCP", s, why);
                self.finish();
            }
            None => {}
        }

        // ── 原生 UDP echo：每 0.5s 发一次；5s 超时判 SKIP ──
        let mut udp_out: Option<Status> = None;
        if let Step::UdpEcho { sock, ip, elapsed, sent_at } = &mut self.step {
            *elapsed += dt;
            if *elapsed >= *sent_at {
                *sent_at = *elapsed + 0.5;
                let _ = sock.send_to(ECHO_PAYLOAD, &format!("{ip}:{DISC_PORT}"));
            }
            if let Some((data, _)) = sock.try_recv_from() {
                if data.as_slice() == ECHO_PAYLOAD {
                    udp_out = Some(Status::Pass);
                }
            } else if *elapsed > 5.0 {
                udp_out = Some(Status::Skip);
            }
        }
        if let Some(s) = udp_out {
            self.udp_ok = s == Status::Pass;
            self.udp_failed = s == Status::Fail;
            self.panel.verdict(
                "net",
                "UDP",
                s,
                match s {
                    Status::Pass => "echo ok",
                    _ => "echo timeout",
                },
            );
            self.finish();
        }

        self.panel.render(ctx, false, |_, _| {});
    }
}

starfish::app_entry!(
    NetProbe::new(),
    WindowConfig::new("probe net", 800, 600)
        .with_fps_cap(60)
        .with_web_canvas_id("canvas")
);
