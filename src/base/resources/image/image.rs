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