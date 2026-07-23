use std::sync::Arc;

use wgpu::{Device, ShaderStages};

use crate::base::render::{
    bind_group::{bindings::{BindItem, BindResource}, storage_buffer::StorageBuffer, uniform_buffer::UniformBuffer}, texture::{Texture, ViewKey},
};

use super::bind_group::BindGroup;
use wgpu::Sampler;

pub struct BindGroupBuilder {
    device:Arc<Device>,
    items: Vec<BindItem>,
}

impl BindGroupBuilder {
    pub(crate) fn new(device:&Arc<Device>) -> Self {
        Self {
            device:device.clone(), 
            items: vec![] 
        }
    }

    pub fn texture(
        mut self,
        binding: u32,
        texture: Arc<Texture>,
    ) -> Self {
        let view = texture.default_view();

        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::FRAGMENT,
            resource: BindResource::Texture {
                view,
            },
        });

        self
    }

    pub fn texture_with_view(
        mut self,
        binding: u32,
        texture: Arc<Texture>,
        view: ViewKey,
    ) -> Self {
        let view = texture.get_view(&view);

        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::FRAGMENT,
            resource: BindResource::Texture {
                view,
            },
        });

        self
    }

    pub fn sampler(
        mut self,
        binding: u32,
        sampler: Arc<Sampler>,
    ) -> Self {
        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::FRAGMENT,
            resource: BindResource::Sampler{ sampler },
        });
        self
    }

    pub fn uniform(mut self, binding: u32, buffer: &UniformBuffer) -> Self {
        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::VERTEX,
            resource: BindResource::Uniform {
                buffer: buffer.arc_buffer().clone(),
                size: None,
            },
        });
        self
    }

    pub fn storage(mut self,binding: u32,buffer: &StorageBuffer,read_only: bool,) -> Self {
        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::VERTEX | ShaderStages::COMPUTE,
            resource: BindResource::Storage {
                buffer: buffer.arc_buffer().clone(),
                size: None,
                read_only,
            },
        });
        self
    }

    pub fn build(
        mut self,
        label: Option<&str>,
    ) -> BindGroup {
        let mut layout_entries = Vec::new();
        let mut bind_entries = Vec::new();

        // 保证 binding 稳定
        self.items.sort_by_key(|i| i.binding);

        for item in &self.items {
            match &item.resource {

                // =========================
                // Uniform
                // =========================
                BindResource::Uniform { buffer, size } => {
                    layout_entries.push(wgpu::BindGroupLayoutEntry {
                        binding: item.binding,
                        visibility: item.visibility,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: size
                                .and_then(std::num::NonZeroU64::new),
                        },
                        count: None,
                    });

                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: buffer.as_entire_binding(),
                    });
                }

                // =========================
                // Storage
                // =========================
                BindResource::Storage {
                    buffer,
                    size,
                    read_only,
                } => {
                    layout_entries.push(wgpu::BindGroupLayoutEntry {
                        binding: item.binding,
                        visibility: item.visibility,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: *read_only,
                            },
                            has_dynamic_offset: false,
                            min_binding_size: size
                                .and_then(std::num::NonZeroU64::new),
                        },
                        count: None,
                    });

                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: buffer.as_entire_binding(),
                    });
                }

                // =========================
                // Texture（已修复版本）
                // =========================
                BindResource::Texture { view } => {
                    layout_entries.push(wgpu::BindGroupLayoutEntry {
                        binding: item.binding,
                        visibility: item.visibility,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float {
                                filterable: true,
                            },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    });

                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: wgpu::BindingResource::TextureView(view.as_ref()),
                    });
                }

                // =========================
                // Sampler
                // =========================
                BindResource::Sampler{sampler}=> {
                    layout_entries.push(wgpu::BindGroupLayoutEntry {
                        binding: item.binding,
                        visibility: item.visibility,
                        ty: wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
                    });

                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: wgpu::BindingResource::Sampler(
                            sampler.as_ref(),
                        ),
                    });
                }
            }
        }

        // =========================
        // create layout
        // =========================
        let layout = Arc::new(
            self.device.create_bind_group_layout(
                &wgpu::BindGroupLayoutDescriptor {
                    label,
                    entries: &layout_entries,
                },
            ),
        );

        // =========================
        // create bind group
        // =========================
        let bind_group = self.device.create_bind_group(
            &wgpu::BindGroupDescriptor {
                label,
                layout: &layout,
                entries: &bind_entries,
            },
        );

        // =========================
        // return
        // =========================
        BindGroup::new(self.items, bind_group, layout)
    }


}