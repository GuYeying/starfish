use std::{sync::Arc};


use wgpu::{BindGroup as WgpuBindGroup, BindGroupLayout as WgpuBindGroupLayout};

use crate::base::render::bind_group::{bindings::{BindItem}};

//Cache
//持有 Weak<BindGroupLayout>
pub struct BindGroup {
    pub items: Vec<BindItem>,
    bind_group: WgpuBindGroup,
    layout: Arc<WgpuBindGroupLayout>,
    
}


impl BindGroup {
    pub(crate) fn new(
        items: Vec<BindItem>,
        bind_group: WgpuBindGroup,
        layout: Arc<WgpuBindGroupLayout>,
    ) -> Self {
        Self {
            items,
            bind_group,
            layout,
        }
    }

    pub fn bind_group(&self) -> &WgpuBindGroup {
        &self.bind_group
    }

    pub fn layout(&self) -> &WgpuBindGroupLayout {
        &self.layout
    }

    pub fn items(&self) -> &[BindItem] {
        &self.items
    }

}



// | 类型      | 更新方式               |
// | ------- | ------------------ |
// | uniform | queue.write_buffer |
// | storage | queue.write_buffer |
// | texture | immutable          |
// | sampler | immutable          |
