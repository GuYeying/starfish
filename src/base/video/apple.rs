//! macOS/iOS 视频硬解后端（VideoToolbox，系统框架零依赖）
//!
//! 解复用走 AVAssetReader（`outputSettings = nil` → 输出压缩 H.264 样本缓冲），
//! 解码走 `VTDecompressionSession`，规格字典带
//! `kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder`——
//! 无硬件解码器时会话创建失败 → [`VideoError::NoHardwareDecoder`]，不落软解。
//!
//! 回调模型：VT 在内部线程回调输出 CVPixelBuffer（NV12/双平面），回调内按
//! 实测 plane stride 拷出（吸取 1088 对齐教训，不假设几何），经 channel 衔接
//! 上层的同步拉取语义（与 MF 同步 `ReadSample` 同契约）。

use std::cell::RefCell;
use std::ptr::{self, NonNull};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use objc2::rc::Retained;
use objc2_av_foundation::{AVAssetReader, AVAssetReaderTrackOutput, AVMediaTypeVideo, AVURLAsset};
use objc2_core_foundation::{kCFBooleanTrue, CFDictionary, CFRetained, CFString};
use objc2_core_media::{CMSampleBuffer, CMVideoFormatDescription};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRowOfPlane,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress, CVImageBuffer,
};
use objc2_foundation::{NSURL, NSString};
use objc2_video_toolbox::{
    kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder, VTDecodeFrameFlags,
    VTDecodeInfoFlags, VTDecompressionOutputCallbackRecord, VTDecompressionSession,
};

use super::{DecodedFrame, DecodeBackend, FramePixels, Poll, VideoError};

type FrameTx = Sender<Result<DecodedFrame, VideoError>>;

/// VT 回调上下文（经 refcon 裸指针传回；生命周期由 VtReader 的 Drop 保证）
struct CallbackCtx {
    tx: RefCell<Option<FrameTx>>,
}

/// VT 输出回调（VT 内部线程）：把 CVPixelBuffer 的 NV12 平面按实测 stride 拷出。
/// 回调内绝不可阻塞——只做内存拷贝后立即投递。
unsafe extern "C-unwind" fn vt_output_callback(
    refcon: *mut core::ffi::c_void,
    _source_frame_refcon: *mut core::ffi::c_void,
    status: i32,
    _info_flags: VTDecodeInfoFlags,
    image_buffer: *mut CVImageBuffer,
    pts: objc2_core_media::CMTime,
    _duration: objc2_core_media::CMTime,
) {
    unsafe {
    let ctx = &*(refcon as *const CallbackCtx);
    let send = |res: Result<DecodedFrame, VideoError>| {
        if let Ok(guard) = ctx.tx.try_borrow() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(res);
            }
        }
    };

    if status != 0 {
        send(Err(VideoError::Backend(format!("VT 解码帧失败 status={status}"))));
        return;
    }
    if image_buffer.is_null() {
        send(Err(VideoError::Backend("VT 回调缺少图像缓冲".into())));
        return;
    }
    // CVImageBuffer → CVPixelBuffer 子类下转（同指针布局的手工桥接）
    let pb = &*(image_buffer as *const CVPixelBuffer);

    {
        CVPixelBufferLockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
        let w = CVPixelBufferGetWidth(pb) as u32;
        let h = CVPixelBufferGetHeight(pb) as u32;
        let y_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 0) as usize;
        let uv_stride = CVPixelBufferGetBytesPerRowOfPlane(pb, 1) as usize;
        let y_ptr = CVPixelBufferGetBaseAddressOfPlane(pb, 0) as *const u8;
        let uv_ptr = CVPixelBufferGetBaseAddressOfPlane(pb, 1) as *const u8;

        if y_ptr.is_null() || uv_ptr.is_null() || w == 0 || h == 0 {
            CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);
            send(Err(VideoError::Backend("VT 像素缓冲平面地址无效".into())));
            return;
        }

        let y_len = y_stride * h as usize;
        let uv_len = uv_stride * (h as usize / 2);
        let mut nv12 = Vec::with_capacity(y_len + uv_len);
        nv12.extend_from_slice(std::slice::from_raw_parts(y_ptr, y_len));
        nv12.extend_from_slice(std::slice::from_raw_parts(uv_ptr, uv_len));
        CVPixelBufferUnlockBaseAddress(pb, CVPixelBufferLockFlags::ReadOnly);

        let pts = Duration::from_secs_f64(pts.seconds().max(0.0));
        send(Ok(DecodedFrame {
            pixels: FramePixels::Nv12 {
                nv12,
                stride: y_stride,
            },
            width: w,
            height: h,
            pts,
        }));
    }
    }
}

/// VideoToolbox 硬解读取器（NV12 系统内存输出）
pub(crate) struct VtReader {
    reader: Retained<AVAssetReader>,
    output: Retained<AVAssetReaderTrackOutput>,
    /// 会话（+1 持有，Drop 时 Invalidate 后经 CFRetained 释放）
    session: Option<CFRetained<VTDecompressionSession>>,
    ctx: *mut CallbackCtx,
    rx: Receiver<Result<DecodedFrame, VideoError>>,
    ended: bool,
    position: Duration,
}

// AVFoundation/VT 句柄均可跨线程传递；主线程契约与 MF 一致（文档约束）
unsafe impl Send for VtReader {}

impl VtReader {
    pub(crate) fn open(path: &str) -> Result<Self, VideoError> {
        unsafe {
            // ── 解复用：AVURLAsset → AVAssetReader（压缩 H.264 样本输出）──
            let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), false);
            let asset = AVURLAsset::URLAssetWithURL_options(&url, None);

            #[allow(deprecated)] // 同步轨道加载（旧 API，仍全平台可用）
            let track = asset.tracksWithMediaType(AVMediaTypeVideo.unwrap()).objectAtIndex(0);
            let Ok(reader) = AVAssetReader::assetReaderWithAsset_error(&asset) else {
                return Err(VideoError::Backend("AVAssetReader 创建失败".into()));
            };
            // outputSettings = nil → 输出压缩样本（H.264 裸流 + 格式描述）
            let output =
                AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
                    &track, None,
                );
            reader.addOutput(&output);
            reader
                .startReading()
                .then_some(())
                .ok_or_else(|| VideoError::Backend("AVAssetReader startReading 失败".into()))?;

            let (tx, rx) = mpsc::channel();
            let ctx = Box::into_raw(Box::new(CallbackCtx {
                tx: RefCell::new(Some(tx)),
            }));

            Ok(Self {
                reader,
                output,
                session: None,
                ctx,
                rx,
                ended: false,
                position: Duration::ZERO,
            })
        }
    }

    /// 首个样本到达时惰性建会话（需要格式描述）
    unsafe fn ensure_session(&mut self, sample_buffer: &CMSampleBuffer) -> Result<(), VideoError> {
        unsafe {
            if self.session.is_some() {
                return Ok(());
            }
            let Some(format) = sample_buffer.format_description() else {
                return Err(VideoError::Backend("样本缺少格式描述".into()));
            };
            // CMFormatDescription → CMVideoFormatDescription（CF 同指针重解释）
            let video_format: CFRetained<CMVideoFormatDescription> =
                CFRetained::cast_unchecked(format);

            // Require-Hardware 规格：无硬解即创建失败（硬解唯一策略）
            let key: &CFString =
                kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder;
            let spec = CFDictionary::from_slices(&[key], &[kCFBooleanTrue.unwrap()]);

            let callback = VTDecompressionOutputCallbackRecord {
                decompressionOutputCallback: Some(vt_output_callback),
                decompressionOutputRefCon: self.ctx.cast(),
            };
            let mut session_out: *mut VTDecompressionSession = ptr::null_mut();
            let status = VTDecompressionSession::create(
                None,
                &video_format,
                Some(spec.as_opaque()),
                None, // 目标像素属性：默认即 NV12 双平面（回调里按实测几何拷出）
                &callback,
                NonNull::from(&mut session_out),
            );
            if status != 0 || session_out.is_null() {
                return Err(VideoError::NoHardwareDecoder);
            }
            self.session = Some(CFRetained::from_raw(NonNull::new_unchecked(session_out)));
            Ok(())
        }
    }
}

impl DecodeBackend for VtReader {
    fn poll_frame(&mut self) -> Result<Poll, VideoError> {
        if self.ended {
            return Ok(Poll::Eos);
        }

        unsafe {
            loop {
                match self.output.copyNextSampleBuffer() {
                    Some(sb) => {
                        self.ensure_session(&sb)?;
                        let status = self
                            .session
                            .as_ref()
                            .ok_or_else(|| VideoError::Backend("会话未初始化".into()))?
                            .decode_frame(&sb, VTDecodeFrameFlags(0), ptr::null_mut(), ptr::null_mut());
                        drop(sb);
                        if status != 0 {
                            return Err(VideoError::Backend(format!(
                                "VTDecompressionSessionDecodeFrame status={status}"
                            )));
                        }
                        // 阻塞等本帧（VT 重排后按显示序回调，pts 以回调为准）
                        match self.rx.recv() {
                            Ok(Ok(frame)) => {
                                self.position = frame.pts;
                                return Ok(Poll::Frame(frame));
                            }
                            Ok(Err(VideoError::Backend(msg))) if msg.contains("VT 解码帧失败") => {
                                // 单帧解码失败不致命：继续喂（对齐 MF 容错语义）
                                continue;
                            }
                            Ok(Err(e)) => return Err(e),
                            Err(_) => return Ok(Poll::Eos), // 通道关闭 = 已结束
                        }
                    }
                    None => {
                        // 解复用完毕：等在途异步帧全部回调后再结束
                        if let Some(session) = &self.session {
                            session.wait_for_asynchronous_frames();
                        }
                        // 排空残留回调帧
                        loop {
                            match self.rx.try_recv() {
                                Ok(Ok(frame)) => {
                                    self.position = frame.pts;
                                    return Ok(Poll::Frame(frame));
                                }
                                Ok(Err(_)) => continue,
                                Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                                    self.ended = true;
                                    return Ok(Poll::Eos);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn position(&self) -> Duration {
        self.position
    }
}

impl Drop for VtReader {
    fn drop(&mut self) {
        unsafe {
            if let Some(session) = &self.session {
                session.wait_for_asynchronous_frames();
                session.invalidate();
            } // CFRetained 随字段 drop 自动释放
              // 关通道 → 迟到回调静默丢弃
            if let Ok(mut guard) = (*self.ctx).tx.try_borrow_mut() {
                *guard = None;
            }
            drop(Box::from_raw(self.ctx));
            self.reader.cancelReading();
        }
    }
}
