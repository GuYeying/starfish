//! 音频测试辅助（仅测试编译）
//!
//! 生成标准测试 WAV（16-bit 单声道 PCM，440Hz 正弦），
//! 供 music / stream / worker 各测试共享。

use std::path::Path;

/// 生成 16-bit 单声道 PCM WAV 字节（440Hz 正弦）
pub(crate) fn sine_wav_bytes(sample_rate: u32, seconds: f32) -> Vec<u8> {
    let n = (sample_rate as f32 * seconds) as usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let s = (t * 440.0 * std::f32::consts::TAU).sin() * 0.5;
        data.extend_from_slice(&((s * i16::MAX as f32) as i16).to_le_bytes());
    }

    let mut wav = Vec::new();
    let data_len = (n * 2) as u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // 单声道
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

/// 写出测试 WAV 到路径
pub(crate) fn write_test_wav(path: &Path, sample_rate: u32, seconds: f32) {
    let bytes = sine_wav_bytes(sample_rate, seconds);
    std::fs::write(path, bytes).unwrap();
}
