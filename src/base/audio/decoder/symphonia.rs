//! 通用解码器（OGG / MP3 / FLAC / WAV）
//!
//! 基于 symphonia 纯 Rust 实现，自动探测格式。
//! 两种消费方式：
//!   - [`Decoder::from_file`]：一次性解码进内存（短音效）
//!   - [`SymphoniaReader`]：逐包拉取（流式播放的解码前端）

use crate::base::audio::SoundData;
use crate::base::subsystem::audio::common::AudioError;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::CODEC_TYPE_NULL;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// 逐包解码读取器（流式播放的解码前端）
///
/// 持有 format reader + codec，[`next_interleaved`](Self::next_interleaved)
/// 每次拉取一个包并转为**源采样率**的 f32 交错采样（多声道取前两个声道）。
pub struct SymphoniaReader {
    format: Box<dyn FormatReader>,
    codec: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    src_rate: u32,
    src_channels: usize,
    /// 容器声明的总帧数（部分格式不提供 → None）
    total_frames: Option<u64>,
}

impl SymphoniaReader {
    /// 打开文件并探测格式
    pub fn open(path: &str) -> Result<Self, AudioError> {
        let file = std::fs::File::open(path)
            .map_err(|e| AudioError::custom(format!("打开音频文件失败 {path}: {e}")))?;

        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let probe = symphonia::default::get_probe()
            .format(
                &Hint::new(),
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| {
                AudioError::custom(format!("symphonia 格式探测失败 {path}: {e}"))
            })?;
        let format = probe.format;

        // 找第一个有音频流的轨
        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| AudioError::custom("音频文件内没有可解码的音轨"))?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let src_rate = codec_params.sample_rate.unwrap_or(44100);
        let src_channels = codec_params.channels.map(|c| c.count()).unwrap_or(2);

        let codec = symphonia::default::get_codecs()
            .make(&codec_params, &Default::default())
            .map_err(|e| AudioError::custom(format!("symphonia 创建解码器失败: {e}")))?;

        Ok(Self {
            format,
            codec,
            track_id,
            src_rate,
            src_channels,
            total_frames: codec_params.n_frames,
        })
    }

    /// 源采样率（Hz）
    pub fn src_rate(&self) -> u32 {
        self.src_rate
    }

    /// 源声道数
    pub fn src_channels(&self) -> usize {
        self.src_channels
    }

    /// 容器声明的总时长（秒）；格式未提供元数据则为 None
    pub fn duration(&self) -> Option<f32> {
        self.total_frames
            .map(|n| n as f32 / self.src_rate as f32)
    }

    /// 拉取下一个包，转为源采样率的 f32 交错采样
    ///
    /// 返回 `None` 表示流结束；非目标轨的包返回空 `Vec`（跳过）。
    pub fn next_interleaved(&mut self) -> Result<Option<Vec<f32>>, AudioError> {
        let packet = match self.format.next_packet() {
            Ok(pkt) => pkt,
            // 正常 EOF 与部分 OGG 流的非正常 EOF 都视为流结束
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None);
            }
            Err(e) => {
                return Err(AudioError::custom(format!("symphonia 解码错误: {e}")));
            }
        };

        if packet.track_id() != self.track_id {
            return Ok(Some(Vec::new()));
        }

        let decoded = self
            .codec
            .decode(&packet)
            .map_err(|e| AudioError::custom(format!("symphonia decode packet 失败: {e}")))?;

        let spec = *decoded.spec();
        let frames = decoded.frames();

        // 用 SampleBuffer 自动转换任意采样格式为 f32 交错
        let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let chunk = sample_buf.samples();

        let mut out = Vec::with_capacity(frames * 2);
        if self.src_channels == 1 {
            out.extend_from_slice(chunk);
        } else {
            for f in 0..frames {
                let idx = f * spec.channels.count();
                out.push(chunk[idx]); // L
                out.push(if spec.channels.count() > 1 {
                    chunk[idx + 1] // R
                } else {
                    chunk[idx] // 复制 L
                });
            }
        }
        Ok(Some(out))
    }

    /// 跳转到指定秒数（sample-accurate，实际落在目标位置之前最近的可 seek 点）
    pub fn seek(&mut self, seconds: f32) -> Result<(), AudioError> {
        self.format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(seconds),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| AudioError::custom(format!("symphonia seek 失败: {e}")))?;
        Ok(())
    }
}

/// 通用解码器（一次性解码进内存）
pub struct Decoder;

impl Decoder {
    /// 从文件路径自动探测并解码为 SoundData
    ///
    /// 支持格式：OGG、MP3、FLAC、WAV（自动根据文件内容判断，不依赖扩展名）
    pub fn from_file(path: &str) -> Result<SoundData, AudioError> {
        let mut reader = SymphoniaReader::open(path)?;
        let mut all_samples: Vec<f32> = Vec::new();

        loop {
            match reader.next_interleaved()? {
                Some(chunk) => all_samples.extend_from_slice(&chunk),
                None => break,
            }
        }

        if all_samples.is_empty() {
            return Err(AudioError::custom("解码结果为空"));
        }

        if reader.src_channels() == 1 {
            Ok(SoundData::from_mono_f32(&all_samples, reader.src_rate()))
        } else {
            Ok(SoundData::from_interleaved_f32(
                &all_samples,
                reader.src_rate(),
            ))
        }
    }
}
