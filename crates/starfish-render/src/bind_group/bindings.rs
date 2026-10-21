use std::sync::{Arc};
use wgpu::{Buffer, Sampler, ShaderStages};

pub enum BindResource {
    Uniform {
        buffer: Arc<Buffer>,
        size: Option<u64>,
    },
    Storage {
        buffer: Arc<Buffer>,
        size: Option<u64>,
        read_only: bool,
    },
    Texture {
        view: Arc<wgpu::TextureView>,
    },
    /// 纹理视图数组（bindless 基础；layout count = views.len()）
    TextureArray {
        views: Vec<Arc<wgpu::TextureView>>,
    },
    /// 存储缓冲数组（compute 场景）
    StorageArray {
        buffers: Vec<Arc<Buffer>>,
        read_only: bool,
    },
    Sampler{
        sampler: Arc<Sampler>,
    },
}



pub struct BindItem {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub resource: BindResource,
}