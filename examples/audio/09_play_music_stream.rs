//! 流式音乐 + 分组音量演示
//!
//! - BGM 用 `music_load_file` 流式播放（边解码边播，内存占用恒定于环形缓冲）
//! - `music_queue_file` 排队下一首（解码线程预先启动，灌满即 park，无缝衔接）
//! - SFX 用 `load_sound` 整段解码（SoundData），并演示 create_group + set_group_volume
//!
//! 音频设备走 cpal（默认播放设备，设备真实采样率）；
//! 全部播放完成后自然退出（Ctrl+C 亦可随时终止）。
//!
//! 运行：cargo run --example 09_play_music_stream

use std::thread;
use std::time::Duration;

use starfish::base::audio::AudioMixer;

fn main() {
    let mut mixer = AudioMixer::new(8).expect("AudioMixer 创建失败");
    println!("混音器就绪（混音域 = 设备采样率 {}Hz）", mixer.output_sample_rate);

    // ── 短音效：整段解码进内存，放入 "ui" 分组并把组音量压到 0.5 ──
    let sfx = mixer
        .load_sound("resources/audio/powerup.wav")
        .expect("解码 powerup.wav 失败");
    let ui_group = mixer.create_group();
    mixer.set_group_volume(ui_group, 0.5);
    mixer.set_channel_group(0, ui_group).expect("set_channel_group 失败");
    let ch = mixer
        .play_in_group(Some(ui_group), sfx.clone(), 0, 0.0)
        .expect("播放失败")
        .expect("声道忙");
    println!("SFX 在 ui 组（组音量 0.5）声道 {ch} 播放，时长 {:.1}s", sfx.duration());

    // ── 长音频：流式加载 + 排队（解码线程已预起，灌满即 park） ──
    mixer
        .music_load_file("resources/audio/breakout.mp3")
        .expect("流式加载 breakout.mp3 失败");
    mixer
        .music_queue_file("resources/audio/sample-speech-1m.wav")
        .expect("排队 sample-speech-1m.wav 失败");
    mixer.music_play(0);
    mixer.music_fade_in(1000);
    println!("BGM 流式播放中（时长 {:.1}s），已排队下一首", mixer.music_duration());

    let mut faded = false;
    loop {
        // 当前曲目快结束时淡出 → 淡完自动接续排队曲目
        if !faded {
            let remaining = mixer.music_duration() - mixer.music_position();
            if remaining > 0.0 && remaining <= 2.0 {
                println!("淡出，接续下一首...");
                mixer.music_fade_out(2000);
                faded = true;
            }
        }

        if !mixer.music_is_playing() && !mixer.is_channel_busy(ch) {
            println!("全部播放完成，退出");
            break;
        }
        thread::sleep(Duration::from_millis(8));
    }
}
