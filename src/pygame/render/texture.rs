//! pygame::render::Texture —— GPU 纹理（契约 ADR-1/ADR-3）
//!
//! Rgba8Unorm（gamma 直出，对齐 pygame 无色彩管理行为）；用途三位一体：
//! `COPY_DST | TEXTURE_BINDING | RENDER_ATTACHMENT`——上传、blit 采样、
//! 离屏绘制一纹全通（Surface::from_texture 零拷贝即可作渲染目标）。
//!
//! ⚠️ 直建 wgpu 纹理（不经 `base::render::Texture`）：base 的
//! `TextureUsage` 枚举把用途拆成 Sampled（有 COPY_DST 无 RENDER_ATTACHMENT）
//! 与 RenderTarget（有 RENDER_ATTACHMENT 无 COPY_DST）两态，无法同时满足
//! "带像素上传 + 可采样 + 可作渲染目标"的 pygame 需求——pygame 层自持
//! `Arc<Device>/Arc<Queue>`（依赖双向规则允许），按需直建。绑定面走
//! `BindGroupBuilder::texture_view`（裸视图直绑口，视频帧同款先例）。

use std::sync::Arc;

use crate::base::resources::image::ImageData;

use super::Gpu;

/// GPU 纹理（Rgba8Unorm，2D；采样语义由 [`super::Gpu`] 的默认采样器承担）
#[derive(Clone)]
pub struct Texture {
    pub(crate) raw: Arc<wgpu::Texture>,
    pub(crate) view: Arc<wgpu::TextureView>,
    size: (u32, u32),
}

impl Texture {
    /// 从 RGBA8 像素构造（`pixels.len() == w * h * 4`；须先建 GPU 槽）
    pub fn from_rgba8(size: (u32, u32), pixels: &[u8]) -> Self {
        let gpu = super::gpu();
        upload_rgba8(&gpu.device, &gpu.queue, size, pixels)
    }

    pub(crate) fn from_gpu(gpu: &Gpu, size: (u32, u32), pixels: &[u8]) -> Self {
        upload_rgba8(&gpu.device, &gpu.queue, size, pixels)
    }

    /// 从 CPU 统一图像格式构造（`resources::image::ImageData` 直通）
    pub fn from_image(image: &ImageData) -> Self {
        match image {
            ImageData::Rgba8(img) => Self::from_rgba8((img.width(), img.height()), img.as_raw()),
            ImageData::Rgb8(img) => {
                // RGB → RGBA（补 alpha=255）
                let mut rgba = Vec::with_capacity((img.width() * img.height() * 4) as usize);
                for p in img.pixels() {
                    rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
                }
                Self::from_rgba8((img.width(), img.height()), &rgba)
            }
            ImageData::Compressed(_) => {
                panic!("Texture::from_image: 压缩纹理 v1 不支持（pygame 层按需立项）")
            }
        }
    }

    /// 从通用数据桥上传构造（ADR-10：Texture↔CPU 最小路径的入端）
    pub fn from_buffer(proxy: &super::BufferProxy) -> Self {
        Self::from_rgba8(proxy.size(), proxy.as_bytes())
    }

    /// 纹理尺寸（像素）
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// 默认视图（全 mip 全层）
    pub(crate) fn view(&self) -> Arc<wgpu::TextureView> {
        self.view.clone()
    }

    /// bind group 缓存键（纹理本体指针；Arc 由段持有，键恒有效）
    pub(crate) fn cache_key(&self) -> usize {
        Arc::as_ptr(&self.raw) as usize
    }
}

/// 直建 wgpu 纹理 + 像素上传（Rgba8Unorm；COPY_DST | TEXTURE_BINDING |
/// RENDER_ATTACHMENT 三位一体——见模块文档的 base 用途枚举拆分说明）
pub(crate) fn upload_rgba8(
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    size: (u32, u32),
    pixels: &[u8],
) -> Texture {
    let (w, h) = (size.0.max(1), size.1.max(1));
    assert!(
        pixels.len() == (w * h * 4) as usize,
        "Texture::from_rgba8: 像素长度 {} 与 {w}×{h}×4 不符",
        pixels.len()
    );
    let raw = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pygame_texture"),
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm, // ADR-3：gamma 直出
        usage: wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &raw,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let view = Arc::new(raw.create_view(&Default::default()));
    Texture {
        raw: Arc::new(raw),
        view,
        size: (w, h),
    }
}
