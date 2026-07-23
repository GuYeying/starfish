//! 通用解码器（OGG / MP3 / FLAC / WAV）
//!
//! 基于 symphonia 纯 Rust 实现，自动探测格式。

use crate::base::audio::SoundData;
use crate::base::subsystem::audio::common::AudioError;

/// 通用解码器
pub struct Decoder;

impl Decoder {
    /// 从文件路径自动探测并解码为 SoundData
    ///
    /// 支持格式：OGG、MP3、FLAC、WAV（自动根据文件内容判断，不依赖扩展名）
    pub fn from_file(path: &str) -> Result<SoundData, AudioError> {
        let file = std::fs::File::open(path)
            .map_err(|e| AudioError::custom(format!("打开音频文件失败 {path}: {e}")))?;

        let mss = symphonia::core::io::MediaSourceStream::new(
            Box::new(file),
            Default::default(),
        );

        let hint = symphonia::core::probe::Hint::new();
        // 不设置扩展名 hint，让 symphonia 自动探测

        let probe = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &symphonia::core::formats::FormatOptions::default(),
                &symphonia::core::meta::MetadataOptions::default(),
            )
            .map_err(|e| {
                AudioError::custom(format!("symphonia 格式探测失败 {path}: {e}"))
            })?;

        let format = probe.format;

        // 找第一个有音频流的轨
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::custom("音频文件内没有可解码的音轨"))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let sample_rate = codec_params.sample_rate.unwrap_or(44100);
        let num_channels =
            codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let mut codec = symphonia::default::get_codecs()
            .make(&codec_params, &Default::default())
            .map_err(|e| AudioError::custom(format!("symphonia 创建解码器失败: {e}")))?;

        // 逐 packet 解码，收集 f32 采样
        let mut all_samples: Vec<f32> = Vec::new();
        let mut format = format;

        loop {
            let packet = match format.next_packet() {
                Ok(pkt) => pkt,
                Err(symphonia::core::errors::Error::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => {
                    return Err(AudioError::custom(format!("symphonia 解码错误: {e}")));
                }
            };

            if packet.track_id() != track_id {
                continue;
            }

            let decoded = codec
                .decode(&packet)
                .map_err(|e| AudioError::custom(format!("symphonia decode packet 失败: {e}")))?;

            let spec = *decoded.spec();
            let frames = decoded.frames();

            // 用 SampleBuffer 自动转换任意采样格式为 f32 交错
            use symphonia::core::audio::SampleBuffer;
            let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
            sample_buf.copy_interleaved_ref(decoded);
            let chunk = sample_buf.samples();

            if num_channels == 1 {
                // 单声道
                all_samples.extend_from_slice(chunk);
            } else {
                // 立体声或多声道：取前两个声道
                for f in 0..frames {
                    let idx = f * spec.channels.count();
                    all_samples.push(chunk[idx]); // L
                    all_samples.push(if spec.channels.count() > 1 {
                        chunk[idx + 1]            // R
                    } else {
                        chunk[idx]                // 复制 L
                    });
                }
            }
        }

        if all_samples.is_empty() {
            return Err(AudioError::custom("解码结果为空"));
        }

        if num_channels == 1 {
            Ok(SoundData::from_mono_f32(&all_samples, sample_rate))
        } else {
            Ok(SoundData::from_interleaved_f32(&all_samples, sample_rate))
        }
    }
}
