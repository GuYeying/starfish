use image::RgbaImage;
use image::RgbImage;

use wgpu::TextureFormat;

pub trait CompressedImage: Send + Sync {
    fn width(&self) -> u32;

    fn height(&self) -> u32;

    fn mip_levels(&self) -> u32;

    fn texture_format(&self) -> TextureFormat;

    fn bytes(&self) -> &[u8];
}


/// CPU统一图像格式
pub enum ImageData {
    /// RGBA8
    Rgba8(RgbaImage),

    /// RGB8
    Rgb8(RgbImage),

    /// GPU压缩纹理（DDS/KTX/KTX2/Basis等）
    Compressed(Box<dyn CompressedImage>),
}

impl ImageData {
    /// 零填充/自带字节的 RGBA8 图像构造（绑定层便捷口——Python 侧免依赖
    /// image crate）。`bytes` 长度须 = w×h×4，不符时 Err。
    pub fn rgba8(width: u32, height: u32, bytes: Vec<u8>) -> Result<Self, ()> {
        match RgbaImage::from_raw(width, height, bytes) {
            Some(img) => Ok(Self::Rgba8(img)),
            None => Err(()),
        }
    }
}



//为了后期能兼容更多更好的纹理文件，暂时实现一个Image抽象类型，这个是作为中间层，也就是各种类型通过转换成Image，然后再通过Image上传到GPU纹理中
pub(crate) struct Image <'a>{
    image: &'a ImageData,
}

impl <'a> Image <'a>{
    pub fn new(image: &'a ImageData) -> Self {
        Self { image }
    }

    pub fn width(&self) -> u32 {
        match &self.image {
            ImageData::Rgba8(img) => img.width(),
            ImageData::Rgb8(img) => img.width(),
            ImageData::Compressed(compressed) => compressed.width(),
        }
    }

    pub fn height(&self) -> u32 {
        match &self.image {
            ImageData::Rgba8(img) => img.height(),
            ImageData::Rgb8(img) => img.height(),
            ImageData::Compressed(compressed) => compressed.height(),
        }
    }

    pub fn data(&self) -> &[u8] {
        match &self.image {
            ImageData::Rgba8(img) => img.as_raw(),
            ImageData::Rgb8(img) => img.as_raw(),
            ImageData::Compressed(compressed) => compressed.bytes(),
        }
    }


    pub fn size(&self) -> (u32, u32) {
        (self.width(), self.height())
    }

    pub fn inner(&self) -> &ImageData {
        &self.image
    }
}