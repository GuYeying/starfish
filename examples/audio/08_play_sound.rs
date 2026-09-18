//! 音频播放演示：SFX + BGM + 自定义效果器（回声延时）——**窗口化**（有画面、可返回退出）
//!
//! 循环模型：引擎持循环（`run`/`run_android`），应用实现 `Application` 三回调：
//! start 创建混音器并启动播放 → frame 显示状态（绿=播放中 / 红=失败），
//! 播放完成自动退出；返回键（Android）随时退出。
//!
//! 效果器：SFX→回声 80ms/20%，BGM→回声 150ms/30%。
//!
//! 运行：cargo run --example 08_play_sound
//! Android：cargo xtask android 08_play_sound
//! （资源装载 cfg 分家：桌面读 resources/，Android 内嵌→私有目录落盘）

use std::sync::Arc;

use starfish::base::app::{Application, Ctx};
#[cfg(not(target_os = "android"))]
use starfish::base::app::{run, WindowConfig};
#[cfg(target_os = "android")]
use starfish::base::app::{run_android, WindowConfig};
use starfish::base::audio::sfx::AudioEffect;
use starfish::base::audio::decoder::SymphoniaDecoder;
use starfish::base::audio::AudioMixer;
use starfish::base::audio::common::StereoFrame;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use wgpu::Color;

// ============================================================================
// 示例效果器：回声延时
// ============================================================================
struct EchoDelay {
    /// 环形缓冲区
    buffer: Vec<StereoFrame>,
    /// 当前写入位置
    pos: usize,
    /// 延时帧数（如 0.05s × 44100Hz ≈ 2205 帧）
    delay_frames: usize,
    /// 反馈系数（0.0 ~ 1.0），越大回声越长
    feedback: f32,
}

impl EchoDelay {
    /// 创建回声效果器
    ///
    /// * `delay_ms`: 延时毫秒，如 80ms = 短回声，200ms = 长回声
    /// * `feedback`: 反馈量，建议 0.2~0.5，太大则回声无限循环
    /// * `sample_rate`: 采样率，用于将毫秒转为帧数
    fn new(delay_ms: u32, feedback: f32, sample_rate: u32) -> Self {
        let delay_frames = (delay_ms as u64 * sample_rate as u64 / 1000) as usize;
        Self {
            buffer: vec![StereoFrame::SILENT; delay_frames.max(1)],
            pos: 0,
            delay_frames: delay_frames.max(1),
            feedback: feedback.clamp(0.0, 0.95),
        }
    }
}

impl AudioEffect for EchoDelay {
    fn name(&self) -> &str {
        "echo"
    }

    fn process(&mut self, frames: &mut [StereoFrame]) {
        for frame in frames.iter_mut() {
            // 读出当前延迟位置的值
            let delayed = self.buffer[self.pos];

            // 写入当前采样（覆盖旧的延迟数据）
            self.buffer[self.pos] = *frame;

            // 叠加延迟信号到原信号
            frame.left += delayed.left * self.feedback;
            frame.right += delayed.right * self.feedback;

            // 推进环形缓冲区指针
            self.pos += 1;
            if self.pos >= self.delay_frames {
                self.pos = 0;
            }
        }
    }
}

// ============================================================================
// 窗口化应用：混音状态上屏（绿=播放中，红=失败），播完自动退出
// ============================================================================

struct AudioApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    /// Ok((混音器, SFX 声道))；Err = 音频链路失败信息
    audio: Result<(AudioMixer, usize), String>,
}

impl AudioApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            audio: Err("未初始化".into()),
        }
    }
}

impl Application for AudioApp {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");
        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);

        let (sfx_path, bgm_path) = audio_paths();
        self.audio = setup_audio(&sfx_path, &bgm_path);
        if let Err(e) = &self.audio {
            println!("[08] 音频初始化失败: {e}");
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let clear = match &mut self.audio {
            Ok((mixer, ch)) => {
                if mixer.is_channel_busy(*ch) || mixer.music_is_playing() {
                    Color { r: 0.05, g: 0.25, b: 0.08, a: 1.0 } // 绿=播放中
                } else {
                    println!("[08] 播放完成，退出");
                    ctx.exit();
                    Color::BLACK
                }
            }
            Err(_) => Color { r: 0.40, g: 0.05, b: 0.05, a: 1.0 }, // 红=失败
        };

        let Some(surface) = self.surface.as_mut() else { return };
        let Some(resouce) = self.resouce.as_ref() else { return };
        surface.begin_frame(clear, 1.0);
        let color_attachment = surface.get_current_color_attachment();
        let mut encoder = resouce.create_command_encoder();
        let color_atts = [&color_attachment];
        let mut pass = encoder.begin_render_pass("audio_bg", &color_atts, None, None, None, None);
        pass.end();
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

/// 音频初始化：解码 → 采样率适配 → SFX/BGM 播放 + 效果器挂载
fn setup_audio(sfx_path: &str, bgm_path: &str) -> Result<(AudioMixer, usize), String> {
    // ── 创建引擎（cpal 默认播放设备，混音域 = 设备真实采样率） ──
    let mut engine = AudioMixer::new(8).map_err(|e| format!("AudioMixer 创建失败: {e:?}"))?;
    let sample_rate = engine.output_sample_rate;

    // ── 解码音频 ──
    let sfx = SymphoniaDecoder::from_file(sfx_path).map_err(|e| format!("{sfx_path} 解码失败: {e:?}"))?;
    let bgm = SymphoniaDecoder::from_file(bgm_path).map_err(|e| format!("{bgm_path} 解码失败: {e:?}"))?;

    // 采样率适配
    let sfx = if sfx.sample_rate != engine.output_sample_rate {
        Arc::new(sfx.resample(engine.output_sample_rate))
    } else {
        Arc::new(sfx)
    };
    let bgm = if bgm.sample_rate != engine.output_sample_rate {
        Arc::new(bgm.resample(engine.output_sample_rate))
    } else {
        Arc::new(bgm)
    };

    // ── SFX 播放（淡入 500ms） ──
    let sfx_ch = engine
        .play_with(sfx, 0, 500.0)
        .map_err(|e| format!("SFX 播放失败: {e:?}"))?
        .ok_or("所有声道繁忙，SFX 无法播放")?;
    engine.channel_fade_out(sfx_ch, 500);

    // ── BGM 播放（淡入 2000ms）+ 回声 ──
    engine.music_load(bgm);
    engine.music_play(0);
    engine.music_fade_in(2000);
    engine.music_add_effect(Box::new(EchoDelay::new(150, 0.3, sample_rate)));
    let _ = engine.sfx_add_effect(sfx_ch, Box::new(EchoDelay::new(150, 0.3, sample_rate)));

    engine.set_master_volume(1.0);
    engine.set_sfx_volume(0.8);
    engine.set_channel_volume(0, 0.5);

    println!("[08] 播放中（SFX→回声80ms/20%，BGM→回声150ms/30%），完成或返回键退出");
    Ok((engine, sfx_ch))
}

// ── 资源路径：桌面读 resources/；Android 内嵌 → 私有目录落盘 ──
#[cfg(not(target_os = "android"))]
fn audio_paths() -> (String, String) {
    (
        "resources/audio/sample-3s.wav".into(),
        "resources/audio/sample-speech-1m.wav".into(),
    )
}

#[cfg(target_os = "android")]
fn audio_paths() -> (String, String) {
    fn extract(dir: &str, name: &str, bytes: &[u8]) -> String {
        std::fs::write(format!("{dir}/{name}"), bytes).expect("私有目录写入失败");
        format!("{dir}/{name}")
    }
    let dir = DATA_DIR
        .lock()
        .unwrap()
        .clone()
        .expect("DATA_DIR 未初始化（android_main 未先执行）");
    (
        extract(&dir, "sample-3s.wav", include_bytes!("../../resources/audio/sample-3s.wav")),
        extract(&dir, "sample-speech-1m.wav", include_bytes!("../../resources/audio/sample-speech-1m.wav")),
    )
}

// Android 应用私有目录：android_main 注入，audio_paths() 消费（文件级单例）
#[cfg(target_os = "android")]
static DATA_DIR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

// ── Android 入口（cdylib）──
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    {
        let mut dir = DATA_DIR.lock().unwrap();
        *dir = app
            .internal_data_path()
            .map(|p| p.to_string_lossy().into_owned());
    }
    run_android(
        app,
        AudioApp::new(),
        WindowConfig::new("audio", 800, 600).with_fps_cap(60),
    );
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    run(AudioApp::new(), WindowConfig::new("Play Sound", 800, 600).with_fps_cap(60));
}
