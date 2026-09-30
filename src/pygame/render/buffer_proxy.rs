//! pygame::render::BufferProxy —— 通用数据桥（契约 ADR-10 定型）
//!
//! 吸取 surfarray/PixelArray/pixelcopy 家族的底层互换类型经验：各类数据
//! （CPU 字节 / GPU 缓冲 / 音频样本）的统一视图与搬运载体。v1 = 类型定型
//! + **Texture↔CPU 最小路径**（CPU → GPU 上传，见
//! [`Texture::from_buffer`]）；GPU 回读慢路径、sndarray 端点等 v2 逐个挂。
//!
//! "模块可不做、地基必须对"：后续任何"数据从 A 形态到 B 形态"的需求都走
//! 同一座桥，不再各造各的轮子。

/// 像素格式（v1 单一 Rgba8；后续端点按需扩展）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGBA8，每像素 4 字节
    Rgba8,
}

/// 数据布局：尺寸 + 格式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
}

impl Layout {
    /// 该布局的字节数
    pub fn byte_len(&self) -> usize {
        let bpp = match self.format {
            PixelFormat::Rgba8 => 4,
        };
        (self.width as usize) * (self.height as usize) * bpp
    }
}

/// 各类数据的统一视图与搬运载体（v1 = CPU 驻留字节）
#[derive(Debug, Clone)]
pub struct BufferProxy {
    layout: Layout,
    data: Vec<u8>,
}

impl BufferProxy {
    /// 从字节构造（长度与布局严格校验）
    pub fn from_bytes(layout: Layout, data: &[u8]) -> Result<Self, String> {
        if data.len() != layout.byte_len() {
            return Err(format!(
                "BufferProxy::from_bytes: 数据长度 {} 与布局 {:?}（{} 字节）不符",
                data.len(),
                layout,
                layout.byte_len()
            ));
        }
        Ok(Self {
            layout,
            data: data.to_vec(),
        })
    }

    /// 布局
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// 尺寸（像素）
    pub fn size(&self) -> (u32, u32) {
        (self.layout.width, self.layout.height)
    }

    /// CPU 侧直读
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_len_validation() {
        let layout = Layout {
            width: 4,
            height: 3,
            format: PixelFormat::Rgba8,
        };
        assert_eq!(layout.byte_len(), 48);

        let ok = BufferProxy::from_bytes(layout, &vec![0u8; 48]);
        assert!(ok.is_ok());
        let proxy = ok.unwrap();
        assert_eq!(proxy.size(), (4, 3));
        assert_eq!(proxy.as_bytes().len(), 48);

        let bad = BufferProxy::from_bytes(layout, &[0u8; 47]);
        assert!(bad.is_err());
    }
}
