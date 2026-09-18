//! 流式音乐 + 分组音量演示——**窗口化**（有画面、可返回退出）
//!
//! - BGM 用 `music_load_file` 流式播放（边解码边播，内存占用恒定于环形缓冲）
//! - `music_queue_file` 排队下一首（解码线程预先启动，灌满即 park，无缝衔接）
//! - SFX 用 `load_sound` 整段解码（SoundData），并演示 create_group + set_group_volume
//!
//! 音频设备走 cpal（默认播放设备，设备真实采样率）；
//! 全部播放完成后自动退出；返回键（Android）随时退出。
//!
//! 运行：cargo run --example 09_play_music_stream
//! Android：cargo xtask android 09_play_music_stream
//! （资源装载 cfg 分家：桌面读 resources/，Android 内嵌→私有目录落盘）

use starfish::base::app::{Application, Ctx};
#[cfg(not(target_os = "android"))]
use starfish::base::app::{run, WindowConfig};
#[cfg(target_os = "android")]
use starfish::base::app::{run_android, WindowConfig};
use starfish::base::audio::AudioMixer;
use starfish::base::render::render_entry::RenderEntry;
use starfish::base::render::render_resource_access::RenderResourceAccess;
use starfish::base::render::render_surface::RenderSurface;
use starfish::base::render::RenderContext;
use wgpu::Color;

struct MusicApp {
    _context: Option<RenderContext>,
    resouce: Option<RenderResourceAccess>,
    surface: Option<RenderSurface>,
    /// Ok((混音器, SFX 声道))；Err = 音频链路失败信息
    audio: Result<(AudioMixer, usize), String>,
}

impl MusicApp {
    fn new() -> Self {
        Self {
            _context: None,
            resouce: None,
            surface: None,
            audio: Err("未初始化".into()),
        }
    }
}

impl Application for MusicApp {
    fn start(&mut self, ctx: &mut Ctx) {
        let (context, resouce, surface) =
            RenderEntry::new(ctx.window(), None, None).expect("RenderContext 初始化失败");
        self._context = Some(context);
        self.resouce = Some(resouce);
        self.surface = Some(surface);

        let (sfx_path, bgm_path, next_path) = audio_paths();
        self.audio = setup_audio(&sfx_path, &bgm_path, &next_path);
        if let Err(e) = &self.audio {
            println!("[09] 音频初始化失败: {e}");
        }
    }

    fn frame(&mut self, ctx: &mut Ctx) {
        let clear = match &mut self.audio {
            Ok((mixer, ch)) => {
                if mixer.music_is_playing() || mixer.is_channel_busy(*ch) {
                    Color { r: 0.05, g: 0.25, b: 0.08, a: 1.0 } // 绿=播放中
                } else {
                    println!("[09] 全部播放完成，退出");
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
        let mut pass = encoder.begin_render_pass("music_bg", &color_atts, None, None, None, None);
        pass.end();
        surface.submit([encoder.finish()]);
        surface.present();
    }
}

/// 音频初始化：SFX 整段解码入分组；BGM 流式 + 排队 + 淡入
fn setup_audio(sfx_path: &str, bgm_path: &str, next_path: &str) -> Result<(AudioMixer, usize), String> {
    let mut mixer = AudioMixer::new(8).map_err(|e| format!("AudioMixer 创建失败: {e:?}"))?;
    println!("[09] 混音器就绪（混音域 = 设备采样率 {}Hz）", mixer.output_sample_rate);

    // ── 短音效：整段解码进内存，放入 "ui" 分组并把组音量压到 0.5 ──
    let sfx = mixer
        .load_sound(sfx_path)
        .map_err(|e| format!("解码 {sfx_path} 失败: {e:?}"))?;
    let ui_group = mixer.create_group();
    mixer.set_group_volume(ui_group, 0.5);
    mixer
        .set_channel_group(0, ui_group)
        .map_err(|e| format!("set_channel_group 失败: {e:?}"))?;
    let ch = mixer
        .play_in_group(Some(ui_group), sfx.clone(), 0, 0.0)
        .map_err(|e| format!("播放失败: {e:?}"))?
        .ok_or("声道忙")?;
    println!("[09] SFX 在 ui 组（组音量 0.5）声道 {ch} 播放，时长 {:.1}s", sfx.duration());

    // ── 长音频：流式加载 + 排队（解码线程已预起，灌满即 park） ──
    mixer
        .music_load_file(bgm_path)
        .map_err(|e| format!("流式加载 {bgm_path} 失败: {e:?}"))?;
    mixer
        .music_queue_file(next_path)
        .map_err(|e| format!("排队 {next_path} 失败: {e:?}"))?;
    mixer.music_play(0);
    mixer.music_fade_in(1000);
    println!("[09] BGM 流式播放中（时长 {:.1}s），已排队下一首", mixer.music_duration());
    Ok((mixer, ch))
}

// ── 资源路径：桌面读 resources/；Android 内嵌 → 私有目录落盘 ──
#[cfg(not(target_os = "android"))]
fn audio_paths() -> (String, String, String) {
    (
        "resources/audio/powerup.wav".into(),
        "resources/audio/breakout.mp3".into(),
        "resources/audio/sample-speech-1m.wav".into(),
    )
}

#[cfg(target_os = "android")]
fn audio_paths() -> (String, String, String) {
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
        extract(&dir, "powerup.wav", include_bytes!("../../resources/audio/powerup.wav")),
        extract(&dir, "breakout.mp3", include_bytes!("../../resources/audio/breakout.mp3")),
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
        MusicApp::new(),
        WindowConfig::new("music", 800, 600).with_fps_cap(60),
    );
}

// ── 桌面入口（bin）──
#[cfg(not(target_os = "android"))]
fn main() {
    run(MusicApp::new(), WindowConfig::new("Music Stream", 800, 600).with_fps_cap(60));
}
