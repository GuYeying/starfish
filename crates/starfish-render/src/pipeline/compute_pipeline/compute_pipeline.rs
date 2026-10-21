
use std::sync::Arc;

use wgpu::{
    PipelineLayout,
    ComputePipeline as WgpuComputePipeline
};

pub struct ComputePipeline {

    pipeline: WgpuComputePipeline,
    layout:   Arc<PipelineLayout>
}


impl ComputePipeline{
    pub fn new(pipeline: WgpuComputePipeline, layout: Arc<PipelineLayout>) -> Self {
        Self { pipeline, layout }
    }

    pub fn pipeline(&self)->&WgpuComputePipeline{
        &self.pipeline
    }
    
    pub fn layout(&self)->Arc<PipelineLayout>{
        self.layout.clone()
    }
}

