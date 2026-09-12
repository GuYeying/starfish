//! 视频播放状态机（平台中立：主时钟 / 追帧纪律 / 帧纹理管理）
//!
//! [`Video`] 是应用持有的播放句柄：解码/上传只发生在 [`Video::update`]
//! （手动泵），引擎帧链对 video 模块零感知。

use std::sync::Arc;
use std::time::Duration;

use super::{yuv, DecodeBackend, DecodedFrame, FramePixels, Poll, VideoError};

/// 视频播放句柄（对象直绑：自带解码状态机与帧纹理）
pub struct Video {
    backend: Box<dyn DecodeBackend>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    texture: Option<wgpu::Texture>,
    /// 帧纹理默认视图（与 texture 同生命周期：纹理重建时换新）
    view: Option<Arc<wgpu::TextureView>>,
    size: (u32, u32),
    /// 主时钟（有音轨时未来 = 音轨位置；v1 = 累计 dt）
    clock: Duration,
    /// 解码位置
    position: Duration,
    /// 视频解码开关（遮挡场景：关 = 只走音频/冻结画面）
    enabled: bool,
    ended: bool,
}

impl Video {
    pub(super) fn new(
        backend: Box<dyn DecodeBackend>,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    ) -> Self {
        Self {
            backend,
            device,
            queue,
            texture: None,
            view: None,
            size: (0, 0),
            clock: Duration::ZERO,
            position: Duration::ZERO,
            enabled: true,
            ended: false,
        }
    }

    /// 非侵入式手动泵：推进解码到主时钟、上传最新帧纹理。
    ///
    /// - `enabled = false`（遮挡）：时钟继续推进，解码停——恢复时自动追帧
    /// - 追帧（解码落后主时钟多帧）只上传最新帧，中间帧解码即弃
    /// - Web 后端帧在途（`Poll::Pending`）：时钟照走，本泵提前收工
    /// - 流结束：`ended()` 置位，后续调用为 no-op
    pub fn update(&mut self, dt: Duration) -> Result<(), VideoError> {
        if self.ended || !self.backend.is_ready() {
            return Ok(());
        }
        self.clock += dt;
        let mut pending: Option<DecodedFrame> = None;

        // 遮挡关闭时解码停走：恢复（enabled 翻回）后 position 落后于 clock，
        // 下方循环会快速连续解码（不上屏）追至主时钟——无需 seek。
        while self.enabled && self.position < self.clock {
            match self.backend.poll_frame()? {
                Poll::Frame(frame) => {
                    self.position = frame.pts;
                    // 追帧只呈现最新：落后多帧时中间帧解码即弃，
                    // 跳过 CPU 转换与纹理上传（二者占单帧成本绝大部分）
                    if let Some(prev) = pending.take() {
                        drop(prev);
                    }
                    pending = Some(frame);
                }
                Poll::Pending => break,
                Poll::Eos => {
                    self.ended = true;
                    break;
                }
            }
        }
        // 仅本次泵的最后一帧上屏（含流结束前的末帧）
        if let Some(frame) = pending.take() {
            self.upload(&frame);
        }
        Ok(())
    }

    /// 将解码帧上传纹理（首帧懒创建，后续整帧覆写）
    fn upload(&mut self, frame: &DecodedFrame) {
        let size = wgpu::Extent3d {
            width: frame.width,
            height: frame.height,
            depth_or_array_layers: 1,
        };

        match &self.texture {
            // 同尺寸：整帧覆写
            Some(t) if (t.width(), t.height()) == (frame.width, frame.height) => {
                copy_frame_pixels(&self.queue, t, frame, size);
            }
            _ => {
                // Web 上 `copy_external_image_to_texture` 要求目标含 RENDER_ATTACHMENT
                let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
                #[cfg(target_arch = "wasm32")]
                let usage = usage | wgpu::TextureUsages::RENDER_ATTACHMENT;
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("starfish.video"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage,
                    view_formats: &[],
                });
                let view = Arc::new(texture.create_view(&wgpu::TextureViewDescriptor::default()));
                copy_frame_pixels(&self.queue, &texture, frame, size);
                self.texture = Some(texture);
                self.view = Some(view);
                self.size = (frame.width, frame.height);
            }
        }
    }

    /// 帧纹理（YUV 已转 RGBA；未解码首帧时 None）
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.texture.as_ref()
    }

    /// 帧纹理默认视图——外部纹理源接入采样管线的直绑口
    ///
    /// `BindGroupBuilder::texture_view` 以裸视图入参，可直接装配 bind group；
    /// 同尺寸覆写路径不重建纹理，视图稳定（绑定一次管到底）。
    /// 未解码首帧时 None。
    pub fn texture_view(&self) -> Option<Arc<wgpu::TextureView>> {
        self.view.clone()
    }

    /// 帧尺寸（首个解码帧确定；此前为 0×0）
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// 解码位置（主时钟）
    pub fn position(&self) -> Duration {
        self.position
    }

    /// 视频解码开关（遮挡场景：关 = 只解音频侧/冻结画面，时钟继续走）
    pub fn set_video_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn video_enabled(&self) -> bool {
        self.enabled
    }

    /// 流已结束
    pub fn ended(&self) -> bool {
        self.ended
    }
}

/// 帧像素 → 纹理：NV12 走 CPU 定点转换 + write_texture；
/// Web VideoFrame 走 GPU 直拷（浏览器完成 YUV→RGB），拷完即关帧。
fn copy_frame_pixels(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    frame: &DecodedFrame,
    size: wgpu::Extent3d,
) {
    match &frame.pixels {
        FramePixels::Nv12 { nv12, stride } => {
            let rgba = yuv::nv12_to_rgba(nv12, *stride, frame.width, frame.height);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                size,
            );
        }
        #[cfg(target_arch = "wasm32")]
        FramePixels::VideoFrame(video_frame) => {
            let src = wgpu::CopyExternalImageSourceInfo {
                // 注意：VideoFrame 的固有 clone() 是 JS 语义（返回 Result），
                // 这里用 trait 形式调用 derive 的 Rust Clone
                source: wgpu::ExternalImageSource::VideoFrame(std::clone::Clone::clone(
                    &video_frame.0,
                )),
                origin: wgpu::Origin2d::ZERO,
                flip_y: false,
            };
            let dst = wgpu::CopyExternalImageDestInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
                color_space: wgpu::PredefinedColorSpace::Srgb,
                premultiplied_alpha: false,
            };
            queue.copy_external_image_to_texture(&src, dst, size);
            // WasmVideoFrame Drop 即 close()：GPU 拷贝已入队，帧可安全释放
        }
    }
}
