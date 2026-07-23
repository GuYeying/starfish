
use std::sync::Arc;

use wgpu::{
    PipelineLayout,
    RenderPipeline as WgpuRenderPipeline
};

pub struct RenderPipeline {

    pipeline: WgpuRenderPipeline,
    layout:   Arc<PipelineLayout>
}


impl RenderPipeline{
    pub fn new(pipeline: WgpuRenderPipeline, layout: Arc<PipelineLayout>) -> Self {
        Self { pipeline, layout }
    }

    pub fn pipeline(&self)->&WgpuRenderPipeline{
        &self.pipeline
    }
    
    pub fn layout(&self)->Arc<PipelineLayout>{
        self.layout.clone()
    }
}
