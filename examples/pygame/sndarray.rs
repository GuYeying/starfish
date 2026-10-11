//! pygame.sndarray 运行测试：SoundArray ↔ SoundData 双向桥。
//!
//! 自检项（状态条顺序）：mono 构造 / mono 回环 / stereo 回环 /
//! 非法声道 → Err / 波形渲染（晚登记——桥出的数据可即时绘制）。
//! 场景：mono 正弦波波形条（桥出数组直接可视化）。

#[path = "common.rs"]
mod common;

use common::{draw_status, Checks};
use starfish::pygame::render::DrawTarget;
use starfish::pygame::display;
use starfish::pygame::sndarray::{self, SoundArray};

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    display::set_mode((800, 600)).await?;
    let screen = display::get_screen();
    let mut checks = Checks::new("pygame.sndarray");
    common::log("[sndarray] set_mode 完成，进入自检");

    // mono：256 点正弦，幅度 ±0.25
    let mono: Vec<f32> = (0..256)
        .map(|i| 0.25 * (i as f32 * std::f32::consts::TAU / 64.0).sin())
        .collect();
    let arr = SoundArray::mono(mono.clone(), 22050);
    checks.require("mono 构造", arr.frame_count() == 256 && arr.sample_rate == 22050);

    // 回环：数组 → SoundData → 数组，采样一致（零转码路径）
    let snd = sndarray::make_sound(&arr).expect("make_sound mono 失败");
    let back = sndarray::get_array(&snd);
    let same = back.channels == 1
        && back.sample_rate == 22050
        && back.samples.len() == mono.len()
        && back.samples.iter().zip(&mono).all(|(a, b)| (a - b).abs() < 1e-6);
    checks.require("mono 回环", same);

    // stereo：L/R 交错
    let st = SoundArray::stereo(vec![0.1, -0.1, 0.2, -0.2, 0.3, -0.3], 44100);
    let st_snd = sndarray::make_sound(&st).expect("make_sound stereo 失败");
    let st_back = sndarray::get_array(&st_snd);
    checks.require(
        "stereo 回环",
        st_back.channels == 2 && st_back.samples == st.samples && st_back.frame_count() == 3,
    );

    // 非法声道（字段公开可构造）→ Err
    let bad = SoundArray { sample_rate: 8000, channels: 3, samples: vec![] };
    checks.require("非法声道 → Err", sndarray::make_sound(&bad).is_err());

    let mut frames = 0u32;
    loop {
        if common::quit_requested() {
            return Ok(());
        }
        if frames == 1 {
            checks.require("波形渲染", true);
        }
        {
            let f = screen.render();
            f.fill([30, 30, 46, 255]);
            f.with_batch(|b| {
                // 波形条：256 点 × 2px 间距
                for (i, s) in back.samples.iter().enumerate() {
                    let h = (s.abs() * 300.0).max(2.0);
                    let c = if *s >= 0.0 {
                        [0.3529, 0.7843, 0.4314, 1.0000]
                    } else {
                        [0.9412, 0.7059, 0.2353, 1.0000]
                    };
                    b.push_quad(40.0 + i as f32 * 2.8, 300.0 - h / 2.0, 2.0, h, c, None);
                }
            });
            draw_status(&f, &checks);
            f.end();
        }
        display::flip();
        common::paced_frame().await;
        frames += 1;
        if frames >= common::EXIT_FRAMES {
            break;
        }
    }
    let (passed, total) = checks.summary();
    common::log(&format!("[sndarray] {frames} 帧完成，自检 {passed}/{total}"));
    common::reside().await;
    Ok(())
}

starfish::app_entry!(run());
