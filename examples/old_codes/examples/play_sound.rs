use std::sync::Arc;
use std::thread;
use std::time::Duration;
use starfish::base::audio::sfx::AudioEffect;
use starfish::base::audio::decoder::SymphoniaDecoder;
use starfish::base::audio::AudioEngine;
use starfish::base::subsystem::audio::common::StereoFrame;
use starfish::base::subsystem::AudioSubsystem;

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
// 主程序
// ============================================================================

fn main() {
    let sfx_path = "resources/audio/sample-3s.wav";
    let bgm_path = "resources/audio/sample-speech-1m.wav";
    let sdl = sdl3::init().expect("sdl3 init error");
    let audio_subsys = AudioSubsystem::new(&sdl);

    // ── 创建引擎（只需传采样率，格式固定 F32LE 立体声） ──
    let sample_rate = 44100;
    let mut engine = AudioEngine::new(&audio_subsys, sample_rate, 8)
        .expect("AudioEngine 创建失败");

    // ── 解码音频 ──
    let sfx = SymphoniaDecoder::from_file(sfx_path).expect("sample-3s.wav 解码失败");
    let bgm = SymphoniaDecoder::from_file(bgm_path).expect("sample-speech-1m.wav 解码失败");

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

    // ── 为 SFX 添加回声效果器（80ms 延时，20% 反馈） ──
    // SFX 将通过 play_with 的 fade_in_ms 参数淡入
    let sfx_ch = engine
        .play_with(sfx.clone(), 0, 500.0)
        .expect("SFX 播放失败")
        .expect("所有声道繁忙，SFX 无法播放");
    engine.channel_fade_out(sfx_ch, 500); // 到末尾会自动停止

    // ── 为 BGM 添加回声效果器（150ms 延时，30% 反馈） ──
    engine.music_load(bgm.clone());
    engine.music_play(0);
    engine.music_fade_in(2000);

    // BGM 添加回声效果
    engine.music_add_effect(Box::new(EchoDelay::new(150, 0.3, sample_rate)));

    let _ = engine.sfx_add_effect(sfx_ch, Box::new(EchoDelay::new(150, 0.3, sample_rate)));

    // 也可以给 SFX 的某个声道添加独立效果器：
    // 直接在 SfxChannel 上操作 — 暂不演示

    engine.set_master_volume(1.0);
    engine.set_sfx_volume(0.8);
    engine.set_channel_volume(0, 0.5);

    if let Some(sound) = engine.get_channel_sound(0) {
        println!(
            "声道 0 播放音频：{:.1}s，{}Hz{}",
            sound.duration(),
            sound.sample_rate,
            if sound.frame_count() > 0 { " ✅" } else { " ⚠️ 无数据" },
        );
    }

    println!("播放中（SFX→回声80ms/20%，BGM→回声150ms/30%）");
    println!("SFX 和 BGM 结束后自动退出...");

    let mut event_pump = sdl.event_pump().unwrap();
    let mut bgm_fadeout_started = false;

    'main_loop: loop {
        for event in event_pump.poll_iter() {
            if let sdl3::event::Event::Quit { .. } = event {
                break 'main_loop;
            }
        }

        // BGM 快结束时淡出
        if !bgm_fadeout_started {
            let remaining = engine.music_duration() - engine.music_position();
            if remaining <= 3.0 {
                println!("BGM 淡出...");
                engine.music_fade_out(3000);
                bgm_fadeout_started = true;
            }
        }

        // 都播完则退出
        let sfx_busy = engine.is_channel_busy(sfx_ch);
        let music_busy = engine.music_is_playing();
        if !sfx_busy && !music_busy {
            println!("全部播放完成，程序退出");
            break;
        }
        thread::sleep(Duration::from_millis(8));
    }
    println!("停止播放，程序退出");
}
