//! 音频设备层 · wasm 输入采集后端（独立模块，可整体删除/替换）
//!
//! 背景：cpal 的 WebAudio 后端未实现输入（`build_input_stream_raw` 直接 Err，
//! 0.18.2 实查），浏览器能力本身完备——getUserMedia 授权 → ScriptProcessor
//! 逐块回调原始 PCM。本模块在设备层补齐录音采集，公开 API
//! （[`AudioRecorder`](super::AudioRecorder)）不变。
//!
//! 回退路径：将来 cpal 支持输入后，删除本文件 +
//! `device.rs` 里 `open_input_stream` 的 wasm 分支 + `audio/mod.rs` 的
//! `mod device_web` 声明即可。与 video 模块按平台分文件（windows/linux/apple/web）
//! 是同一套架构模式。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::base::audio::common::{AudioError, AudioUserCallback, StereoFrame};

/// wasm 单线程模型的 Send 包装：JS 对象引用只在主线程触碰，
/// 不存在跨线程访问（ScriptProcessor 回调经主线程事件循环派发）——
/// AudioOutputBackend 的 Send 约束在此平台平凡满足
struct SendWrap<T>(T);
unsafe impl<T> Send for SendWrap<T> {}

/// 采集门（pause/resume 翻转）+ 用户回调
struct State {
    sink: Box<dyn AudioUserCallback + Send>,
    enabled: Arc<AtomicBool>,
}

/// 采集链各节点（防 GC + Drop 时关停）
struct Nodes {
    _closure: SendWrap<Closure<dyn FnMut(web_sys::AudioProcessingEvent)>>,
    _stream: web_sys::MediaStream,
    _source: web_sys::MediaStreamAudioSourceNode,
    _processor: web_sys::ScriptProcessorNode,
}

pub(crate) struct WebInput {
    ctx: SendWrap<web_sys::AudioContext>,
    nodes: Arc<Mutex<Option<Nodes>>>,
    enabled: Arc<AtomicBool>,
}

impl WebInput {
    /// 暂停采集（回调空转，设备保持打开）
    pub fn pause(&self) {
        self.enabled.store(false, Ordering::Relaxed);
    }

    /// 恢复采集
    pub fn resume(&self) {
        self.enabled.store(true, Ordering::Relaxed);
    }
}

impl Drop for WebInput {
    fn drop(&mut self) {
        // 关闭 AudioContext → 采集回调停止 + 麦克风释放（浏览器收回使用指示）
        let _ = self.ctx.0.close();
        if let Some(mut nodes) = self.nodes.lock().unwrap().take() {
            nodes._processor.set_onaudioprocess(None);
            // 闭包与节点随结构体一起 Drop（闭包 Drop = 解除 JS 回调注册）
        }
    }
}

/// 打开麦克风采集：getUserMedia 授权（浏览器自动弹框）→ 单声道 PCM
/// → 立体声帧喂给用户回调。采样率 = AudioContext 真实采样率。
///
/// 注意：授权是异步的——本函数同步返回后，帧在授权完成才开始流入。
pub(crate) fn open_input(
    mut sink: Box<dyn AudioUserCallback + Send>,
) -> Result<(WebInput, u32), AudioError> {
    let window = web_sys::window().ok_or_else(|| AudioError::custom("Web 录音需要浏览器环境"))?;
    let md = window
        .navigator()
        .media_devices()
        .map_err(|e| AudioError::custom(format!("mediaDevices 不可用: {e:?}")))?;

    let ctx = SendWrap(
        web_sys::AudioContext::new()
            .map_err(|e| AudioError::custom(format!("AudioContext 创建失败: {e:?}")))?,
    );
    let sample_rate = ctx.0.sample_rate() as u32;

    let enabled = Arc::new(AtomicBool::new(true));
    let nodes: Arc<Mutex<Option<Nodes>>> = Arc::new(Mutex::new(None));

    let state = Arc::new(Mutex::new(State { sink, enabled: enabled.clone() }));

    let mut constraints = web_sys::MediaStreamConstraints::new();
    constraints.audio(&wasm_bindgen::JsValue::from_bool(true));
    let promise = md
        .get_user_media_with_constraints(&constraints)
        .map_err(|e| AudioError::custom(format!("getUserMedia 失败: {e:?}")))?;

    let state2 = state.clone();
    let ctx2 = ctx.0.clone();
    let nodes2 = nodes.clone();
    wasm_bindgen_futures::spawn_local(async move {
        let js = match wasm_bindgen_futures::JsFuture::from(promise).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[starfish-audio] 麦克风授权失败/被拒: {e:?}");
                return;
            }
        };
        let stream: web_sys::MediaStream = js.into();
        let source = match ctx2.create_media_stream_source(&stream) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[starfish-audio] MediaStreamSource 创建失败: {e:?}");
                return;
            }
        };
        // 输出声道 0：不回放（避免外放啸叫），仅采集
        let processor = match ctx2.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(4096, 1, 0) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[starfish-audio] ScriptProcessor 创建失败: {e:?}");
                return;
            }
        };
        let state_cb = state2.clone();
        let closure = SendWrap(Closure::wrap(Box::new(
            move |ev: web_sys::AudioProcessingEvent| {
                let mut st = state_cb.lock().unwrap();
                if !st.enabled.load(Ordering::Relaxed) {
                    return;
                }
                if let Ok(input) = ev.input_buffer() {
                    if let Ok(data) = input.get_channel_data(0) {
                        // 单声道 → 立体声帧（左右同值）
                        let mut frames: Vec<StereoFrame> = data
                            .iter()
                            .map(|&s| StereoFrame { left: s, right: s })
                            .collect();
                        st.sink.on_frames(&mut frames);
                    }
                }
            },
        ) as Box<dyn FnMut(web_sys::AudioProcessingEvent)>));
        processor.set_onaudioprocess(Some(closure.0.as_ref().unchecked_ref()));
        // ScriptProcessor 需接入图内才会回调；接 destination 但输出声道 0 → 不外放
        let _ = source.connect_with_audio_node(&processor);
        let _ = processor.connect_with_audio_node(&ctx2.destination());
        *nodes2.lock().unwrap() = Some(Nodes {
            _closure: closure,
            _stream: stream,
            _source: source,
            _processor: processor,
        });
    });

    Ok((
        WebInput {
            ctx,
            nodes,
            enabled,
        },
        sample_rate,
    ))
}
