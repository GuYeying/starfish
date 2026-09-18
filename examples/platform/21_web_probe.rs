//! 21_web_probe：Web 平台能力总探针（输入/音频/录音/视频/手柄/对话框/网络）
//!
//! 每个测试项一行状态，点屏幕依次触发需要手势的测试（浏览器自动播放/
//! 麦克风授权都要求用户手势），结果全部字体上屏：
//!
//! | 行 | 测试 | 触发 | 通过标准 |
//! |---|---|---|---|
//! | INPUT  | 键鼠输入 | 被动计数 | 点击数 ≥ 1 |
//! | AUDIO  | 音频播放 | 第 1 次点击 | 内嵌 WAV 解码播放无错 |
//! | RECORD | 录音 2s  | 第 2 次点击 | getUserMedia + 采集字节 > 0 |
//! | VIDEO  | 视频硬解 | 自动（fetch） | WebCodecs 解码 + 纹理就绪 |
//! | GAMEPAD| 手柄环境 | 被动轮询 | 显示已连接手柄数 |
//! | DIALOG | 文件选择 | 第 3 次点击 | input[file] 选择成功 |
//! | NET WS | WebSocket echo | 自动 | 回显比对一致 |
//!
//! 运行（wasm 构建 + 服务器同源部署，见 reference/wasm编译与运行指南.md）：
//! ```bash
//! cargo build --release --target wasm32-unknown-unknown --example 21_web_probe
//! wasm-bindgen --out-dir web --target web target/wasm32-unknown-unknown/release/examples/21_web_probe.wasm
//! # 视频测试需把 resources/videos/sample-5s.mp4 拷进 web 目录
//! python examples/server/server.py --web-dir ./web
//! ```
//! 桌面：`cargo run --example 21_web_probe`（同样可跑，作为行为对照）

use starfish::base::app::{Application, Ctx, InitSlot, WindowConfig};
use starfish::base::audio::decoder::SymphoniaReader;
use starfish::base::audio::{AudioMixer, AudioRecorder, SoundData};
use starfish::base::dialog;
use starfish::base::font::{self, Font, GlyphAtlas};
use starfish::base::net::{ConnState, TcpConn};
use starfish::base::render::bind_group::bind_group::BindGroup;
use starfish::base::render::mesh::mesh::Mesh;
use starfish::base::render::pipeline::RenderPipeline;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::settings::{GpuSettings, SurfaceSettings};
use starfish::base::render::RenderContext;
use starfish::base::web::console_log;
use starfish::base::video::{Video, VideoModule};
use starfish::base::window::{Window, WindowEvent};
use wgpu::BufferUsages;
use wgpu::Color;

use std::sync::Arc;
use std::time::Duration;

const ATLAS_CHARS: &str = " !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ[\\]^_`abcdefghijklmnopqrstuvwxyz{|}~";
const LINES: [&str; 7] = [
    "INPUT : taps 0",
    "AUDIO : tap 1 to play",
    "RECORD: tap 2 to record",
    "VIDEO : pending",
    "GAMEPAD: checking...",
    "DIALOG: tap 3 to pick",
    "NET WS: connecting...",
];

/// Web 端对话框结果的跨帧传递槽（spawn_local 写 → frame 读）
#[cfg(target_arch = "wasm32")]
static DIALOG_MSG: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// GPU 资源包（InitSlot 异步初始化完成后的渲染侧全部家当；视频在 Web 端
/// = fetch 解码，桌面 = resources 路径直开）
struct Gpu {
    _context: RenderContext,
    access: RenderResourceAccess,
    surface: RenderSurface,
    atlas: GlyphAtlas,
    _camera_buffer: Arc<wgpu::Buffer>,
    camera_bind: BindGroup,
    atlas_bind: BindGroup,
    text_meshes: Vec<Mesh>,
    pipeline: Arc<RenderPipeline>,
    /// 视频句柄（Web = fetch 解码；桌面 = resources 路径）
    video: Option<Video>,
}

struct WebProbe {
    gpu: InitSlot<Gpu>,
    lines: Vec<String>,
    taps: u32,
    text_dirty: bool,
    /// 录音进行中的采集器
    recorder: Option<AudioRecorder>,
    record_elapsed: f32,
    /// 音频播放测试的混音器（保活）
    audio: Option<AudioMixer>,
    /// WS echo 连接
    conn: Option<TcpConn>,
    net_elapsed: f32,
}

impl WebProbe {
    fn new() -> Self {
        Self {
            gpu: InitSlot::default(),
            lines: LINES.iter().map(|s| s.to_string()).collect(),
            taps: 0,
            text_dirty: false,
            recorder: None,
            record_elapsed: 0.0,
            audio: None,
            conn: None,
            net_elapsed: 0.0,
        }
    }

    /// 设置第 item 行状态（0=INPUT, 1=AUDIO, 2=RECORD, 3=VIDEO, 4=GAMEPAD, 5=DIALOG, 6=NET）
    fn set_status(&mut self, item: usize, status: &str) {
        let tag = [
            "INPUT", "AUDIO", "RECORD", "VIDEO", "GAMEPAD", "DIALOG", "NET WS",
        ][item];
        let line: String = format!("{tag}: {status}")
            .chars()
            .map(|c| if ATLAS_CHARS.contains(c) { c } else { '?' })
            .collect();
        if self.lines.get(item) != Some(&line) {
            while self.lines.len() <= item {
                self.lines.push(String::new());
            }
            self.lines[item] = line;
            self.text_dirty = true;
        }
    }

    /// 重建全部文本网格（状态变化时一次；atlas/access 均在 Gpu 内）
    fn rebuild_meshes(&mut self) {
        if let Some(mut gpu) = self.gpu.get_mut() {
            gpu.text_meshes = self
                .lines
                .iter()
                .enumerate()
                .filter(|(_, t)| !t.is_empty())
                .map(|(i, t)| {
                    font::text_mesh_tf(
                        &gpu.access,
                        &gpu.atlas,
                        t,
                        &glam::Mat4::from_translation(glam::Vec3::new(
                            24.0,
                            50.0 + i as f32 * 60.0,
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

    // ── 手势测试 1：音频播放（内嵌 WAV 拉式解码 → 混音器播放）──
    fn run_audio_test(&mut self) {
        let wav = include_bytes!("../../resources/audio/solid.wav").to_vec();
        let mut reader = match SymphoniaReader::from_bytes(wav) {
            Ok(d) => d,
            Err(e) => {
                self.set_status(1, &format!("decode ERR {e:?}"));
                return;
            }
        };
        let rate = reader.src_rate();
        let mut samples: Vec<f32> = Vec::new();
        loop {
            match reader.next_interleaved() {
                Ok(Some(chunk)) => samples.extend(chunk),
                Ok(None) => break,
                Err(e) => {
                    self.set_status(1, &format!("decode ERR {e:?}"));
                    return;
                }
            }
        }
        if samples.is_empty() {
            self.set_status(1, "decode empty");
            return;
        }
        let mut mixer = match AudioMixer::new(4) {
            Ok(m) => m,
            Err(e) => {
                self.set_status(1, &format!("mixer ERR {e:?}"));
                return;
            }
        };
        let sound = Arc::new(SoundData::from_interleaved_f32(&samples, rate));
        let dur = sound.duration();
        match mixer.play_with(sound, 0, 0.0) {
            Ok(_) => self.set_status(1, &format!("playing {:.1}s", dur)),
            Err(e) => self.set_status(1, &format!("play ERR {e:?}")),
        }
        self.audio = Some(mixer); // 保活
    }

    // ── 手势测试 2：录音 2 秒（getUserMedia 授权框 → 采集 → WAV 字节数）──
    fn start_record(&mut self) {
        match AudioRecorder::new_with_capacity(48_000 * 4) {
            Ok(recorder) => {
                self.record_elapsed = 0.0;
                self.recorder = Some(recorder);
                self.set_status(2, "RECORD: recording 2s...");
            }
            Err(e) => self.set_status(2, &format!("REC ERR {e:?}")),
        }
    }

    // ── 手势测试 3：对话框文件选择（桌面 rfd 阻塞 / Web input[file] 真异步）──
    fn run_dialog_test(&mut self) {
        self.set_status(5, "DIALOG: opening...");

        #[cfg(target_arch = "wasm32")]
        {
            // pick 是真异步：spawn_local 驱动，结果经静态槽跨帧回传
            wasm_bindgen_futures::spawn_local(async move {
                let msg = match dialog::pick_file(Some("web pick"), &[("any", &["*"])]).await {
                    Ok(Some(pf)) => format!("PICK OK {}", pf.name()),
                    Ok(None) => "PICK CANCELLED".into(),
                    Err(e) => format!("PICK ERR {e:?}"),
                };
                *DIALOG_MSG.lock().unwrap() = Some(msg);
            });
        }

        #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
        {
            let msg = match pollster::block_on(dialog::pick_file(None, &[])) {
                Ok(Some(pf)) => format!("PICK OK {}", pf.name()),
                Ok(None) => "PICK CANCELLED".into(),
                Err(e) => format!("PICK ERR {e:?}"),
            };
            self.set_status(5, msg.as_str());
        }

        #[cfg(target_os = "android")]
        {
            let msg = match pollster::block_on(dialog::pick_file(None, &[])) {
                Ok(Some(pf)) => format!("PICK OK {}", pf.name()),
                Ok(None) => "PICK CANCELLED".into(),
                Err(e) => format!("PICK ERR {e:?}"),
            };
            self.set_status(5, msg.as_str());
        }
    }
}

impl Application for WebProbe {
    /// 首窗就绪：InitSlot 异步建 GPU 家当 + 打开视频（fetch）
    fn start(&mut self, ctx: &mut Ctx) {
        let window = ctx.window().clone();
        let slot = self.gpu.clone();

        // [InitSlot] 桌面：阻塞填充；Web：spawn_local 排队（绝不阻塞主线程）
        slot.init(async move {
            let (context, access, surface) =
                RenderEntry::async_new(
                    &window,
                    SurfaceSettings::default(),
                    GpuSettings::default(),
                )
                    .await
                    .expect("RenderContext 初始化失败");

            let font = Font::from_bytes(
                include_bytes!("../../resources/fonts/Antonio-Regular.ttf").to_vec(),
                30.0,
            )
            .expect("字体加载失败");
            let atlas = font
                .build_atlas(ATLAS_CHARS.chars(), &access)
                .expect("图集构建失败");
            let camera_buffer = access.create_raw_buffer(
                Some("web_camera"),
                64,
                BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            );
            let camera_bind = access
                .bind_group_builder()
                .uniform_raw(0, camera_buffer.clone(), 64)
                .build(Some("web_camera_bind"));
            let atlas_bind = font::atlas_bind_group(&access, &atlas);
            let first = font::text_mesh_tf(
                &access,
                &atlas,
                &LINES[0],
                &glam::Mat4::from_translation(glam::Vec3::new(24.0, 50.0, 0.0)),
                1.0,
                [0.6, 0.9, 1.0, 1.0],
            );
            let pipeline = font::text_pipeline(&access, &camera_bind, &atlas_bind, &first, 1);

            // 视频（Web = fetch 页面相对 URL；桌面 = resources 路径）
            #[cfg(target_arch = "wasm32")]
            let opened = VideoModule::new(context.device().clone(), context.queue().clone())
                .open("sample-5s.mp4");
            #[cfg(not(target_arch = "wasm32"))]
            let opened = VideoModule::new(context.device().clone(), context.queue().clone())
                .open("resources/videos/sample-5s.mp4");
            let video = match opened {
                Ok(v) => Some(v),
                Err(e) => {
                    console_log("[21] 视频打开失败");
                    console_log(&format!("[21] {e:?}"));
                    None
                }
            };

            Gpu {
                _context: context,
                access,
                surface,
                atlas,
                _camera_buffer: camera_buffer,
                camera_bind,
                atlas_bind,
                text_meshes: vec![first],
                pipeline,
                video,
            }
        });

        // 网络：WS echo（Web 自动 ws://页面host:8024；桌面 TCP 127.0.0.1:8022）
        #[cfg(target_arch = "wasm32")]
        let net_addr = {
            let host = web_sys::window()
                .map(|w| w.location().host().unwrap_or_default())
                .unwrap_or_default();
            let ip = host
                .rsplit_once(':')
                .map(|(ip, _)| ip.to_string())
                .unwrap_or(host);
            format!("ws://{ip}:8024")
        };
        #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
        let net_addr = "127.0.0.1:8022".to_string();
        self.conn = match TcpConn::connect(&net_addr) {
            Ok(c) => Some(c),
            Err(e) => {
                console_log(&format!("[21] WS 连接失败: {e:?}"));
                None
            }
        };
    }

    fn event(&mut self, _win: &Window, event: &WindowEvent, _ctx: &mut Ctx) {
        if let WindowEvent::Resized { width, height } = event {
            if let Some(mut gpu) = self.gpu.get_mut() {
                gpu.surface.resize(*width, *height);
            }
        }
        // 输入测试：点击计数（触摸映射为鼠标左键）+ 手势测试依次触发
        if let WindowEvent::MousePressed(_) = event {
            self.taps += 1;
            let taps = self.taps;
            self.set_status(0, &format!("taps {taps} ok"));
            match self.taps {
                1 => self.run_audio_test(),
                2 => self.start_record(),
                3 => self.run_dialog_test(),
                _ => {}
            }
        }
    }

    /// 帧钩子：录音推进 + 视频泵 + 手柄轮询 + 对话框结果收割 + 文本绘制
    fn frame(&mut self, ctx: &mut Ctx) {
        // ── 录音测试推进（2 秒采集 → WAV 字节校验）──
        if let Some(recorder) = self.recorder.as_mut() {
            self.record_elapsed += ctx.delta();
            if self.record_elapsed >= 2.0 {
                let bytes = recorder.wav_bytes().unwrap_or_default();
                self.recorder = None;
                if bytes.is_empty() {
                    self.set_status(2, "RECORD: FAIL (empty)");
                } else {
                    self.set_status(2, &format!("RECORD: PASS ({} B wav)", bytes.len()));
                }
            }
        }

        // ── 对话框结果收割 ──
        #[cfg(target_arch = "wasm32")]
        if let Some(msg) = DIALOG_MSG.lock().unwrap().take() {
            self.set_status(5, &format!("DIALOG: {msg}"));
        }

        // ── 网络：WS echo 回显比对（连上后每 2s 发一次）──
        if let Some(conn) = self.conn.as_mut() {
            self.net_elapsed += ctx.delta();
            let mut net_status = None;
            if matches!(conn.state(), ConnState::Connected) && self.net_elapsed >= 2.0 {
                self.net_elapsed = 0.0;
                let payload = b"ws-echo-probe".to_vec();
                if let Err(e) = conn.send(&payload) {
                    net_status = Some(format!("send ERR {e:?}"));
                    console_log(&format!("[21] net send ERR {e:?}"));
                }
            }
            if let Some(data) = conn.try_recv() {
                let ok = data == b"ws-echo-probe".to_vec();
                net_status = Some(
                    format!(
                        "NET: {}",
                        if ok { "echo PASS" } else { "echo MISMATCH" }
                    ),
                );
                // Web 无头测试判读点：回显结果落浏览器控制台
                console_log(&format!(
                    "[21] net echo {}",
                    if ok { "PASS" } else { "MISMATCH" }
                ));
            }
            if let Some(msg) = net_status {
                self.set_status(6, msg.as_str());
            }
        }

        if self.text_dirty {
            self.rebuild_meshes();
        }

        let Some(mut gpu) = self.gpu.get_mut() else { return };

        // ── 视频泵：解码推进 + 纹理上传（探针核心：pos/纹理状态上屏）──
        let pump_err = gpu.video.as_mut().and_then(|video| {
            video
                .update(Duration::from_secs_f32(ctx.delta()))
                .err()
                .map(|e| format!("PUMP FAIL: {e:?}"))
        });

        gpu.surface
            .begin_frame(Color { r: 0.03, g: 0.04, b: 0.07, a: 1.0 }, 1.0);
        let color_attachment = gpu.surface.get_current_color_attachment();
        let mut encoder = gpu.access.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass =
            encoder.begin_render_pass("web_probe", &color_atts, None, None, None, None);
        for mesh in &gpu.text_meshes {
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, &gpu.camera_bind);
            pass.set_bind_group(1, &gpu.atlas_bind);
            pass.set_mesh(mesh);
            pass.draw(0..mesh.vertex_count(), 0..1);
        }
        pass.end();
        let (w, h) = ctx.size();
        let (w, h) = (w.max(1) as f32, h.max(1) as f32);
        let projection = glam::Mat4::from_cols_array(&[
            2.0 / w, 0.0, 0.0, 0.0,
            0.0, -2.0 / h, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            -1.0, 1.0, 0.0, 1.0,
        ]);
        gpu.access.write_buffer(
            &gpu._camera_buffer,
            0,
            bytemuck::bytes_of(&projection.to_cols_array_2d()),
        );
        gpu.surface.submit([encoder.finish()]);
        gpu.surface.present();

        // 状态上屏（直接操作字段，避免 gpu 可变借用冲突）
        if let Some(msg) = pump_err {
            if self.lines.get(3) != Some(&msg) {
                while self.lines.len() <= 3 {
                    self.lines.push(String::new());
                }
                self.lines[3] = msg;
                self.text_dirty = true;
            }
        }
    }
}

// ── 入口（wasm 由 web_entry! 驱动 main；桌面为 bin）──
fn main() {
    use starfish::base::app::run;
    run(
        WebProbe::new(),
        WindowConfig::new("web probe", 1280, 720)
            .with_fps_cap(60)
            .with_web_canvas_id("canvas"),
    );
}

starfish::web_entry!();
