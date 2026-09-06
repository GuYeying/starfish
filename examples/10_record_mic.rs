//! 麦克风录音演示
//!
//! 录制 N 秒（默认 5 秒，可通过命令行参数覆盖），
//! 保存为 16-bit PCM WAV：recording.wav
//!
//! 运行：cargo run --example 10_record_mic [秒数]

use std::thread;
use std::time::Duration;

use starfish::base::audio::AudioRecorder;
use starfish::base::subsystem::AudioSubsystem;

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let rate = 44100u32;

    let sdl = sdl3::init().expect("sdl3 init error");
    let audio_subsys = AudioSubsystem::new(&sdl);

    match AudioRecorder::device_names(&audio_subsys) {
        Ok(names) if !names.is_empty() => println!("录音设备：{}", names.join(" | ")),
        Ok(_) => println!("未检测到录音设备"),
        Err(e) => println!("枚举录音设备失败：{e}"),
    }

    // 容量一次性给足（秒数 + 1 秒余量），全程无需中途读取
    let mut recorder = AudioRecorder::new_with_capacity(
        &audio_subsys,
        rate,
        rate as usize * (secs as usize + 1),
    )
    .expect("打开录音设备失败（检查麦克风权限/占用）");

    println!("开始录音 {secs} 秒（{rate}Hz 立体声）...");
    for left in (1..=secs).rev() {
        thread::sleep(Duration::from_secs(1));
        println!("  剩余 {left}s");
    }

    if recorder.dropped() > 0 {
        println!("⚠ 溢出丢弃了 {} 帧", recorder.dropped());
    }

    let frames = recorder.save_wav("recording.wav").expect("保存 WAV 失败");
    println!(
        "已保存 recording.wav：{} 帧，{:.1}s",
        frames,
        frames as f32 / rate as f32
    );
}
