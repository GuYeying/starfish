//! 麦克风录音演示
//!
//! 录制 N 秒（默认 5 秒，可通过命令行参数覆盖），
//! 保存为 16-bit PCM WAV：recording.wav
//!
//! 录音设备走 cpal（默认录音设备，设备真实采样率，WAV 头据此写出）。
//!
//! 运行：cargo run --example 10_record_mic [秒数]

use std::thread;
use std::time::Duration;

use starfish::base::audio::AudioRecorder;

fn main() {
    let secs: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);

    match AudioRecorder::device_names() {
        Ok(names) if !names.is_empty() => println!("录音设备：{}", names.join(" | ")),
        Ok(_) => println!("未检测到录音设备"),
        Err(e) => println!("枚举录音设备失败：{e}"),
    }

    // 容量一次性给足（按 48kHz 估秒数 + 1 秒余量，只需量级正确），全程无需中途读取
    let mut recorder = AudioRecorder::new_with_capacity(48_000 * (secs as usize + 1))
        .expect("打开录音设备失败（检查麦克风权限/占用）");
    let rate = recorder.sample_rate();

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
