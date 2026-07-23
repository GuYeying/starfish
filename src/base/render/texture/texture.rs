//! 引擎纹理系统核心模块
//! 功能：2D / CubeMap / 纹理数组 / 深度缓冲 / 阴影贴图 / PBR 纹理全支持
//! CPU 资源统一使用：Image (ImageBuffer<Rgba<u8>)
//! GPU 层自动映射格式、Usage、View 类型

use std::collections::HashMap;
use std::sync::{Arc, Mutex};


use wgpu::{
    Device, Queue,
    Texture as WgpuTexture, TextureView,
    TextureFormat,
    TextureViewDimension,
    Extent3d, Origin3d,
    TextureAspect
};

use wgpu::{TexelCopyTextureInfo, TexelCopyBufferLayout};

use crate::base::resources::image::Image;

use super::texture_desc::{TextureDescriptor, TextureDim, TextureSemantic};

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct ViewKey {
    pub base_mip: u32,
    pub mip_count: u32,
    pub base_layer: u32,
    pub layer_count: Option<u32>,
    pub dimension: TextureViewDimension,
    pub aspect: TextureAspect,
}


// -----------------------------------------------------------------------------
// GPU 纹理对象
// -----------------------------------------------------------------------------
pub struct Texture {
    texture: WgpuTexture,
    default_view: Arc<TextureView>,
    views: Mutex<HashMap<ViewKey, Arc<TextureView>>>, // 内部可变性，无需&mut self
    desc: TextureDescriptor,
    label: String,
    size: Extent3d,
    mipmap_generated: bool,
}

impl Texture {
    /// 从RGBA8图像创建2D纹理
    pub(crate) fn from_rgba8_2d(
        device: &Device,
        queue: &Queue,
        label: &str,
        image: &Image,
        desc: TextureDescriptor,
    ) -> Self {
        assert!(matches!(desc.dimension, TextureDim::D2));
        let size = Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: desc.depth_or_array_layers(),
        };
        Self::create_and_upload(device, queue, label, size, image, desc, Origin3d::ZERO)
    }

    /// 创建立方体贴图
    pub(crate) fn from_rgba8_cube(
        device: &Device,
        queue: &Queue,
        label: &str,
        faces: &[Image; 6],
        desc: TextureDescriptor,
    ) -> Self {
        assert!(matches!(desc.dimension, TextureDim::Cube));
        let image = &faces[0];
        let size = Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: 6,
        };

        let texture = Self::create_texture(device, label, size, &desc);
        for (z, face) in faces.iter().enumerate() {
            Self::upload_single_layer(
                queue,
                &texture,
                face,
                0,
                Origin3d { x: 0, y: 0, z: z as u32 },
                &desc,
            );
        }

        Self::build_texture_object(label.to_string(), texture, size, desc)
    }

    /// 2D纹理数组
    pub(crate) fn from_rgba8_array(
        device: &Device,
        queue: &Queue,
        label: &str,
        images: &[Image],
        desc: TextureDescriptor,
    ) -> Self {
        assert!(matches!(desc.dimension, TextureDim::D2Array));
        assert!(!images.is_empty());
        let image = &images[0];
        let layers = images.len() as u32;
        let size = Extent3d {
            width: image.width(),
            height: image.height(),
            depth_or_array_layers: layers,
        };

        let texture = Self::create_texture(device, label, size, &desc);
        for (z, img) in images.iter().enumerate() {
            Self::upload_single_layer(
                queue,
                &texture,
                img,
                0,
                Origin3d { x: 0, y: 0, z: z as u32 },
                &desc,
            );
        }

        Self::build_texture_object(label.to_string(), texture, size, desc)
    }

    /// 1D纹理
    pub(crate) fn from_rgba8_1d(
        device: &Device,
        queue: &Queue,
        label: &str,
        image: &Image,
        desc: TextureDescriptor,
    ) -> Self {
        assert!(matches!(desc.dimension, TextureDim::D1));

        let size = Extent3d {
            width: image.width(),
            height: 1,
            depth_or_array_layers: desc.depth_or_array_layers(),
        };
        Self::create_and_upload(device, queue, label, size, image, desc, Origin3d::ZERO)
    }

    /// 3D体积纹理
    pub(crate) fn from_rgba8_3d(
        device: &Device,
        queue: &Queue,
        label: &str,
        images: &[Image],
        desc: TextureDescriptor,
    ) -> Self {
        assert!(matches!(desc.dimension, TextureDim::D3));
        assert!(!images.is_empty());
        let first_img = &images[0];
        let depth = images.len() as u32;

        let size = Extent3d {
            width: first_img.width(),
            height: first_img.height(),
            depth_or_array_layers: depth,
        };

        let texture = Self::create_texture(device, label, size, &desc);
        for (z, img) in images.iter().enumerate() {
            Self::upload_single_layer(
                queue,
                &texture,
                img,
                0,
                Origin3d { x: 0, y: 0, z: z as u32 },
                &desc,
            );
        }

        Self::build_texture_object(label.to_string(), texture, size, desc)
    }

    // -------------------------- 内部工具方法 --------------------------
    fn create_texture(
        device: &Device,
        label: &str,
        size: Extent3d,
        desc: &TextureDescriptor,
    ) -> WgpuTexture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: desc.mip_level_count(size.width, size.height),
            sample_count: desc.sample_count(),
            dimension: desc.gpu_dimension(),
            format: desc.format(),
            usage: desc.usage_flags(),
            view_formats: &[],
        })
    }

    fn create_default_view(texture: &WgpuTexture, desc: &TextureDescriptor) -> TextureView {
        let mip_cnt = desc.mip_level_count(texture.size().width, texture.size().height);
        let aspect = match desc.semantic {
            TextureSemantic::Depth => TextureAspect::DepthOnly,
            _ => TextureAspect::All,
        };

        texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(desc.view_dimension()),
            aspect,
            base_mip_level: 0,
            mip_level_count: Some(mip_cnt),
            base_array_layer: 0,
            array_layer_count: Some(desc.depth_or_array_layers()),
            ..Default::default()
        })
    }

    fn upload_single_layer(
        queue: &Queue,
        texture: &WgpuTexture,
        image: &Image,
        mip_level: u32,
        origin: Origin3d,
        desc: &TextureDescriptor,
    ) {
        let aspect = match desc.semantic {
            TextureSemantic::Depth => TextureAspect::DepthOnly,
            _ => TextureAspect::All,
        };

        queue.write_texture(
            TexelCopyTextureInfo {
                texture,
                mip_level,
                origin,
                aspect,
            },
            image.data(),
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * image.width()),
                rows_per_image: Some(image.height()),
            },
            Extent3d {
                width: image.width(),
                height: image.height(),
                depth_or_array_layers: 1,
            },
        );
    }

    fn create_and_upload(
        device: &Device,
        queue: &Queue,
        label: &str,
        size: Extent3d,
        image: &Image,
        desc: TextureDescriptor,
        origin: Origin3d,
    ) -> Self {
        let texture = Self::create_texture(device, label, size, &desc);
        Self::upload_single_layer(queue, &texture, image, 0, origin, &desc);
        Self::build_texture_object(label.to_string(), texture, size, desc)
    }

    /// 统一构造Texture实例（填充default_view、缓存容器、size、label）
    fn build_texture_object(
        label: String,
        texture: WgpuTexture,
        size: Extent3d,
        desc: TextureDescriptor,
    ) -> Self {
        let default_view = Arc::new(Self::create_default_view(&texture, &desc));
        Self {
            texture,
            default_view,
            views: Mutex::new(HashMap::new()),
            desc,
            label,
            size,
            mipmap_generated: false,
        }
    }

    // -------------------------- 统一视图接口（核心改造点） --------------------------
    /// 全局唯一视图获取入口，自动缓存，不需要&mut self
    pub fn get_view(&self, key: &ViewKey) -> Arc<TextureView> {
        // 先尝试读取缓存
        let mut cache = self.views.lock().unwrap();
        if let Some(view) = cache.get(key) {
            return view.clone();
        }

        // 缓存未命中，新建视图
        let new_view = Arc::new(self.texture.create_view(&wgpu::TextureViewDescriptor {
            label: None,
            format: Some(self.desc.format()),
            dimension: Some(key.dimension),
            aspect: key.aspect,
            base_mip_level: key.base_mip,
            mip_level_count: Some(key.mip_count),
            base_array_layer: key.base_layer,
            array_layer_count: key.layer_count,
            ..Default::default()
        }));

        // 存入缓存
        cache.insert(key.clone(), new_view.clone());
        new_view
    }

    // -------------------------- 成员访问接口 --------------------------
    pub fn set_mipmap_generated(&mut self, generated: bool) {
        self.mipmap_generated = generated;
    }

    pub fn is_mipmap_generated(&self) -> bool {
        self.mipmap_generated
    }

    pub fn desc(&self) -> &TextureDescriptor {
        &self.desc
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn size(&self) -> Extent3d {
        self.size
    }

    pub fn mip_count(&self) -> u32 {
        self.desc.mip_level_count(self.size.width, self.size.height)
    }

    pub fn need_mipmap(&self) -> bool {
        self.mip_count() > 1
    }

    pub fn format(&self) -> TextureFormat {
        self.desc.format()
    }

    pub fn inner(&self) -> &WgpuTexture {
        &self.texture
    }

    /// 获取默认完整视图（常规渲染绑定用）
    pub fn default_view(&self) -> Arc<TextureView> {
        self.default_view.clone()
    }
}