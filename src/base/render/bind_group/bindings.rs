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
    Sampler{
        sampler: Arc<Sampler>,
    },
}



pub struct BindItem {
    pub binding: u32,
    pub visibility: ShaderStages,
    pub resource: BindResource,
}