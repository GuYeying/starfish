//! base/video —— 视频硬解抽象（硬解唯一策略：无硬件解码器直接报错，不落软解）
//!
//! 设计定稿：`reference/video硬解抽象设计笔记.md`
//!
//! 模块分区：
//! - [`player`]：播放状态机（主时钟 / 追帧纪律 / 帧纹理管理，平台中立）
//! - [`yuv`]：NV12 → RGBA 整数定点转换（纯函数 + 色值锚点测试）
//! - 平台后端（按平台命名，`DecodeBackend` 为 seam，输出统一 NV12 系统内存）：
//!   · [`windows`]：Media Foundation SourceReader（D3D11 前置硬解探测）
//!   · [`linux`]：GStreamer 硬解聚合（klass=Hardware 过滤，Ubuntu 为准）
//!   · [`apple`]：macOS/iOS VideoToolbox（Require-Hardware 键，零系统依赖）
//!
//! 平台矩阵（设计笔记 §六）：
//! - Windows ✓ / Linux(Ubuntu) ✓ / macOS·iOS ✓
//! - Web ⏳ 批次 B：WebCodecs `hardwareAcceleration:"require"` + mp4 纯 Rust 解复用
//! - Android ⏳ 批次 C：MediaCodec（被引擎层安卓支持阻塞）
//! - 其余平台：`open` 返回 [`VideoError::UnsupportedPlatform`]
//!
//! v1 边界（§九 待定项同步）：
//! - 解码输出 NV12（系统内存）→ CPU 转 RGBA → wgpu 纹理
//!   （YUV→RGB 着色器转换与显存零拷贝 = v2）
//! - 格式承诺收敛：仅 H.264/MP4（各平台统一的最通用格式）
//! - 音轨注入：待 mixer 流声部 API（待定项 7）——v1 为纯画面核心
//!
//! 非侵入式契约：解码/上传只发生在 [`Video::update`]（手动泵），
//! 引擎帧链对 video 模块零感知。

mod player;
mod yuv;

// 解复用共享层：android / web 后端用；test cfg 使 Windows 测试构建亦可编译
// （用真实示例视频做集成测试）
#[cfg(any(target_os = "android", target_arch = "wasm32", test))]
mod mp4_demux;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple;

#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_os = "android")]
mod android;

pub use player::Video;

use std::sync::Arc;
use std::time::Duration;

/// 视频错误
#[derive(Debug)]
pub enum VideoError {
    /// 当前平台暂无解码后端（见设计笔记 §六 平台矩阵）
    UnsupportedPlatform,
    /// 未检测到可用的硬件解码器（硬解唯一策略：不落软解）
    NoHardwareDecoder,
    /// 后端错误（含平台原生信息）
    Backend(String),
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoError::UnsupportedPlatform => write!(f, "video: 该平台暂无解码后端"),
            VideoError::NoHardwareDecoder => {
                write!(f, "video: 未检测到可用的硬件 H.264 解码器（本库不落软解）")
            }
            VideoError::Backend(s) => write!(f, "video 后端错误: {s}"),
        }
    }
}
impl std::error::Error for VideoError {}

/// 解码帧（平台后端的统一输出）
pub(crate) struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub pts: Duration,
    pub pixels: FramePixels,
}

/// 帧像素载体：桌面平台 = NV12 系统内存；Web = 解码器 GPU 帧句柄（零 CPU 转换）
pub(crate) enum FramePixels {
    /// NV12 系统内存（stride 为 Y 行距；色度平面几何由缓冲长度反推）
    Nv12 { nv12: Vec<u8>, stride: usize },
    /// Web：WebCodecs GPU 帧句柄（`copy_external_image_to_texture` 直拷纹理，
    /// 浏览器负责 YUV→RGB）
    #[cfg(target_arch = "wasm32")]
    VideoFrame(WasmVideoFrame),
}

/// [`web_sys::VideoFrame`] 包装：Drop 即 `close()`——追帧丢弃路径及时
/// 释放浏览器侧 GPU 资源（不依赖 GC 终结器）。
#[cfg(target_arch = "wasm32")]
pub(crate) struct WasmVideoFrame(pub(crate) web_sys::VideoFrame);

#[cfg(target_arch = "wasm32")]
impl std::ops::Deref for WasmVideoFrame {
    type Target = web_sys::VideoFrame;
    fn deref(&self) -> &web_sys::VideoFrame {
        &self.0
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for WasmVideoFrame {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// 逐帧拉取结果。桌面同步解码只产生 `Frame`/`Eos`；Web 解码异步，
/// 帧未到但流未结束时为 `Pending`（上层时钟照走，下次 update 继续）。
pub(crate) enum Poll {
    Frame(DecodedFrame),
    Pending,
    Eos,
}

/// 平台解码后端 seam（多平台扩展点：设计笔记 §六 平台矩阵）
///
/// 无 `Send` 约束：COM 接口指针具备套间亲和性，视频按线程契约固定在
/// 主线程（`base/rt` 锚点）——`Video`/`VideoModule` 同样主线程使用。
pub(crate) trait DecodeBackend {
    /// 拉取下一帧 / 在途 / 流结束
    fn poll_frame(&mut self) -> Result<Poll, VideoError>;
    /// 后端就绪（Web fetch/配置未完成时 false：时钟照走、暂不解码）
    fn is_ready(&self) -> bool {
        true
    }
    /// 当前解码位置（主时钟对齐用）
    fn position(&self) -> Duration;
}

/// 视频管理器（init 阶梯成员：依赖渲染设备的 device/queue）
///
/// Rust 侧在 start 中以 `VideoModule` 打开 [`Video`] 句柄；
/// Python 侧对应 `pygame.video.init()` + `pygame.video.open(...)`。
pub struct VideoModule {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl VideoModule {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self { device, queue }
    }

    /// 打开视频文件（惰性：首个 [`update`](Video::update) 才解码首帧）
    ///
    /// 硬解唯一策略：平台无硬件 H.264 解码器时返回 [`VideoError::NoHardwareDecoder`]，
    /// 不落任何软解兜底。
    pub fn open(&self, path: impl Into<String>) -> Result<Video, VideoError> {
        let path = path.into();

        // 平台后端分发（设计笔记 §六 平台矩阵）
        #[cfg(target_os = "windows")]
        let backend: Box<dyn DecodeBackend> = {
            if !windows::hardware_h264_available() {
                return Err(VideoError::NoHardwareDecoder);
            }
            Box::new(
                windows::MfReader::open(&path)
                    .map_err(|e| VideoError::Backend(e.to_string()))?,
            )
        };
        #[cfg(target_os = "linux")]
        let backend: Box<dyn DecodeBackend> = Box::new(linux::GstReader::open(&path)?);
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        let backend: Box<dyn DecodeBackend> = Box::new(apple::VtReader::open(&path)?);
        #[cfg(target_arch = "wasm32")]
        let backend: Box<dyn DecodeBackend> = Box::new(web::WebDecoder::open(&path)?);
        #[cfg(target_os = "android")]
        let backend: Box<dyn DecodeBackend> = Box::new(android::MediaCodecReader::open(&path)?);
        #[cfg(not(any(
            target_os = "windows",
            target_os = "linux",
            target_os = "macos",
            target_os = "ios",
            target_arch = "wasm32",
            target_os = "android"
        )))]
        let backend: Box<dyn DecodeBackend> = {
            let _ = path;
            return Err(VideoError::UnsupportedPlatform);
        };

        Ok(Video::new(backend, self.device.clone(), self.queue.clone()))
    }
}
