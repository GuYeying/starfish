//! pygame.image 对应物（契约 §五 image 行：load/save → Surface）
//!
//! 解码基于 `image` crate（PNG/JPEG/BMP/GIF/WebP 等全支持），经
//! `resources::image::ImageData`（CPU 统一图像格式）→ [`Texture`] →
//! [`Surface`]——契约定稿的"基于 resources/image 封装"路线。
//!
//! 加载：
//! - [`load`]：文件路径（桌面同步；Web 上 std::fs 不可用会报错）；
//! - [`load_async`]：**全平台口**（走 `base::io`——原生 fs / Web fetch）；
//! - [`load_from_bytes`]：内存字节（include_bytes!/内嵌资产）；
//! - [`get_extended`]：垫底恒 true（解码器内建）。
//!
//! 保存（**GPU 回读慢路径**，ADR-2 分期 v2、本批落地）：
//! - [`save_async`]：全平台（回读 → 编码 → `base::io::write`——原生落盘
//!   / Web POST 到端点）；
//! - [`save`]：桌面同步壳（Web 无阻塞模型不提供，用 save_async）。
//!
//! 回读机理：`copy_texture_to_buffer`（bytes_per_row 按 256 对齐）→
//! `map_async` 等待（桌面 `poll(Wait)` 阻塞驱动 / Web `poll(Poll)` +
//! rAF 让出重试）→ 去行填充 → RGBA8。

use crate::base::io as engine_io;
use crate::base::resources::image::ImageData;
use crate::pygame::render::{gpu, DrawTarget, Surface, Texture};

/// 图像加载/保存错误
#[derive(Debug)]
pub enum ImageError {
    /// 文件读取失败（仅 [`load`]）
    Io(std::io::Error),
    /// 解码/编码失败（格式不支持 / 数据损坏）
    Decode(image::ImageError),
    /// 跨平台 IO 后端错误（`base::io`；Web fetch 非 2xx 等）
    Backend(engine_io::IoError),
    /// GPU 回读失败（映射错误等）
    Readback(String),
}

impl std::fmt::Display for ImageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImageError::Io(e) => write!(f, "图像文件读取失败: {e}"),
            ImageError::Decode(e) => write!(f, "图像编码/解码失败: {e}"),
            ImageError::Backend(e) => write!(f, "图像 IO 后端错误: {e}"),
            ImageError::Readback(e) => write!(f, "GPU 回读失败: {e}"),
        }
    }
}

impl std::error::Error for ImageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImageError::Io(e) => Some(e),
            ImageError::Decode(e) => Some(e),
            ImageError::Backend(e) => Some(e),
            ImageError::Readback(_) => None,
        }
    }
}

impl From<std::io::Error> for ImageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<image::ImageError> for ImageError {
    fn from(e: image::ImageError) -> Self {
        Self::Decode(e)
    }
}

/// 从文件路径加载图像为 Surface（桌面文件系统；对位 `pygame.image.load`）
pub fn load(path: &str) -> Result<Surface, ImageError> {
    let bytes = std::fs::read(path)?;
    load_from_bytes(&bytes)
}

/// 从内存字节加载（全平台口；对位 `pygame.image.load(fileobj)` 形态）
pub fn load_from_bytes(bytes: &[u8]) -> Result<Surface, ImageError> {
    let data = decode(bytes)?;
    let texture = Texture::from_image(&data);
    Ok(Surface::from_texture(texture))
}

/// 解码为 CPU 统一图像格式（RGBA8；纯逻辑，可测）
fn decode(bytes: &[u8]) -> Result<ImageData, ImageError> {
    let img = image::load_from_memory(bytes)?;
    Ok(ImageData::Rgba8(img.to_rgba8()))
}

/// 是否带扩展解码支持（垫底原则：解码器内建，恒 true）
pub fn get_extended() -> bool {
    true
}

// ── 跨平台 IO（base::io 直通：原生 fs / Web fetch）──────────────

/// 异步加载（全平台；走 `base::io::read`——原生 fs / Web fetch GET）
pub async fn load_async(path: &str) -> Result<Surface, ImageError> {
    let bytes = engine_io::read(path).await.map_err(ImageError::Backend)?;
    load_from_bytes(&bytes)
}

/// 保存 Surface 为图像文件（**全平台异步**；走 `base::io::write`——原生
/// 落盘 / Web POST 到端点）。格式按扩展名推断（png/jpg/bmp/tga…），
/// 无扩展名默认 PNG。
///
/// 仅接受离屏 [`Surface`]（纹理可回读）；`Screen` 背板是交换链（ADR-2
/// 不回读）。Web 语义 = 字节 POST 到 `path` 端点，由服务端落盘。
pub async fn save_async(surface: &Surface, path: &str) -> Result<(), ImageError> {
    surface.flush(); // 未打包的绘制先落纹理（对齐 blit 自动 flush 语义）
    let rgba = readback_rgba(surface).await?;
    let bytes = encode_for_path(&rgba, surface.size(), path)?;
    engine_io::write(path, bytes)
        .await
        .map_err(ImageError::Backend)
}

/// 保存（桌面同步壳 = `block_on(save_async)`；**Web 不提供**——无阻塞
/// 模型，同步等待 GPU 回读/fetch 会冻结页面，用 [`save_async`]）
#[cfg(not(target_arch = "wasm32"))]
pub fn save(surface: &Surface, path: &str) -> Result<(), ImageError> {
    crate::base::app::block_on(save_async(surface, path))
}

// ── GPU 回读（慢路径；ADR-2 v2 分期的实现）───────────────────────

/// bytes_per_row 的 256 对齐（copy_texture_to_buffer 硬性要求）
fn padded_bpr(width: u32) -> u32 {
    (width * 4).div_ceil(256) * 256
}

/// 去行填充：GPU 缓冲（带 padding 行）→ 紧凑 RGBA
fn unpad_rows(padded: &[u8], width: u32, height: u32, bpr: u32) -> Vec<u8> {
    let row = (width * 4) as usize;
    let mut out = Vec::with_capacity(row * height as usize);
    for y in 0..height as usize {
        let start = y * bpr as usize;
        out.extend_from_slice(&padded[start..start + row]);
    }
    out
}

/// GPU 回读 Surface 像素（紧凑 RGBA8；mask/save 共享的慢路径）
pub(crate) async fn readback_rgba(surface: &Surface) -> Result<Vec<u8>, ImageError> {
    let gpu = gpu();
    let (w, h) = surface.size();
    let bpr = padded_bpr(w);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pygame_readback"),
        size: (bpr * h) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &surface.texture.raw,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bpr),
                rows_per_image: Some(h),
            },
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);

    // 映射等待：桌面 poll(Wait) 阻塞驱动；Web poll(Poll) + rAF 让出重试
    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    loop {
        match rx.try_recv() {
            Ok(Ok(())) => break,
            Ok(Err(e)) => return Err(ImageError::Readback(e.to_string())),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(ImageError::Readback("映射回调通道断开".into()));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let _ = gpu.device.poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: None,
                    });
                }
                #[cfg(target_arch = "wasm32")]
                {
                    gpu.device.poll(wgpu::PollType::Poll);
                    crate::base::window::next_frame().await; // rAF 让出（勿冻页面）
                }
            }
        }
    }
    let data = buffer
        .get_mapped_range(..)
        .map_err(|e| ImageError::Readback(e.to_string()))?
        .to_vec();
    buffer.unmap();
    Ok(unpad_rows(&data, w, h, bpr))
}

/// RGBA → 目标格式字节（扩展名推断：png 默认 / jpg / bmp / tga…）
fn encode_for_path(rgba: &[u8], size: (u32, u32), path: &str) -> Result<Vec<u8>, ImageError> {
    let img = image::RgbaImage::from_raw(size.0, size.1, rgba.to_vec())
        .ok_or_else(|| ImageError::Readback("像素长度与尺寸不符".into()))?;
    let fmt = image::ImageFormat::from_path(path).unwrap_or(image::ImageFormat::Png);
    // JPEG 无 alpha 通道：RGBA → RGB 后编码
    let dyn_img = if matches!(fmt, image::ImageFormat::Jpeg) {
        image::DynamicImage::ImageRgb8(
            image::DynamicImage::ImageRgba8(img).into_rgb8(),
        )
    } else {
        image::DynamicImage::ImageRgba8(img)
    };
    let mut out = std::io::Cursor::new(Vec::new());
    dyn_img
        .write_to(&mut out, fmt)
        .map_err(ImageError::Decode)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpr_alignment_and_unpad() {
        // 476×4 = 1904 → 对齐到 2048
        assert_eq!(padded_bpr(476), 2048);
        assert_eq!(padded_bpr(64), 256); // 恰好对齐不虚增

        // 去行填充：3×2 图，bpr=8（每行尾部 4 字节填充）
        let padded = [
            1, 2, 3, 4, 9, 9, 9, 9, // 行 0
            5, 6, 7, 8, 9, 9, 9, 9, // 行 1
        ];
        assert_eq!(unpad_rows(&padded, 1, 2, 8), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    /// 1×1 白色 PNG（手工构造的最小合法图像字节）
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
        0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
        0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08,
        0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D,
        0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn decode_png_dimensions() {
        let data = decode(TINY_PNG).expect("合法 PNG 应解码成功");
        match data {
            ImageData::Rgba8(img) => {
                assert_eq!((img.width(), img.height()), (1, 1));
                assert_eq!(img.as_raw().len(), 4);
            }
            _ => panic!("decode 应产出 RGBA8"),
        }
    }

    #[test]
    fn decode_garbage_errors() {
        assert!(matches!(decode(b"not an image"), Err(ImageError::Decode(_))));
    }

    #[test]
    fn get_extended_is_true() {
        assert!(get_extended());
    }
}
