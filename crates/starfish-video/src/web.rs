//! Web 视频硬解后端（WebCodecs + mp4 纯 Rust 解复用）
//!
//! - 加载：`open(path)` 为同步入口，web 无阻塞 IO——内部 spawn_local 发起
//!   fetch（`path` 即 URL，相对页面基址），期间 `is_ready()=false`，上层时钟照走
//! - 解码：`VideoDecoder`（`hardwareAcceleration: "prefer-hardware"`——Web 平台
//!   API 只允许"偏好"不允许"强制"，浏览器自行决定硬解/软解，此为平台上限；
//!   `NotSupported` 类错误映射 [`VideoError::NoHardwareDecoder`]）
//! - 喂流：mp4_demux 产 Annex-B + 关键帧前置 SPS/PPS，按 decodeQueueSize 匀速喂
//! - 输出：回调即主线程 JS 任务，VideoFrame 入队；`poll_frame` 弹出交付，
//!   GPU 直拷纹理（见 player），追帧丢弃路径经 [`WasmVideoFrame`] Drop 及时 close
//!
//! WebCodecs 在 web-sys 中仍处 unstable gate（需 `--cfg=web_sys_unstable_apis`），
//! 本后端自持最小绑定，不给构建链添 cfg。

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Cursor;
use std::rc::Rc;
use std::time::Duration;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use js_sys::Object;

use super::mp4_demux::Demuxer;
use super::{DecodedFrame, DecodeBackend, FramePixels, Poll, VideoError};

// ── 最小 WebCodecs 动态绑定（Reflect 直调，不经 web-sys unstable 门控）────

/// VideoDecoder 封装（js_sys 动态调用）
struct VideoDecoder {
    obj: Object,
}

impl VideoDecoder {
    fn new(init: &Object) -> Result<Self, JsValue> {
        let global = js_sys::global();
        let ctor = js_sys::Reflect::get(&global, &"VideoDecoder".into())?;
        let ctor: js_sys::Function = ctor
            .dyn_into()
            .map_err(|_| JsValue::from_str("globalThis.VideoDecoder 不存在（浏览器不支持 WebCodecs）"))?;
        let args = js_sys::Array::new();
        args.push(&init.into());
        let obj = js_sys::Reflect::construct(&ctor, &args)?;
        Ok(Self { obj: obj.into() })
    }

    fn configure(&self, config: &Object) -> Result<(), JsValue> {
        let f: js_sys::Function = js_sys::Reflect::get(&self.obj, &"configure".into())?.into();
        f.call1(&self.obj, config).map(|_| ())
    }

    fn decode(&self, chunk: &Object) -> Result<(), JsValue> {
        let f: js_sys::Function = js_sys::Reflect::get(&self.obj, &"decode".into())?.into();
        f.call1(&self.obj, chunk).map(|_| ())
    }

    fn decode_queue_size(&self) -> u32 {
        js_sys::Reflect::get(&self.obj, &"decodeQueueSize".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32
    }

    /// flush：返回 Promise 但此处丢弃——输出仍经 output 回调送达
    fn flush(&self) {
        let f: js_sys::Function = match js_sys::Reflect::get(&self.obj, &"flush".into()) {
            Ok(v) => match v.dyn_into() {
                Ok(f) => f,
                Err(_) => return,
            },
            Err(_) => return,
        };
        let _ = f.call0(&self.obj);
    }
}

fn chunk_new(init: &Object) -> Result<Object, JsValue> {
    let global = js_sys::global();
    let ctor = js_sys::Reflect::get(&global, &"EncodedVideoChunk".into())?;
    let ctor: js_sys::Function = ctor
        .dyn_into()
        .map_err(|_| JsValue::from_str("globalThis.EncodedVideoChunk 不存在"))?;
    let args = js_sys::Array::new();
    args.push(&init.into());
    js_sys::Reflect::construct(&ctor, &args).map(|v| v.into())
}

/// VideoFrame 封装
struct VideoFrame {
    obj: Object,
}

impl VideoFrame {
    fn display_width(&self) -> u32 {
        js_sys::Reflect::get(&self.obj, &"displayWidth".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32
    }

    fn display_height(&self) -> u32 {
        js_sys::Reflect::get(&self.obj, &"displayHeight".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u32
    }

    fn timestamp(&self) -> f64 {
        js_sys::Reflect::get(&self.obj, &"timestamp".into())
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    }
}

/// 解码器状态（回调与泵共享）
enum State {
    /// fetch / 解复用 / 配置进行中
    Loading,
    Ready,
    /// 平台无解码能力（codec 不支持、解码器初始化失败）
    NoHardware,
    Failed(String),
    Eos,
}

/// 异步装配产物（spawn_local 完成后经 Rc 填充）
struct Inner {
    decoder: VideoDecoder,
    demux: Demuxer<Cursor<Vec<u8>>>,
    /// 回调须与解码器同生命周期，随 Inner 持有
    _on_output: Closure<dyn FnMut(JsValue)>,
    _on_error: Closure<dyn FnMut(JsValue)>,
}

pub(crate) struct WebDecoder {
    inner: Rc<RefCell<Option<Inner>>>,
    frames: Rc<RefCell<VecDeque<VideoFrame>>>,
    state: Rc<RefCell<State>>,
    /// 解码器输入在途水位（decodeQueueSize 上限）
    feed_high_water: u32,
    eos_flushing: bool,
    position: Duration,
}

/// fetch 全文件字节（视频后端与音轨泵共用的加载前端）
pub(crate) async fn fetch_bytes(path: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or("无 window")?;
    let resp_val = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(path))
        .await
        .map_err(|e| format!("fetch 失败: {e:?}"))?;
    let resp: web_sys::Response = resp_val
        .dyn_into()
        .map_err(|_| "fetch 响应类型异常".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf_val = wasm_bindgen_futures::JsFuture::from(
        resp.array_buffer().map_err(|e| format!("{e:?}"))?,
    )
    .await
    .map_err(|e| format!("读取响应失败: {e:?}"))?;
    let buf: js_sys::ArrayBuffer = buf_val
        .dyn_into()
        .map_err(|_| "ArrayBuffer 类型异常".to_string())?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

/// 解复用终点为帧步进兜底 pts 的便利读取
fn chunk_init(data: &[u8], pts_us: f64, key: bool) -> Result<Object, JsValue> {
    let init = Object::new();
    js_sys::Reflect::set(&init, &"type".into(), &if key { "key" } else { "delta" }.into())?;
    js_sys::Reflect::set(&init, &"timestamp".into(), &pts_us.into())?;
    js_sys::Reflect::set(&init, &"data".into(), &js_sys::Uint8Array::from(data))?;
    Ok(chunk_new(&init)?)
}

impl WebDecoder {
    pub(crate) fn open(path: &str) -> Result<Self, VideoError> {
        let frames: Rc<RefCell<VecDeque<VideoFrame>>> = Rc::new(RefCell::new(VecDeque::new()));
        let state: Rc<RefCell<State>> = Rc::new(RefCell::new(State::Loading));
        let inner: Rc<RefCell<Option<Inner>>> = Rc::new(RefCell::new(None));

        {
            let frames = frames.clone();
            let state = state.clone();
            let inner = inner.clone();
            let path = path.to_string();
            spawn_local(async move {
                let set_failed = |s: &Rc<RefCell<State>>, no_hw: bool, msg: String| {
                    *s.borrow_mut() = if no_hw {
                        State::NoHardware
                    } else {
                        State::Failed(msg)
                    };
                };

                // ── fetch 全文件（字节范围流式为后续优化）──
                let bytes = match fetch_bytes(&path).await {
                    Ok(b) => b,
                    Err(msg) => return set_failed(&state, false, msg),
                };

                // ── 解复用 ──
                let mut demux = match Demuxer::new(Cursor::new(bytes)) {
                    Ok(d) => d,
                    Err(e) => return set_failed(&state, false, e.to_string()),
                };
                let (width, height) = demux.size();
                let codec = demux.codec_string.clone();

                // ── 回调 ──
                let on_output = {
                    let frames = frames.clone();
                    Closure::<dyn FnMut(JsValue)>::new(move |frame: JsValue| {
                        frames.borrow_mut().push_back(VideoFrame { obj: frame.into() });
                    })
                };
                let on_error = {
                    let state = state.clone();
                    Closure::<dyn FnMut(JsValue)>::new(move |e: JsValue| {
                        let msg = e
                            .as_string()
                            .or_else(|| {
                                js_sys::JSON::stringify(&e)
                                    .ok()
                                    .map(|s| s.as_string().unwrap_or_default())
                            })
                            .unwrap_or_else(|| "decoder error".into());
                        let lower = msg.to_ascii_lowercase();
                        let no_hw =
                            lower.contains("notsupported") || lower.contains("unsupported");
                        let mut s = state.borrow_mut();
                        if no_hw {
                            *s = State::NoHardware;
                        } else {
                            *s = State::Failed(msg);
                        }
                    })
                };

                // ── VideoDecoder 配置 ──
                let init = Object::new();
                let config = Object::new();
                let setup = (|| -> Result<(), JsValue> {
                    js_sys::Reflect::set(&init, &"output".into(), on_output.as_ref())?;
                    js_sys::Reflect::set(&init, &"error".into(), on_error.as_ref())?;
                    js_sys::Reflect::set(&config, &"codec".into(), &codec.into())?;
                    js_sys::Reflect::set(&config, &"codedWidth".into(), &width.into())?;
                    js_sys::Reflect::set(&config, &"codedHeight".into(), &height.into())?;
                    // Web 平台无"强制硬解"，prefer-hardware 为上限
                    js_sys::Reflect::set(
                        &config,
                        &"hardwareAcceleration".into(),
                        &"prefer-hardware".into(),
                    )?;
                    Ok(())
                })();
                if let Err(e) = setup {
                    return set_failed(&state, false, format!("解码配置组装失败: {e:?}"));
                }
                let decoder = match VideoDecoder::new(&init) {
                    Ok(d) => d,
                    Err(e) => {
                        return set_failed(&state, false, format!("VideoDecoder 创建失败: {e:?}"))
                    }
                };
                if let Err(e) = decoder.configure(&config) {
                    return set_failed(&state, false, format!("解码器配置失败: {e:?}"));
                }

                *inner.borrow_mut() = Some(Inner {
                    decoder,
                    demux,
                    _on_output: on_output,
                    _on_error: on_error,
                });
                *state.borrow_mut() = State::Ready;
            });
        }

        Ok(Self {
            inner,
            frames,
            state,
            feed_high_water: 4,
            eos_flushing: false,
            position: Duration::ZERO,
        })
    }
}

impl DecodeBackend for WebDecoder {
    fn poll_frame(&mut self) -> Result<Poll, VideoError> {
        match &*self.state.borrow() {
            State::Loading => return Ok(Poll::Pending),
            State::NoHardware => return Err(VideoError::NoHardwareDecoder),
            State::Failed(msg) => return Err(VideoError::Backend(msg.clone())),
            State::Eos => return Ok(Poll::Eos),
            State::Ready => {}
        }

        let mut inner = self.inner.borrow_mut();
        let Inner {
            decoder, demux, ..
        } = inner.as_mut().expect("Ready 态 inner 必已填充");

        // 喂流：维持输入在途水位
        while decoder.decode_queue_size() < self.feed_high_water && !self.eos_flushing {
            match demux.next_sample()? {
                Some((data, pts, key)) => {
                    let chunk = chunk_init(
                        &data,
                        pts.as_micros() as f64,
                        key,
                    )
                    .map_err(|e| VideoError::Backend(format!("chunk 构造失败: {e:?}")))?;
                    decoder
                        .decode(&chunk)
                        .map_err(|e| VideoError::Backend(format!("decode 失败: {e:?}")))?;
                }
                None => {
                    self.eos_flushing = true;
                    decoder.flush(); // Promise 丢弃：输出仍经回调送达
                }
            }
        }

        // 收输出
        let frame = self.frames.borrow_mut().pop_front();
        if let Some(frame) = frame {
            let pts = Duration::from_micros(frame.timestamp().max(0.0) as u64);
            self.position = pts;
            let eos = self.eos_flushing
                && decoder.decode_queue_size() == 0
                && self.frames.borrow().is_empty();
            return Ok(Poll::Frame(DecodedFrame {
                width: frame.display_width(),
                height: frame.display_height(),
                pts,
                pixels: FramePixels::VideoFrame(super::WasmVideoFrame(
                    web_sys::VideoFrame::from(JsValue::from(frame.obj)),
                )),
            }));
        }

        // 无帧可出：流已喂完且管线排空 → Eos，否则在途
        if self.eos_flushing && decoder.decode_queue_size() == 0 {
            *self.state.borrow_mut() = State::Eos;
            return Ok(Poll::Eos);
        }
        Ok(Poll::Pending)
    }

    fn is_ready(&self) -> bool {
        matches!(*self.state.borrow(), State::Ready)
    }

    fn position(&self) -> Duration {
        self.position
    }
}
