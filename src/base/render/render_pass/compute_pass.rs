use wgpu::ComputePass as WgpuComputePass;

use crate::base::render::{
    bind_group::bind_group::BindGroup,
    pipeline::ComputePipeline,
};

pub struct ComputePass<'a> {
    inner: WgpuComputePass<'a>,
    bind_group_order: u32,
}

impl<'a> ComputePass<'a> {
    pub fn new(inner: WgpuComputePass<'a>) -> Self {
        Self {
            inner,
            bind_group_order: 0,
        }
    }

    pub fn set_pipeline(&mut self, pipeline: &ComputePipeline) {
        self.inner.set_pipeline(pipeline.pipeline());
        self.bind_group_order = 0;
    }

    pub fn set_bind_group(&mut self, group: &BindGroup) {
        self.inner.set_bind_group(
            self.bind_group_order,
            group.bind_group(),
            &[],
        );
        self.bind_group_order += 1;
    }

    pub fn set_bind_groups(
        &mut self,
        groups: &[&BindGroup],
    ) {
        for group in groups {
            self.set_bind_group(group);
        }
    }

    pub fn dispatch(
        &mut self,
        x: u32,
        y: u32,
        z: u32,
    ) {
        self.inner.dispatch_workgroups(x, y, z);
    }

    pub fn dispatch_indirect(
        &mut self,
        buffer: &wgpu::Buffer,
        offset: u64,
    ) {
        self.inner.dispatch_workgroups_indirect(buffer, offset);
    }
}