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

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use std::sync::Arc;

// cpal 导入门控：OHOS 目标（target_os="linux" + target_env="ohos"，
// rustc --print cfg 实测 2026-11-11）会拉 ALSA 主机——alsa-sys 构建
// 脚本在 OHOS 实证失败；OHOS 走本文件桩分支（下方同款 cfg），真适配
// = OHAudio 实现同一公共面（open_output_stream/open_input_stream/
// 枚举——上层零 cpal 类型，唯一收敛点即本文件）。
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
use cpal::{SampleFormat, StreamConfig};

use super::common::{AudioError, AudioUserCallback};
// StereoFrame 仅原生 cpal 回调路径使用（OHOS 桩分支无回调面）
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
use super::common::StereoFrame;

/// 设备端实际生效的流规格（打开成功后由设备值回填）
#[derive(Debug, Clone, Copy)]
pub struct StreamSpec {
    /// 设备真实采样率（Hz）——混音域基准
    pub sample_rate: u32,
    /// 声道数（v1 恒为 2）
    pub channels: u16,
}

/// WebAudio 上下文句柄（open_output_stream 时存入；
/// pygame/event 翻译层钩子经 web_resume_audio 恢复——浏览器自动播放策略）
#[cfg(target_arch = "wasm32")]
static WEB_AUDIO_CTX: std::sync::OnceLock<Arc<web_sys::AudioContext>> = std::sync::OnceLock::new();

/// WebAudio 上下文句柄读取（web_resume_audio 用）
#[cfg(target_arch = "wasm32")]
pub(crate) fn web_audio_ctx() -> Option<Arc<web_sys::AudioContext>> {
    WEB_AUDIO_CTX.get().cloned()
}

/// 打开的设备流（Drop 即停止并释放设备）
///
/// cpal 0.17+ 的 `Stream` 为 Send+Sync，本结构可安全跨线程持有
///（对应 free-threaded 线程契约：混音器/录音器可从任意线程创建与销毁）。
#[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
pub struct DeviceStream {
    stream: cpal::Stream,
    pub spec: StreamSpec,
}

/// wasm 变体：输出走 cpal（WebAudio 后端）；输入为 web-sys 自持采集
/// （cpal 的 WebAudio 后端未实现输入，采集后端独立在 device_web.rs）
#[cfg(target_arch = "wasm32")]
pub struct DeviceStream {
    stream: Option<cpal::Stream>,
    input: Option<super::device_web::WebInput>,
    pub spec: StreamSpec,
}

/// OHOS 站桩变体：设备面 = OHAudio（真适配层待建）——本结构刻意不可
/// 构造（`_never: Infallible` 无值可填；open_output_stream /
/// open_input_stream 恒 Err），类型系统编码"OHOS 尚无设备流"；`spec`
/// 字段保留 = 上层（混音器/录音器直取字段）零改动。
#[cfg(all(target_os = "linux", target_env = "ohos"))]
pub struct DeviceStream {
    /// 站桩面恒 `StreamSpec { 48000, 2 }` 语义占位（永不读取——无实例）
    pub spec: StreamSpec,
    /// 不可构造标记（Infallible 无值——本类型无实例）
    _never: std::convert::Infallible,
}

impl DeviceStream {
    /// 暂停流（设备保持打开，回调停发）
    #[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
    pub fn pause(&self) -> Result<(), AudioError> {
        self.stream
            .pause()
            .map_err(|e| AudioError::Device(format!("流 pause 失败: {e}")))
    }

    /// 暂停流
    #[cfg(target_arch = "wasm32")]
    pub fn pause(&self) -> Result<(), AudioError> {
        if let Some(input) = &self.input {
            input.pause();
            return Ok(());
        }
        if let Some(stream) = &self.stream {
            return stream
                .pause()
                .map_err(|e| AudioError::Device(format!("流 pause 失败: {e}")));
        }
        Ok(())
    }

    /// 恢复流
    #[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
    pub fn resume(&self) -> Result<(), AudioError> {
        self.stream
            .play()
            .map_err(|e| AudioError::Device(format!("流 resume 失败: {e}")))
    }

    /// OHOS 站桩变体（DeviceStream 不可构造——经 Infallible 字段编码不可达；
    /// struct 空匹配不适用，分流到字段自身的 uninhabited match）
    #[cfg(all(target_os = "linux", target_env = "ohos"))]
    pub fn pause(&self) -> Result<(), AudioError> {
        match self._never {}
    }

    #[cfg(all(target_os = "linux", target_env = "ohos"))]
    pub fn resume(&self) -> Result<(), AudioError> {
        match self._never {}
    }

    /// 恢复流
    #[cfg(target_arch = "wasm32")]
    pub fn resume(&self) -> Result<(), AudioError> {
        if let Some(input) = &self.input {
            input.resume();
            return Ok(());
        }
        if let Some(stream) = &self.stream {
            return stream
                .play()
                .map_err(|e| AudioError::Device(format!("流 resume 失败: {e}")));
        }
        Ok(())
    }
}

impl super::AudioOutputBackend for DeviceStream {
    fn spec(&self) -> super::device::StreamSpec { self.spec }
    fn pause(&self) -> Result<(), AudioError> { DeviceStream::pause(self) }
    fn resume(&self) -> Result<(), AudioError> { DeviceStream::resume(self) }
}

/// 打开默认播放设备：数据面 = 用户回调周期性填充立体声帧（构造即播放）
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
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
    let err_cb = move |e| starfish_debug::log_error!(format!("[starfish-audio] 播放流错误: {e}"));
    let data_cb = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        fill_interleaved(data, channels, |frames| user_cb.on_frames(frames));
    };

    let stream = device
        .build_output_stream(config, data_cb, err_cb, None)
        .map_err(|e| AudioError::Device(format!("播放流打开失败: {e}")))?;
    stream
        .play()
        .map_err(|e| AudioError::Device(format!("播放流启动失败: {e}")))?;

    // WebAudio 上下文句柄存静态（浏览器自动播放策略：首个用户手势解锁；
    // 2026-09-30 批次 H，pygame-rs 绑定需求）。仅 wasm（原生无此静态/无此约束）。
    // cpal 0.18 平台 Stream 是动态分发包装（StreamInner 枚举），audio_context()
    // 只在具体 webaudio::Stream 上（且仅非 atomics 路径提供）——经 as_inner
    // 解包获取；单主机枚举单臂即穷尽（atomics = audioworklet 后端届时另议）
    #[cfg(target_arch = "wasm32")]
    match stream.as_inner() {
        cpal::platform::StreamInner::WebAudio(s) => {
            let _ = WEB_AUDIO_CTX.set(Arc::new(s.audio_context().clone()));
        }
    }

    Ok(DeviceStream {
        #[cfg(target_arch = "wasm32")]
        stream: Some(stream),
        #[cfg(not(target_arch = "wasm32"))]
        stream,
        #[cfg(target_arch = "wasm32")]
        input: None,
        spec: StreamSpec { sample_rate, channels },
    })
}

/// OHOS 站桩：OHAudio 设备面待建（显式 Err 定约——绝不静默；
/// DeviceStream 不可构造，真适配层落位后本分支退役）。
#[cfg(all(target_os = "linux", target_env = "ohos"))]
pub fn open_output_stream(
    _user_cb: impl AudioUserCallback + Send + 'static,
) -> Result<DeviceStream, AudioError> {
    Err(AudioError::Device(
        "open_output_stream: HarmonyOS 站桩预留——OHAudio 设备面待建，\
         接入位见 pygamest reference/新平台扩展指南（macOS-iOS-HarmonyOS）"
            .into(),
    ))
}

/// 打开默认录音设备：数据面 = 用户回调周期性收到采集帧（构造即采集）
#[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
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
    let err_cb = move |e| starfish_debug::log_error!(format!("[starfish-audio] 录音流错误: {e}"));
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

/// 打开默认录音设备（wasm）：cpal 的 WebAudio 后端未实现输入（build_input_stream_raw
/// 直接 Err）→ 用浏览器标准接口自持采集：getUserMedia 授权 → ScriptProcessor 逐块
/// 单声道 PCM → 立体声帧喂给既有 RingSink。公开 API（AudioRecorder）不变；
/// 将来 cpal 支持输入后，删除本分支与 `device_web.rs` 模块即可回退。
#[cfg(target_arch = "wasm32")]
pub fn open_input_stream(
    user_cb: impl AudioUserCallback + Send + 'static,
) -> Result<DeviceStream, AudioError> {
    let (input, sample_rate) = super::device_web::open_input(Box::new(user_cb))?;
    Ok(DeviceStream {
        stream: None,
        input: Some(input),
        spec: StreamSpec { sample_rate, channels: 2 },
    })
}

/// OHOS 站桩（录音）：同 open_output_stream 的显式 Err 定约。
#[cfg(all(target_os = "linux", target_env = "ohos"))]
pub fn open_input_stream(
    _user_cb: impl AudioUserCallback + Send + 'static,
) -> Result<DeviceStream, AudioError> {
    Err(AudioError::Device(
        "open_input_stream: HarmonyOS 站桩预留——OHAudio 采集面待建，\
         接入位见 pygamest reference/新平台扩展指南（macOS-iOS-HarmonyOS）"
            .into(),
    ))
}

/// 枚举播放设备名（诊断用）
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
pub fn output_device_names() -> Result<Vec<String>, AudioError> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .map_err(|e| AudioError::Device(format!("枚举播放设备失败: {e}")))?;
    Ok(devices.map(device_name).collect())
}

/// OHOS 站桩（播放枚举）：同 open_output_stream 的显式 Err 定约。
#[cfg(all(target_os = "linux", target_env = "ohos"))]
pub fn output_device_names() -> Result<Vec<String>, AudioError> {
    Err(AudioError::Device(
        "output_device_names: HarmonyOS 站桩预留——OHAudio 枚举面待建".into(),
    ))
}

/// 枚举录音设备名（诊断用）
#[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
pub fn input_device_names() -> Result<Vec<String>, AudioError> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| AudioError::Device(format!("枚举录音设备失败: {e}")))?;
    Ok(devices.map(device_name).collect())
}

/// wasm 无设备枚举接口（浏览器不暴露输入设备列表）：返回单个虚拟设备名
#[cfg(target_arch = "wasm32")]
pub fn input_device_names() -> Result<Vec<String>, AudioError> {
    Ok(vec!["WebAudio 默认麦克风".into()])
}

/// OHOS 站桩（录音枚举）：同 open_output_stream 的显式 Err 定约。
#[cfg(all(target_os = "linux", target_env = "ohos"))]
pub fn input_device_names() -> Result<Vec<String>, AudioError> {
    Err(AudioError::Device(
        "input_device_names: HarmonyOS 站桩预留——OHAudio 枚举面待建".into(),
    ))
}

/// 设备名（0.18 起 name() 并入 description()）
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
fn device_name(d: cpal::Device) -> String {
    d.description()
        .map(|desc| desc.name().to_string())
        .unwrap_or_else(|_| "未知设备".into())
}

// ───────────────────────────────── 内部 ─────────────────────────────────

// 以下 helper 仅原生 cpal 路径使用（OHOS 桩分支无回调/协商面）
/// 把交错 f32 缓冲以 [`StereoFrame`] 视角交给用户回调
///
/// `#[repr(C) {f32, f32}]` 与交错立体声 f32 布局一致，bytemuck 零拷贝。
/// 声道数非 2 时静音兜底（协商层已保证 v1 恒为立体声，此为防御分支）。
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
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
#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
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

#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
#[inline]
fn is_stereo_f32(c: &cpal::SupportedStreamConfig) -> bool {
    c.sample_format() == SampleFormat::F32 && c.channels() == 2
}

#[cfg(not(all(target_os = "linux", target_env = "ohos")))]
#[inline]
fn is_stereo_f32_range(c: &cpal::SupportedStreamConfigRange) -> bool {
    c.sample_format() == SampleFormat::F32 && c.channels() == 2
}

// ───────────────────────── wasm 输入采集（web-sys 自持） ─────────────────────────
//
// 背景：cpal 的 WebAudio 后端未实现输入（build_input_stream_raw 直接 Err，
// 0.18.2 实查），浏览器能力本身完备——getUserMedia 授权 → ScriptProcessor
// 逐块回调原始 PCM。本模块在设备层补齐采集，公开 API（AudioRecorder）不变。
// 将来 cpal 支持输入后：删除本模块 + open_input_stream 的 wasm 分支即可回退。
