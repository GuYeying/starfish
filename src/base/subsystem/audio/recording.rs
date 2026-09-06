use super::common::samples_to_frames;

use super::common::{AudioError, AudioUserCallback, StereoFrame};
use sdl3::audio::{ AudioRecordingCallback, AudioSpec, AudioStream, AudioStreamWithCallback};
use sdl3::AudioSubsystem;

/// 栈上临时帧缓冲区，和播放保持一致 8KB
const STACK_FRAME_BUF: usize = 1024;

/// 录音回调结构体，与PlaybackCallback架构完全统一
pub struct RecordingCallback {
    user_cb: Box<dyn AudioUserCallback + Send>,

    sample_buffer: Vec<f32>,

    frame_buf: [StereoFrame; STACK_FRAME_BUF],
}

impl RecordingCallback {
    pub fn new(
        user_cb: impl AudioUserCallback + Send + 'static
    ) -> Self {
        Self {
            user_cb: Box::new(user_cb),

            sample_buffer: vec![0.0; STACK_FRAME_BUF * 2],

            frame_buf: [StereoFrame::SILENT; STACK_FRAME_BUF],
        }
    }
}


impl AudioRecordingCallback<f32> for RecordingCallback {
    fn callback(
        &mut self,
        stream: &mut AudioStream,
        additional_amount: i32,
    ) {
        let sample_count =
            (additional_amount as usize)
                .min(STACK_FRAME_BUF * 2);

        let samples =
            &mut self.sample_buffer[..sample_count];

        let samples_read =
            match stream.read_f32_samples(samples) {
                Ok(n) => n,
                Err(_) => return,
            };

        let frame_count =
            (samples_read / 2)
                .min(STACK_FRAME_BUF);

        let frames =
            &mut self.frame_buf[..frame_count];

        let _ = samples_to_frames(
            &samples[..frame_count * 2],
            frames,
        );

        self.user_cb.on_frames(frames);
    }
}

/// 打开录音流，和open_playback_stream接口完全对称
pub fn open_recording_stream<CB>(
    subsystem: &AudioSubsystem,
    spec: &AudioSpec,
    user_cb: CB,
) -> Result<AudioStreamWithCallback<RecordingCallback>, AudioError>
where
    CB: AudioUserCallback + Send + 'static,
{

    // 校验声道必须立体声2
    if spec.channels != Some(2) {
        return Err(AudioError::UnsupportedFormat);
    }
    // 修改：兼容F32LE / F32BE，不再卡死大端
    match spec.format {
        Some(sdl3::audio::AudioFormat::F32LE) | Some(sdl3::audio::AudioFormat::F32BE) => {}//直接删掉 F32BE 分支，只保留 F32LE 即可一劳永逸
        _ => return Err(AudioError::UnsupportedFormat),
    }

    // 构造回调结构体，内部持有用户回调、配置、预分配缓冲区
    let callback = RecordingCallback::new(user_cb);

    // 传入设备、规格、结构体回调（自动满足AudioRecordingCallback trait）
    let stream = subsystem.open_recording_stream(spec, callback)?;

    // 不再使用set_userdata、不再操作全局静态变量
    Ok(stream)
}