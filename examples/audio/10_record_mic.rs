//! 麦克风录音演示——**窗口化**（有画面、可返回退出、失败不闪退）
//!
//! 流程：start 申请麦克风权限（系统弹授权框，阻塞至用户操作/超时）→
//! 录制 5 秒（橙色屏 + 帧累积计时）→ 录完自动弹出**保存对话框**（用户选
//! 可见位置，如 Downloads）→ 写入完成 → 绿屏 → 自动退出。
//!
//! 录音设备走 cpal（设备真实采样率）；保存字节经
//! `AudioRecorder::wav_bytes()` + `dialog::save_bytes_start`（SAF/系统保存框，
//! 轮询式不阻塞事件循环）。
//!
//! 运行：cargo run --example 10_record_mic [秒数]
//! Android：cargo xtask android 10_record_mic

use starfish::base::app::{Application, Ctx, WindowConfig};
#[cfg(not(target_os = "android"))]
use starfish::base::app::run;
#[cfg(target_os = "android")]
use starfish::base::app::run_android;
use starfish::base::audio::AudioRecorder;
use starfish::base::dialog;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use wgpu::Color;

struct RecApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    /// 目标录音秒数（new 时给定）
    secs: u64,
    /// 绿屏停留计时
    done_hold: f32,
    state: RecState,
}

enum RecState {
    /// 录音中：设备句柄 + 已录秒数
    Recording { recorder: AudioRecorder, elapsed: f32 },
    /// 已录完，等待用户在保存对话框选位置
    Saving { job: dialog::SaveJob },
    /// 已保存（绿屏 → 自动退出）
    Saved,
    /// 失败（红屏 + 详情走 println；返回键退出）
    Error(String),
}

impl RecApp {
    fn new(secs: u64) -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            secs: secs.max(1),
            done_hold: 0.0,
            state: RecState::Error("未初始化".into()),
        }
    }
}

impl Application for RecApp {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");
        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);

        // Android：运行时申请麦克风权限（系统弹授权框，阻塞至用户操作/超时）
        #[cfg(target_os = "android")]
        {
            if !dialog::ensure_permission("android.permission.RECORD_AUDIO") {
                println!("[10] RECORD_AUDIO 未授权——请在系统设置中手动授权后重试");
                self.state = RecState::Error(
                    "RECORD_AUDIO denied (grant mic permission in settings, then reopen)".into(),
                );
                return;
            }
        }

        println!("[10] 录音设备：{:?}", AudioRecorder::device_names().map(|n| n.join("|")));
        match AudioRecorder::new_with_capacity(48_000 * (self.secs as usize + 1)) {
            Ok(recorder) => {
                println!("[10] 开始录音 {} 秒（{}Hz）...", self.secs, recorder.sample_rate());
                self.state = RecState::Recording { recorder, elapsed: 0.0 };
            }
            Err(e) => {
                println!("[10] 打开录音设备失败（检查麦克风权限/占用）: {e:?}");
                self.state = RecState::Error(format!("open mic failed: {e:?}"));
            }
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        // 状态推进
        match &mut self.state {
            RecState::Recording { recorder, elapsed } => {
                *elapsed += ctx.delta();
                if *elapsed >= self.secs as f32 {
                    if recorder.dropped() > 0 {
                        println!("[10] ⚠ 溢出丢弃了 {} 帧", recorder.dropped());
                    }
                    let bytes = match recorder.wav_bytes() {
                        Ok(b) => b,
                        Err(e) => {
                            self.state = RecState::Error(format!("wav build failed: {e:?}"));
                            return;
                        }
                    };
                    // 录完 → 弹保存对话框（用户选可见位置，如 Downloads）
                    match dialog::save_bytes_start("recording.wav", bytes) {
                        Ok(job) => self.state = RecState::Saving { job },
                        Err(e) => self.state = RecState::Error(format!("save dialog failed: {e:?}")),
                    }
                }
            }
            RecState::Saving { job } => {
                if let Some(res) = job.try_result() {
                    match res {
                        Ok(Some(path)) => {
                            println!("[10] 已保存 → {}", path.display());
                            self.state = RecState::Saved;
                        }
                        Ok(None) => {
                            println!("[10] 保存已取消");
                            self.state = RecState::Saved; // 取消也走绿屏收尾
                        }
                        Err(e) => {
                            println!("[10] 保存失败: {e:?}");
                            self.state = RecState::Error(format!("save failed: {e:?}"));
                        }
                    }
                }
            }
            RecState::Saved => {
                // 绿屏停留 ~1s 后自动退出
                self.done_hold += ctx.delta();
                if self.done_hold > 1.0 {
                    println!("[10] 完成，退出");
                    ctx.exit();
                }
            }
            RecState::Error(_) => {}
        }

        // 状态色：橙=录音中 / 紫=保存中 / 绿=已保存 / 红=失败
        let clear = match &self.state {
            RecState::Recording { .. } => Color { r: 0.45, g: 0.28, b: 0.02, a: 1.0 },
            RecState::Saving { .. } => Color { r: 0.30, g: 0.10, b: 0.35, a: 1.0 },
            RecState::Saved => Color { r: 0.05, g: 0.30, b: 0.08, a: 1.0 },
            RecState::Error(_) => Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 },
        };

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(resouce) = self.resouce.as_ref() else { return };
        surface.begin_frame(clear, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = resouce.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("rec_bg", &color_atts, None, None, None, None);
        pass.end();
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    run_android(
        app,
        RecApp::new(5),
        WindowConfig::new("record", 800, 600).with_fps_cap(60),
    );
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    run(RecApp::new(secs), WindowConfig::new("Record Mic", 800, 600).with_fps_cap(60));
}
