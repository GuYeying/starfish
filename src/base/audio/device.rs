//! 音频设备层：cpal 胶水（原 SDL 设备层的退役替代）
//!
//! 职责收敛：默认设备获取、格式协商（f32 立体声）、输出/输入流构建、设备枚举。
//! 上层（[`AudioMixer`](super::AudioMixer) / [`AudioRecorder`](super::AudioRecorder)）
//! 只接触本模块的 [`DeviceStream`]，**不接触任何 cpal 类型**——
//! 音频后端的可替换性由此文件独自承担（对应渲染层的 render_entry 边界）。
//!
//! 与 SDL 的语义差异（迁移记录，见 doc/log）：
//! - SDL 在设备边界做任意规格转换（请求 44100 → 设备内部转）；cpal 无此层，
//!   **混音域 = 设备真实采样率**。采样率适配交给数据侧：
//!   [`SoundData::resample`](super::SoundData::resample)（SFX，加载/播放时一次性）
//!   与 MusicStream 的 Resampler（流式，本来就按目标率重采样）。
//! - cpal 回调直接给出待填充/待读取缓冲，无需 SDL 的
//!   `additional_amount` / `put_data` 拉取协议。
//! - v1 格式约束：只协商 **f32 交错立体声**（优先设备默认配置，
//!   否则在支持列表里取最高采样率）；不满足则报 [`AudioError::UnsupportedFormat`]。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use super::common::{AudioError, AudioUserCallback, StereoFrame};

/// 设备端实际生效的流规格（打开成功后由设备值回填）
#[derive(Debug, Clone, Copy)]
pub struct StreamSpec {
    /// 设备真实采样率（Hz）——混音域基准
    pub sample_rate: u32,
    /// 声道数（v1 恒为 2）
    pub channels: u16,
}

/// 打开的设备流（Drop 即停止并释放设备）
///
/// cpal 0.17+ 的 `Stream` 为 Send+Sync，本结构可安全跨线程持有
///（对应 free-threaded 线程契约：混音器/录音器可从任意线程创建与销毁）。
pub struct DeviceStream {
    stream: cpal::Stream,
    pub spec: StreamSpec,
}

impl DeviceStream {
    /// 暂停流（设备保持打开，回调停发）
    pub fn pause(&self) -> Result<(), AudioError> {
        self.stream
            .pause()
            .map_err(|e| AudioError::Device(format!("流 pause 失败: {e}")))
    }

    /// 恢复流
    pub fn resume(&self) -> Result<(), AudioError> {
        self.stream
            .play()
            .map_err(|e| AudioError::Device(format!("流 resume 失败: {e}")))
    }
}

/// 打开默认播放设备：数据面 = 用户回调周期性填充立体声帧（构造即播放）
pub fn open_output_stream(
    user_cb: impl AudioUserCallback + Send + 'static,
) -> Result<DeviceStream, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| AudioError::Device("没有可用的默认播放设备".into()))?;
    let config = negotiate_stereo_f32(
        device.default_output_config().ok(),
        device.supported_output_configs(),
    )?;

    let sample_rate = config.sample_rate;
    let channels = config.channels;

    let mut user_cb = user_cb;
    let err_cb = move |e| eprintln!("[starfish-audio] 播放流错误: {e}");
    let data_cb = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        fill_interleaved(data, channels, |frames| user_cb.on_frames(frames));
    };

    let stream = device
        .build_output_stream(config, data_cb, err_cb, None)
        .map_err(|e| AudioError::Device(format!("播放流打开失败: {e}")))?;
    stream
        .play()
        .map_err(|e| AudioError::Device(format!("播放流启动失败: {e}")))?;

    Ok(DeviceStream {
        stream,
        spec: StreamSpec { sample_rate, channels },
    })
}

/// 打开默认录音设备：数据面 = 用户回调周期性收到采集帧（构造即采集）
pub fn open_input_stream(
    user_cb: impl AudioUserCallback + Send + 'static,
) -> Result<DeviceStream, AudioError> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| AudioError::Device("没有可用的默认录音设备".into()))?;
    let config = negotiate_stereo_f32(
        device.default_input_config().ok(),
        device.supported_input_configs(),
    )?;

    let channels = config.channels;
    let sample_rate = config.sample_rate;

    let mut user_cb = user_cb;
    // cpal 输入侧给的是只读采样，而 AudioUserCallback 约定 &mut [StereoFrame]
    //（录音路径要就地处理）；复用一块暂存缓冲做中转，音频线程内零重复分配
    let mut staging: Vec<StereoFrame> = Vec::new();
    let err_cb = move |e| eprintln!("[starfish-audio] 录音流错误: {e}");
    let data_cb = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        if channels != 2 {
            return;
        }
        let n = data.len() - data.len() % 2;
        staging.clear();
        staging.extend_from_slice(bytemuck::cast_slice::<f32, StereoFrame>(&data[..n]));
        user_cb.on_frames(&mut staging);
    };

    let stream = device
        .build_input_stream(config, data_cb, err_cb, None)
        .map_err(|e| AudioError::Device(format!("录音流打开失败: {e}")))?;
    stream
        .play()
        .map_err(|e| AudioError::Device(format!("录音流启动失败: {e}")))?;

    Ok(DeviceStream {
        stream,
        spec: StreamSpec { sample_rate, channels },
    })
}

/// 枚举播放设备名（诊断用）
pub fn output_device_names() -> Result<Vec<String>, AudioError> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|e| AudioError::Device(format!("枚举播放设备失败: {e}")))?;
    Ok(devices.map(device_name).collect())
}

/// 枚举录音设备名（诊断用）
pub fn input_device_names() -> Result<Vec<String>, AudioError> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| AudioError::Device(format!("枚举录音设备失败: {e}")))?;
    Ok(devices.map(device_name).collect())
}

/// 设备名（0.18 起 name() 并入 description()）
fn device_name(d: cpal::Device) -> String {
    d.description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| "未知设备".into())
}

// ───────────────────────────────── 内部 ─────────────────────────────────

/// 把交错 f32 缓冲以 [`StereoFrame`] 视角交给用户回调
///
/// `#[repr(C) {f32, f32}]` 与交错立体声 f32 布局一致，bytemuck 零拷贝。
/// 声道数非 2 时静音兜底（协商层已保证 v1 恒为立体声，此为防御分支）。
#[inline]
fn fill_interleaved(data: &mut [f32], channels: u16, f: impl FnOnce(&mut [StereoFrame])) {
    if channels == 2 {
        let n = data.len() - data.len() % 2;
        f(bytemuck::cast_slice_mut::<f32, StereoFrame>(&mut data[..n]));
    } else {
        data.fill(0.0);
    }
}

/// 格式协商：优先设备默认配置（f32 立体声命中即用），
/// 否则扫描支持列表取最高采样率的 f32 立体声档位
fn negotiate_stereo_f32(
    default: Option<cpal::SupportedStreamConfig>,
    enumerate: Result<impl Iterator<Item = cpal::SupportedStreamConfigRange>, cpal::Error>,
) -> Result<StreamConfig, AudioError> {
    if let Some(d) = default.into_iter().filter(is_stereo_f32).next() {
        return Ok(d.into());
    }
    enumerate
        .map_err(|e| AudioError::Device(format!("枚举设备配置失败: {e}")))?
        .filter(|c| is_stereo_f32_range(c))
        .max_by_key(|c| c.max_sample_rate())
        .map(|c| c.with_max_sample_rate().into())
        .ok_or_else(|| {
            AudioError::UnsupportedFormat
        })
}

#[inline]
fn is_stereo_f32(c: &cpal::SupportedStreamConfig) -> bool {
    c.sample_format() == SampleFormat::F32 && c.channels() == 2
}

#[inline]
fn is_stereo_f32_range(c: &cpal::SupportedStreamConfigRange) -> bool {
    c.sample_format() == SampleFormat::F32 && c.channels() == 2
}
