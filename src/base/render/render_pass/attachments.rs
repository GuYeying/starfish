use std::sync::{Arc};
use wgpu::{Color,Operations,TextureView,};

pub use wgpu::{StoreOp,LoadOp};




/// Color Attachment
#[derive(Clone)]
pub struct ColorAttachment {
    pub view: Arc<TextureView>,

    pub load: LoadOp<Color>,

    pub store: StoreOp,

    pub resolve_target: Option<Arc<TextureView>>,

    pub depth_slice: Option<u32>,
}


/// Depth Attachment
#[derive(Clone)]
pub struct DepthAttachment {
    pub view: Arc<TextureView>,

    pub load: LoadOp<f32>,

    pub store: StoreOp,

    pub stencil_ops: Option<Operations<u32>>,

    pub depth_slice: Option<u32>,
}



impl ColorAttachment {
    pub(crate) fn to_wgpu(
        &self,
    ) -> wgpu::RenderPassColorAttachment<'_> {
        wgpu::RenderPassColorAttachment {
            view: &self.view,

            resolve_target: self.resolve_target.as_deref(),

            ops: wgpu::Operations {
                load: self.load,
                store: self.store,
            },

            depth_slice: self.depth_slice,
        }
    }
}

impl DepthAttachment {
    pub(crate) fn to_wgpu(
        &self,
    ) -> wgpu::RenderPassDepthStencilAttachment<'_> {
        wgpu::RenderPassDepthStencilAttachment {
            view: &self.view,

            depth_ops: Some(wgpu::Operations {
                load: self.load,
                store: self.store,
            }),

            stencil_ops: self.stencil_ops,
        }
    }
}