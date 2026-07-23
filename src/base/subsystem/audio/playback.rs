


use super::common::{
    AudioUserCallback,
    StereoFrame,
};

use sdl3::audio::{AudioCallback, AudioStream};

const STACK_FRAME_BUF: usize = 1024; // 栈帧缓冲区，8KB，安全无压力

pub struct PlaybackCallback {
    user_cb: Box<dyn AudioUserCallback + Send>,
    frame_buffer: [StereoFrame; STACK_FRAME_BUF],
}

impl PlaybackCallback {
    pub fn new(
        user_cb: impl AudioUserCallback + Send + 'static,
    ) -> Self {
        Self {
            user_cb: Box::new(user_cb),
            frame_buffer: [StereoFrame::SILENT; STACK_FRAME_BUF],
        }
    }
}

impl AudioCallback<f32> for PlaybackCallback {
    fn callback(
        &mut self,
        stream: &mut AudioStream,
        additional_amount: i32,
    ) {
        let sample_count =
            (additional_amount as usize)
                .min(STACK_FRAME_BUF * 2);

        let frame_count =
            (sample_count / 2)
                .min(STACK_FRAME_BUF);

        let frames =
            &mut self.frame_buffer[..frame_count];

        frames.fill(StereoFrame::SILENT);

        self.user_cb.on_frames(frames);

        // StereoFrame 和 [f32; 2] 内存布局完全一致，
        // 直接用 bytemuck 零拷贝 reinterpret 为 &[f32] 送给 SDL
        let samples: &[f32] = bytemuck::cast_slice(frames);
        let _ = stream.put_data_f32(samples);
    }
}


// pub fn open_playback_stream<CB>(
//     subsystem: &AudioSubsystem,
//     spec: &AudioSpec,
//     user_cb: CB,
// ) -> Result<
//     AudioStreamWithCallback<PlaybackCallback>,
//     AudioError,
// >
// where
//     CB: AudioUserCallback + Send + 'static,
// {
//     // 校验声道必须立体声2
//     if spec.channels != Some(2) {
//         return Err(AudioError::UnsupportedFormat);
//     }
//     // 修改：兼容F32LE / F32BE，不再卡死大端
//     match spec.format {
//         Some(sdl3::audio::AudioFormat::F32LE) | Some(sdl3::audio::AudioFormat::F32BE) => {}//直接删掉 F32BE 分支，只保留 F32LE 即可一劳永逸
//         _ => return Err(AudioError::UnsupportedFormat),
//     }

//     let device = subsystem.default_playback_device();

//     // 用new构造，内部自动预分配byte_buffer，替代原来手动结构体+todo
//     let callback = PlaybackCallback::new(user_cb);

//     let stream = subsystem.open_playback_stream_with_callback(&device, spec, callback)?;

//     Ok(stream)
// }









// use std::collections::VecDeque;

// /// 承载完整长音频，一次性加载进内存，自动续帧播放
// pub struct LongAudioPlayer {
//     frame_queue: VecDeque<StereoFrame>,
// }

// impl LongAudioPlayer {
//     /// 从交错f32 PCM载入完整音乐
//     pub fn new(interleaved_f32: &[f32]) -> Result<Self, AudioError> {
//         let mut frames = vec![StereoFrame::SILENT; interleaved_f32.len() / 2];
//         crate::lingyu::core::subsystem::audio::common::samples_to_frames(interleaved_f32, &mut frames)?;
//         Ok(Self {
//             frame_queue: VecDeque::from(frames),
//         })
//     }

//     /// 是否播放完毕
//     pub fn finished(&self) -> bool {
//         self.frame_queue.is_empty()
//     }
// }

// // 实现回调：SDL要多少帧，自动从队列取，队列空就静音
// impl AudioUserCallback for LongAudioPlayer {
//     fn on_frames(&mut self, out_frames: &mut [StereoFrame]) {
//         for frame in out_frames {
//             *frame = self.frame_queue.pop_front().unwrap_or(StereoFrame::SILENT);
//         }
//     }
// }
