//! 20_net_probe：网络模块全流程探针（UDP 广播发现 → TCP echo → UDP echo）
//!
//! 流程（启动 ~0.5s 自动跑，点屏幕重跑；每步结果上屏）：
//! 1. DISCOVER  → UDP 广播 STARFISH_PROBE（3s 等待应答，学到服务器地址）
//! 2. CONNECT   → TcpConn::connect（轮询 state 直到 Connected，5s 超时）
//! 3. TCP ECHO  → 发 4 字节前缀分帧 payload → try_recv 收回显比对
//! 4. UDP ECHO  → send_to / try_recv_from 回显比对
//! 全过 → 绿屏 PASS；任一步失败 → 红屏 + 失败行（内容直接可读）
//!
//! 前置：PC 上运行 `python examples/server/server.py`（TCP echo :8022 +
//! UDP 发现 :8023 与 HTTP io :8021 一并启动）；手机与 PC 同一局域网。
//!
//! 运行：
//! - 桌面：`cargo run --example 20_net_probe`（同 PC 自测）
//! - Android：`cargo xtask android 20_net_probe`

use starfish::base::app::{Application, Ctx, WindowConfig};
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use starfish::base::app::run;
#[cfg(target_os = "android")]
use starfish::base::app::run_android;
use starfish::base::font::{self, Font, GlyphAtlas};
use starfish::base::net::{TcpConn, UdpSock};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use starfish::base::window::{Window, WindowEvent};
use wgpu::BufferUsages;

use std::sync::Arc;

const ATLAS_CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
const DISC_PORT: u16 = 8023;
const TCP_PORT: u16 = 8022;
const PROBE_REQ: &[u8] = b"STARFISH_PROBE";
const PROBE_REPLY: &[u8] = b"STARFISH_SERVER";
const ECHO_PAYLOAD: &[u8] = b"starfish-net-probe-echo-payload";

#[derive(Debug, Clone, Copy, PartialEq)]
enum Step {
    Discover,
    Connect,
    TcpEcho,
    UdpEcho,
    Pass,
    Done,
}

struct NetProbe {
    _context: Option<RenderContext>,
    access: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    atlas: Option<GlyphAtlas>,
    _camera_buffer: Option<Arc<wgpu::Buffer>>,
    camera_bind: Option<BindGroup>,
    atlas_bind: Option<BindGroup>,
    text_meshes: Vec<Mesh>,
    pipeline: Option<Arc<RenderPipeline>>,
    clear_color: wgpu::Color,
    lines: Vec<String>,
    text_dirty: bool,
    step: Step,
    step_elapsed: f32,
    next_send: f32,
    sock: Option<UdpSock>,
    conn: Option<TcpConn>,
    server: Option<String>,
}

impl NetProbe {
    fn new() -> Self {
        Self {
            _context: None,
            access: None,
            surface: None,
            atlas: None,
            _camera_buffer: None,
            camera_bind: None,
            atlas_bind: None,
            text_meshes: Vec::new(),
            pipeline: None,
            clear_color: wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 },
            lines: vec!["starting...".into()],
            text_dirty: false,
            step: Step::Discover,
            step_elapsed: 0.0,
            next_send: 0.0,
            sock: None,
            conn: None,
            server: None,
        }
    }

    fn set_lines(&mut self, lines: Vec<String>) {
        self.lines = lines
            .into_iter()
            .map(|t| {
                t.chars()
                    .map(|c| if ATLAS_CHARS.contains(c) { c } else { '?' })
                    .collect::<String>()
            })
            .collect();
        self.text_dirty = true;
    }

    fn rebuild_meshes(&mut self) {
        if let (Some(access), Some(atlas)) = (self.access.as_ref(), self.atlas.as_ref()) {
            self.text_meshes = self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .map(|(i, t)| {
                    font::text_mesh_tf(
                        access,
                        atlas,
                        t,
                        &glam::Mat4::from_translation(glam::Vec3::new(
                            24.0,
                            50.0 + i as f32 * 64.0,
                            0.0,
                        )),
                        1.0,
                        [0.6, 0.9, 1.0, 1.0],
                    )
                })
                .collect();
        }
        self.text_dirty = false;
    }

    fn fail(&mut self, msg: String) {
        println!("[net] FAIL: {msg}");
        self.set_lines(vec!["FAIL".into(), msg]);
        self.clear_color = wgpu::Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 };
        self.step = Step::Done;
    }

    /// 步骤 1：UDP 广播发现服务器（节流：每 0.5s 广播一次）。
    /// 返回 Some(ip) = 找到服务器；None = 等待中（bind 失败时已置错误态）。
    fn step_discover(&mut self) -> Option<String> {
        if self.sock.is_none() {
            match UdpSock::bind("0.0.0.0:0") {
                Ok(s) => {
                    let _ = s.set_broadcast(true);
                    self.sock = Some(s);
                }
                Err(e) => {
                    self.fail(format!("UDP bind: {e:?}"));
                    return None;
                }
            }
        }
        if self.step_elapsed >= self.next_send {
            self.next_send = self.step_elapsed + 0.5;
            if let Some(sock) = self.sock.as_ref() {
                let _ = sock.send_to(PROBE_REQ, &format!("255.255.255.255:{DISC_PORT}"));
            }
        }
        if let Some((data, peer)) = self.sock.as_ref().and_then(|s| s.try_recv_from()) {
            if data.as_slice() == PROBE_REPLY {
                return Some(peer.ip().to_string());
            }
        }
        None
    }
}

impl Application for NetProbe {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, access, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");

        let font = Font::from_bytes(
            include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
            30.0,
        )
        .expect("字体加载失败");
        let atlas = font
            .build_atlas(ATLAS_CHARS.chars(), &access)
            .expect("图集构建失败");
        let camera_buffer = access.create_raw_buffer(
            Some("net_camera"),
            64,
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let camera_bind = access
            .bind_group_builder()
            .uniform_raw(0, camera_buffer.clone(), 64)
            .build(Some("net_camera_bind"));
        let atlas_bind = font::atlas_bind_group(&access, &atlas);
        let first = font::text_mesh_tf(
            &access,
            &atlas,
            &self.lines[0],
            &glam::Mat4::from_translation(glam::Vec3::new(24.0, 50.0, 0.0)),
            1.0,
            [0.6, 0.9, 1.0, 1.0],
        );
        let pipeline = font::text_pipeline(&access, &camera_bind, &atlas_bind, &first, 1);

        self._context = Some(context);
        self.access = Some(access);
        self.surface = Some(surface);
        self.atlas = Some(atlas);
        self._camera_buffer = Some(camera_buffer);
        self.camera_bind = Some(camera_bind);
        self.atlas_bind = Some(atlas_bind);
        self.text_meshes = vec![first];
        self.pipeline = Some(pipeline);
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        if let WindowEvent::Resized { width, height } = event {
            if let Some(surface) = self.surface.as_mut() {
                surface.resize(*width, *height);
            }
        }
        if matches!(event, WindowEvent::MousePressed(_)) {
            // 重跑
            self.sock = None;
            self.conn = None;
            self.server = None;
            self.step = Step::Discover;
            self.step_elapsed = 0.0;
            self.set_lines(vec!["re-probing...".into()]);
            self.clear_color = wgpu::Color { r: 0.15, g: 0.15, b: 0.15, a: 1.0 };
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let dt = ctx.delta();
        match self.step {
            Step::Discover => {
                self.step_elapsed += dt;
                if self.step_elapsed > 3.0 && self.server.is_none() {
                    self.fail(
                        "discovery timeout (is the server on the same LAN and running?)".into(),
                    );
                    return;
                }
                if let Some(ip) = self.step_discover() {
                    self.server = Some(format!("{ip}:{TCP_PORT}"));
                    self.step_elapsed = 0.0;
                    self.next_send = 0.0;
                    self.set_lines(vec![format!("SERVER: {ip}"), "connecting...".into()]);
                    self.step = Step::Connect;
                }
            }
            Step::Connect => {
                self.step_elapsed += dt;
                let server = self.server.clone().unwrap_or_default();
                if self.conn.is_none() {
                    match TcpConn::connect(&format!("{server}")) {
                        Ok(c) => self.conn = Some(c),
                        Err(e) => {
                            self.fail(format!("connect: {e:?}"));
                            return;
                        }
                    }
                }
                let conn = self.conn.as_mut().unwrap();
                match conn.state() {
                    starfish::base::net::ConnState::Connected => {
                        self.set_lines(vec!["CONNECTED".into(), "TCP echo...".into()]);
                        self.step = Step::TcpEcho;
                        self.step_elapsed = 0.0;
                    }
                    _ if self.step_elapsed > 5.0 => {
                        self.fail("connect timeout".into());
                    }
                    _ => {}
                }
            }
            Step::TcpEcho => {
                let conn = self.conn.as_mut().unwrap();
                conn.send(ECHO_PAYLOAD).ok();
                match conn.try_recv() {
                    Some(data) if data == ECHO_PAYLOAD => {
                        self.set_lines(vec!["TCP ECHO OK".into(), "UDP echo...".into()]);
                        self.step = Step::UdpEcho;
                        self.step_elapsed = 0.0;
                    }
                    Some(other) => {
                        self.fail(format!("tcp echo mismatch: {other:?}"));
                    }
                    None if self.step_elapsed > 5.0 => {
                        self.fail("tcp echo timeout".into());
                    }
                    None => {}
                }
            }
            Step::UdpEcho => {
                self.step_elapsed += dt;
                if self.step_elapsed >= self.next_send {
                    self.next_send = self.step_elapsed + 0.5;
                    if let (Some(sock), Some(server)) = (self.sock.as_ref(), self.server.as_deref()) {
                        let ip = server.rsplit_once(':').map(|(ip, _)| ip).unwrap_or("");
                        let _ = sock.send_to(ECHO_PAYLOAD, &format!("{ip}:{DISC_PORT}"));
                    }
                }
                match self.sock.as_ref().and_then(|s| s.try_recv_from()) {
                    Some((data, _)) if data == ECHO_PAYLOAD => {
                        self.set_lines(vec!["PASS".into(), "udp + tcp all ok".into()]);
                        self.clear_color = wgpu::Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 };
                        self.step = Step::Pass;
                    }
                    _ if self.step_elapsed > 5.0 => {
                        self.fail("udp echo timeout".into());
                    }
                    _ => {}
                }
            }
            Step::Pass => {}
            Step::Done => {}
        }

        if self.text_dirty {
            self.rebuild_meshes();
        }

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(access) = self.access.as_ref() else { return };
        let Some(camera_buffer) = self._camera_buffer.as_ref() else { return };
        let Some(camera_bind) = self.camera_bind.as_ref() else { return };
        let Some(atlas_bind) = self.atlas_bind.as_ref() else { return };
        let Some(pipeline) = self.pipeline.as_ref() else { return };

        surface.begin_frame(self.clear_color, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = access.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("net_probe", &color_atts, None, None, None, None);
        for mesh in &self.text_meshes {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, camera_bind);
            pass.set_bind_group(1, atlas_bind);
            pass.set_mesh(mesh);
            pass.draw(0..mesh.vertex_count(), 0..1);
        }
        pass.end();
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
mod entry {
    use super::NetProbe;
    use starfish::base::app::{run_android, WindowConfig};

    #[unsafe(no_mangle)]
    fn android_main(app: winit::platform::android::activity::AndroidApp) {
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
        run_android(
            app,
            NetProbe::new(),
            WindowConfig::new("net probe", 800, 600).with_fps_cap(60),
        );
    }
}

// ── 桌面入口（bin）──
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn main() {
    use starfish::base::app::run;
    run(
        NetProbe::new(),
        WindowConfig::new("Net Probe", 800, 600).with_fps_cap(60),
    );
}
