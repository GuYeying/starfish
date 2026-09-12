//! Android 视频硬解后端（MediaCodec，JNI 驱动）
//!
//! - JavaVM 取自 `ndk_context`（引擎经 android-activity/winit 启动时注入；
//!   未注入时返回 Backend 错误，即视频功能依赖引擎侧安卓引导）
//! - 硬解唯一策略：`MediaCodecList(ALL_CODECS)` 过滤 `isHardwareAccelerated`
//!   （API < 29 用名称启发式：`omx.google`/`c2.android` 等为软解），无候选直接
//!   [`VideoError::NoHardwareDecoder`]
//! - 解复用：mp4_demux（纯 Rust）产 Annex-B；SPS/PPS 经 `csd-0`/`csd-1` 注入
//! - 输出：COLOR_FormatYUV420Flexible（实践即 NV12）系统内存，stride 以
//!   INFO_OUTPUT_FORMAT_CHANGED 实测为准（不假设 `stride × height`）
//! - 同步阻塞语义（dequeue 短超时轮询）与 MF `ReadSample` 同契约
//!
//! 主线程契约与 MF 一致；MediaCodec 同步模式无内部回调线程。

use std::io::BufReader;
use std::time::Duration;

use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::JavaVM;

use super::mp4_demux::Demuxer;
use super::{DecodedFrame, DecodeBackend, FramePixels, Poll, VideoError};

/// dequeue 超时（µs）：短轮询，保持主线程行为可预期
const DEQUEUE_TIMEOUT_US: i64 = 10_000;
/// MediaCodecInfo.CodecCapabilities.COLOR_FormatYUV420Flexible
const COLOR_FORMAT_YUV420_FLEXIBLE: i32 = 2_135_033_992;
/// MediaCodec.BUFFER_FLAG_END_OF_STREAM
const BUFFER_FLAG_EOS: i32 = 4;
/// MediaCodec.INFO_TRY_AGAIN_LATER
const INFO_TRY_AGAIN: i32 = -1;
/// MediaCodec.INFO_OUTPUT_FORMAT_CHANGED
const INFO_FORMAT_CHANGED: i32 = -2;

pub(crate) struct MediaCodecReader {
    vm: JavaVM,
    codec: GlobalRef,
    buffer_info: GlobalRef,
    demux: Demuxer<BufReader<std::fs::File>>,
    width: u32,
    height: u32,
    stride: usize,
    /// 输入耗尽且 EOS 标志已下发
    eos_sent: bool,
    /// 已收到 EOS 标志的输出
    eos_seen: bool,
    /// EOS 后输出已排空
    drained: bool,
    position: Duration,
}

impl MediaCodecReader {
    pub(crate) fn open(path: &str) -> Result<Self, VideoError> {
        // ── JavaVM：引擎安卓引导注入 ──
        let ctx = ndk_context::android_context();
        let vm_ptr = ctx.vm();
        if vm_ptr.is_null() {
            return Err(VideoError::Backend(
                "ndk_context 未初始化：视频功能依赖引擎安卓引导（android-activity）".into(),
            ));
        }
        let vm = unsafe { JavaVM::from_raw(vm_ptr.cast()) }
            .map_err(|e| VideoError::Backend(format!("JavaVM 获取失败: {e}")))?;
        // ── 解复用（不需要 JVM）──
        let file = std::fs::File::open(path)
            .map_err(|e| VideoError::Backend(format!("打开视频失败: {e}")))?;
        let mut demux = Demuxer::new(BufReader::new(file))?;
        let (width, height) = demux.size();

        // JNI 段独立作用域：env 守卫借用 vm，须在 vm 移动前结束
        let (codec_g, buffer_info_g) = {
        let env = &mut vm
            .attach_current_thread()
            .map_err(|e| VideoError::Backend(format!("JNI attach 失败: {e}")))?;

        // ── 硬解码器发现（无候选即报错）──
        let codec_name = find_hw_avc_decoder(env)?;

        // ── MediaFormat：宽高 + YUV420Flexible + csd（SPS/PPS）──
        let media_format_cls = env
            .find_class("android/media/MediaFormat")
            .map_err(jerr("MediaFormat 类缺失"))?;
        let jmime = env.new_string("video/avc").map_err(jerr("new_string 失败"))?;
        let format = env
            .call_static_method(
                media_format_cls,
                "createVideoFormat",
                "(Ljava/lang/String;II)Landroid/media/MediaFormat;",
                &[
                    JValue::Object(&jmime),
                    JValue::Int(width as i32),
                    JValue::Int(height as i32),
                ],
            )
            .and_then(|v| v.l())
            .map_err(jerr("createVideoFormat 失败"))?;
        set_int(env, &format, "color-format", COLOR_FORMAT_YUV420_FLEXIBLE)?;

        let sps_buf = jni_byte_buffer(env, demux.sps_annexb())?;
        let pps_buf = jni_byte_buffer(env, demux.pps_annexb())?;
        set_buffer(env, &format, "csd-0", &sps_buf)?;
        set_buffer(env, &format, "csd-1", &pps_buf)?;

        // ── MediaCodec 创建 / 配置 / 启动 ──
        let jcodec_name = env.new_string(&codec_name).map_err(jerr("new_string 失败"))?;
        let codec_cls = env
            .find_class("android/media/MediaCodec")
            .map_err(jerr("MediaCodec 类缺失"))?;
        let codec = env
            .call_static_method(
                codec_cls,
                "createByCodecName",
                "(Ljava/lang/String;)Landroid/media/MediaCodec;",
                &[JValue::Object(&jcodec_name)],
            )
            .and_then(|v| v.l())
            .map_err(jerr("createByCodecName 失败"))?;
        env.call_method(
            &codec,
            "configure",
            "(Landroid/media/MediaFormat;Landroid/view/Surface;Landroid/media/MediaCrypto;I)V",
            &[
                JValue::Object(&format),
                JValue::Object(&JObject::null()),
                JValue::Object(&JObject::null()),
                JValue::Int(0),
            ],
        )
        .map_err(|e| VideoError::Backend(format!("MediaCodec configure 失败: {e}")))?;
        env.call_method(&codec, "start", "()V", &[])
            .map_err(jerr("MediaCodec start 失败"))?;

        let buffer_info = env
            .new_object("android/media/MediaCodec$BufferInfo", "()V", &[])
            .map_err(jerr("BufferInfo 创建失败"))?;

            (
                env.new_global_ref(&codec).map_err(jerr("GlobalRef 失败"))?,
                env.new_global_ref(&buffer_info)
                    .map_err(jerr("GlobalRef 失败"))?,
            )
        };

        Ok(Self {
            vm,
            codec: codec_g,
            buffer_info: buffer_info_g,
            demux,
            width,
            height,
            stride: width as usize, // INFO_OUTPUT_FORMAT_CHANGED 到达后以实测为准
            eos_sent: false,
            eos_seen: false,
            drained: false,
            position: Duration::ZERO,
        })
    }
}

/// 查找支持 video/avc 的硬件解码器名（无候选 → NoHardwareDecoder）
fn find_hw_avc_decoder(env: &mut jni::JNIEnv) -> Result<String, VideoError> {
    let list_cls = env
        .find_class("android/media/MediaCodecList")
        .map_err(jerr("MediaCodecList 类缺失"))?;
    let list = env
        .new_object(list_cls, "(I)V", &[JValue::Int(0)]) // ALL_CODECS
        .map_err(jerr("MediaCodecList 创建失败"))?;
    let infos = env
        .call_method(
            &list,
            "getCodecInfos",
            "()[Landroid/media/MediaCodecInfo;",
            &[],
        )
        .and_then(|v| v.l())
        .map_err(jerr("getCodecInfos 失败"))?;
    let infos: jni::objects::JObjectArray = infos.into();
    let count = env.get_array_length(&infos).map_err(jerr("数组长度失败"))?;

    let sdk = env
        .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")
        .and_then(|v| v.i())
        .unwrap_or(29);

    for i in 0..count {
        let info = env
            .get_object_array_element(&infos, i)
            .map_err(jerr("codec info 获取失败"))?;
        let is_encoder = env
            .call_method(&info, "isEncoder", "()Z", &[])
            .and_then(|v| v.z())
            .unwrap_or(true);
        if is_encoder {
            continue;
        }
        // 硬解判定：API 29+ 官方标志；旧版本名称启发式
        let hw = if sdk >= 29 {
            env.call_method(&info, "isHardwareAccelerated", "()Z", &[])
                .and_then(|v| v.z())
                .unwrap_or(false)
        } else {
            let name_obj = env
                .call_method(&info, "getName", "()Ljava/lang/String;", &[])
                .and_then(|v| v.l())
                .map_err(jerr("getName 失败"))?;
            let name = jstring_value(env, name_obj)?;
            let lower = name.to_ascii_lowercase();
            !(lower.starts_with("omx.google")
                || lower.starts_with("c2.android")
                || lower.contains(".sw."))
        };
        if !hw {
            continue;
        }
        // 支持 video/avc？
        let types = env
            .call_method(&info, "getSupportedTypes", "()[Ljava/lang/String;", &[])
            .and_then(|v| v.l())
            .map_err(jerr("getSupportedTypes 失败"))?;
        let types: jni::objects::JObjectArray = types.into();
        let tcount = env.get_array_length(&types).map_err(jerr("数组长度失败"))?;
        for t in 0..tcount {
            let tobj = env
                .get_object_array_element(&types, t)
                .map_err(jerr("类型获取失败"))?;
            if jstring_value(env, tobj)? == "video/avc" {
                let name_obj = env
                    .call_method(&info, "getName", "()Ljava/lang/String;", &[])
                    .and_then(|v| v.l())
                    .map_err(jerr("getName 失败"))?;
                return jstring_value(env, name_obj);
            }
        }
    }
    Err(VideoError::NoHardwareDecoder)
}

/// JObject(String) → Rust String
fn jstring_value(env: &mut jni::JNIEnv, obj: JObject) -> Result<String, VideoError> {
    let jstr = JString::from(obj);
    let java_str = env.get_string(&jstr).map_err(jerr("字符串读取失败"))?;
    Ok(java_str.to_string_lossy().into_owned())
}

// ── JNI 小工具 ──────────────────────────────────────────

fn jerr(msg: &'static str) -> impl Fn(jni::errors::Error) -> VideoError {
    move |e| VideoError::Backend(format!("{msg}: {e}"))
}

fn set_int(
    env: &mut jni::JNIEnv,
    format: &JObject,
    key: &str,
    value: i32,
) -> Result<(), VideoError> {
    let jkey = env.new_string(key).map_err(jerr("new_string 失败"))?;
    env.call_method(
        format,
        "setInteger",
        "(Ljava/lang/String;I)V",
        &[JValue::Object(&jkey), JValue::Int(value)],
    )
    .map_err(jerr("setInteger 失败"))?;
    Ok(())
}

fn jni_byte_buffer<'local>(
    env: &mut jni::JNIEnv<'local>,
    bytes: &[u8],
) -> Result<JObject<'local>, VideoError> {
    let arr = env
        .new_byte_array(bytes.len() as i32)
        .map_err(jerr("new_byte_array 失败"))?;
    // jni jbyte = i8；视频字节按位等价转视图
    let as_i8 = unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast::<i8>(), bytes.len()) };
    env.set_byte_array_region(&arr, 0, as_i8)
        .map_err(jerr("set_byte_array_region 失败"))?;
    Ok(arr.into())
}

fn set_buffer(
    env: &mut jni::JNIEnv,
    format: &JObject,
    key: &str,
    buf: &JObject,
) -> Result<(), VideoError> {
    let jkey = env.new_string(key).map_err(jerr("new_string 失败"))?;
    env.call_method(
        format,
        "setByteBuffer",
        "(Ljava/lang/String;Ljava/nio/ByteBuffer;)V",
        &[JValue::Object(&jkey), JValue::Object(buf)],
    )
    .map_err(jerr("setByteBuffer 失败"))?;
    Ok(())
}

fn get_int(env: &mut jni::JNIEnv, obj: &JObject, key: &str) -> Option<i32> {
    let jkey = env.new_string(key).ok()?;
    let integer = env
        .call_method(
            obj,
            "getInteger",
            "(Ljava/lang/String;)Ljava/lang/Integer;",
            &[JValue::Object(&jkey)],
        )
        .and_then(|v| v.l())
        .ok()?;
    env.call_method(&integer, "intValue", "()I", &[])
        .and_then(|v| v.i())
        .ok()
}

impl DecodeBackend for MediaCodecReader {
    fn poll_frame(&mut self) -> Result<Poll, VideoError> {
        if self.drained {
            return Ok(Poll::Eos);
        }
        let env = &mut self
            .vm
            .attach_current_thread()
            .map_err(|e| VideoError::Backend(format!("JNI attach 失败: {e}")))?;
        let codec = self.codec.as_obj();

        loop {
            // ── 喂输入：拿一个空闲输入缓冲，投一份样本 ──
            if !self.eos_sent {
                let in_idx = env
                    .call_method(
                        codec,
                        "dequeueInputBuffer",
                        "(I)I",
                        &[JValue::Long(DEQUEUE_TIMEOUT_US)],
                    )
                    .and_then(|v| v.i())
                    .map_err(jerr("dequeueInputBuffer 失败"))?;
                if in_idx >= 0 {
                    match self.demux.next_sample()? {
                        Some((data, pts, _)) => {
                            let buf = env
                                .call_method(
                                    codec,
                                    "getInputBuffer",
                                    "(I)Ljava/nio/ByteBuffer;",
                                    &[JValue::Int(in_idx)],
                                )
                                .and_then(|v| v.l())
                                .map_err(jerr("getInputBuffer 失败"))?;
                            let arr = jni_byte_buffer(env, &data)?;
                            env.call_method(
                                &buf,
                                "put",
                                "([B)Ljava/nio/ByteBuffer;",
                                &[JValue::Object(&arr)],
                            )
                            .map_err(jerr("ByteBuffer.put 失败"))?;
                            env.call_method(
                                codec,
                                "queueInputBuffer",
                                "(IIIJI)V",
                                &[
                                    JValue::Int(in_idx),
                                    JValue::Int(0),
                                    JValue::Int(data.len() as i32),
                                    JValue::Long(pts.as_micros() as i64),
                                    JValue::Int(0),
                                ],
                            )
                            .map_err(jerr("queueInputBuffer 失败"))?;
                        }
                        None => {
                            // 样本耗尽：下发 EOS 标志
                            env.call_method(
                                codec,
                                "queueInputBuffer",
                                "(IIIJI)V",
                                &[
                                    JValue::Int(in_idx),
                                    JValue::Int(0),
                                    JValue::Int(0),
                                    JValue::Long(0),
                                    JValue::Int(BUFFER_FLAG_EOS),
                                ],
                            )
                            .map_err(jerr("queueInputBuffer(EOS) 失败"))?;
                            self.eos_sent = true;
                        }
                    }
                }
            }

            // ── 收输出 ──
            let out_idx = env
                .call_method(
                    codec,
                    "dequeueOutputBuffer",
                    "(Landroid/media/MediaCodec$BufferInfo;I)I",
                    &[
                        JValue::Object(self.buffer_info.as_obj()),
                        JValue::Long(DEQUEUE_TIMEOUT_US),
                    ],
                )
                .and_then(|v| v.i())
                .map_err(jerr("dequeueOutputBuffer 失败"))?;

            if out_idx >= 0 {
                let size = env
                    .get_field(self.buffer_info.as_obj(), "size", "I")
                    .and_then(|v| v.i())
                    .map_err(jerr("BufferInfo.size 失败"))? as usize;
                let pts_us = env
                    .get_field(self.buffer_info.as_obj(), "presentationTimeUs", "J")
                    .and_then(|v| v.j())
                    .map_err(jerr("BufferInfo.pts 失败"))?;
                let flags = env
                    .get_field(self.buffer_info.as_obj(), "flags", "I")
                    .and_then(|v| v.i())
                    .map_err(jerr("BufferInfo.flags 失败"))?;

                // 先拷后还（release 后缓冲归还解码器，顺序不可颠倒）
                let nv12 = if size > 0 {
                    let buf = env
                        .call_method(
                            codec,
                            "getOutputBuffer",
                            "(I)Ljava/nio/ByteBuffer;",
                            &[JValue::Int(out_idx)],
                        )
                        .and_then(|v| v.l())
                        .map_err(jerr("getOutputBuffer 失败"))?;
                    let arr = env
                        .new_byte_array(size as i32)
                        .map_err(jerr("new_byte_array 失败"))?;
                    env.call_method(
                        &buf,
                        "get",
                        "([B)Ljava/nio/ByteBuffer;",
                        &[JValue::Object(&arr)],
                    )
                    .map_err(jerr("ByteBuffer.get 失败"))?;
                    env.convert_byte_array(arr)
                        .map_err(jerr("convert_byte_array 失败"))?
                } else {
                    Vec::new()
                };

                env.call_method(
                    codec,
                    "releaseOutputBuffer",
                    "(IZ)V",
                    &[JValue::Int(out_idx), JValue::Bool(0)],
                )
                .map_err(jerr("releaseOutputBuffer 失败"))?;

                if flags & BUFFER_FLAG_EOS != 0 {
                    self.eos_seen = true;
                }
                if size == 0 {
                    if self.eos_sent && self.eos_seen {
                        self.drained = true;
                        return Ok(Poll::Eos);
                    }
                    continue;
                }

                let pts = Duration::from_micros(pts_us.max(0) as u64);
                self.position = pts;
                return Ok(Poll::Frame(DecodedFrame {
                    pixels: FramePixels::Nv12 {
                        nv12,
                        stride: self.stride,
                    },
                    width: self.width,
                    height: self.height,
                    pts,
                }));
            } else if out_idx == INFO_FORMAT_CHANGED {
                // 输出几何以实测为准（stride/slice-height 由驱动给出）
                let fmt = env
                    .call_method(codec, "getOutputFormat", "()Landroid/media/MediaFormat;", &[])
                    .and_then(|v| v.l())
                    .map_err(jerr("getOutputFormat 失败"))?;
                if let Some(w) = get_int(env, &fmt, "width") {
                    self.width = w as u32;
                }
                if let Some(h) = get_int(env, &fmt, "height") {
                    self.height = h as u32;
                }
                if let Some(s) = get_int(env, &fmt, "stride") {
                    if s > 0 {
                        self.stride = s as usize;
                    }
                }
                continue;
            } else if out_idx == INFO_TRY_AGAIN {
                // 无输出可收：输入已全喂且 EOS 已见 → 结束；否则回环继续喂
                if self.eos_sent && self.eos_seen {
                    self.drained = true;
                    return Ok(Poll::Eos);
                }
                continue;
            }
        }
    }

    fn position(&self) -> Duration {
        self.position
    }
}

impl Drop for MediaCodecReader {
    fn drop(&mut self) {
        if let Ok(mut env) = self.vm.attach_current_thread() {
            let codec = self.codec.as_obj();
            let _ = env.call_method(codec, "stop", "()V", &[]);
            let _ = env.call_method(codec, "release", "()V", &[]);
        }
    }
}
