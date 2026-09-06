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

    /// 绑定裸 uniform 缓冲（已持有 `Arc<wgpu::Buffer>` 时直用；语义同 [`Self::uniform`]）
    ///
    /// `size` 作为 min_binding_size（字节）。默认可见性 VERTEX | FRAGMENT。
    pub fn uniform_raw(mut self, binding: u32, buffer: Arc<wgpu::Buffer>, size: u64) -> Self {
        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            resource: BindResource::Uniform {
                buffer,
                size: Some(size),
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

    /// 绑定纹理视图数组（bindless 基础）
    ///
    /// layout 的 count = views.len()；WGSL 侧用 `binding_array<texture_2d<f32>>` 承接。
    /// 需要 `TEXTURE_BINDING_ARRAY` 与 `SAMPLED_..._NON_UNIFORM_INDEXING`
    /// （`features::recommended()` 已含）+ 上限愿望 `features::recommended_limits()`
    /// （`max_binding_array_elements_per_shader_stage` 默认为 0，不开 limit 用不了）。
    pub fn texture_view_array(mut self, binding: u32, views: Vec<Arc<wgpu::TextureView>>) -> Self {
        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::FRAGMENT,
            resource: BindResource::TextureArray { views },
        });
        self
    }

    /// 便捷版数组绑定：取每张纹理的默认视图
    pub fn texture_array(self, binding: u32, textures: &[Arc<Texture>]) -> Self {
        let views = textures.iter().map(|t| t.default_view()).collect();
        self.texture_view_array(binding, views)
    }

    /// 存储缓冲数组（compute 场景；读写限制同 [`Self::storage`]）
    pub fn storage_array(mut self, binding: u32, buffers: &[&StorageBuffer], read_only: bool) -> Self {
        self.items.push(BindItem {
            binding,
            visibility: ShaderStages::VERTEX | ShaderStages::COMPUTE,
            resource: BindResource::StorageArray {
                buffers: buffers.iter().map(|b| b.arc_buffer().clone()).collect(),
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

        // 数组绑定的引用/binding 中转（bind_entries 里存的是切片借用，
        // 故先收集完再构建 entries，避免借用跨迭代冲突）
        let mut view_refs_storage: Vec<Vec<&wgpu::TextureView>> = Vec::new();
        let mut buffer_bindings_storage: Vec<Vec<wgpu::BufferBinding>> = Vec::new();

        // 保证 binding 稳定
        self.items.sort_by_key(|i| i.binding);

        // ── 第一遍：构建 layout entries，收集数组中转 ──
        for item in &self.items {
            match &item.resource {

                // =========================
                // Uniform
                // =========================
                BindResource::Uniform { size, .. } => {
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
                }

                // =========================
                // Storage
                // =========================
                BindResource::Storage {
                    size,
                    read_only,
                    ..
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
                }

                // =========================
                // Texture
                // =========================
                BindResource::Texture { .. } => {
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
                }

                // =========================
                // Texture 数组（bindless）
                // =========================
                BindResource::TextureArray { views } => {
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
                        count: Some(std::num::NonZeroU32::new(views.len() as u32)
                            .expect("纹理视图数组不能为空")),
                    });

                    view_refs_storage.push(views.iter().map(|v| v.as_ref()).collect());
                }

                // =========================
                // Storage 数组
                // =========================
                BindResource::StorageArray { buffers, read_only } => {
                    layout_entries.push(wgpu::BindGroupLayoutEntry {
                        binding: item.binding,
                        visibility: item.visibility,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage {
                                read_only: *read_only,
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: Some(std::num::NonZeroU32::new(buffers.len() as u32)
                            .expect("存储缓冲数组不能为空")),
                    });

                    buffer_bindings_storage.push(
                        buffers
                            .iter()
                            .map(|b| wgpu::BufferBinding { buffer: b, offset: 0, size: None })
                            .collect(),
                    );
                }

                // =========================
                // Sampler
                // =========================
                BindResource::Sampler{..}=> {
                    layout_entries.push(wgpu::BindGroupLayoutEntry {
                        binding: item.binding,
                        visibility: item.visibility,
                        ty: wgpu::BindingType::Sampler(
                            wgpu::SamplerBindingType::Filtering,
                        ),
                        count: None,
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

        // ── 第二遍：构建 bind entries（数组引用中转此时已定型） ──
        let mut bind_entries = Vec::new();
        let mut view_iter = view_refs_storage.iter();
        let mut buf_iter = buffer_bindings_storage.iter();

        for item in &self.items {
            match &item.resource {
                BindResource::Uniform { buffer, .. } => {
                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: buffer.as_entire_binding(),
                    });
                }
                BindResource::Storage { buffer, .. } => {
                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: buffer.as_entire_binding(),
                    });
                }
                BindResource::Texture { view } => {
                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: wgpu::BindingResource::TextureView(view.as_ref()),
                    });
                }
                BindResource::TextureArray { .. } => {
                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: wgpu::BindingResource::TextureViewArray(
                            view_iter.next().unwrap().as_slice(),
                        ),
                    });
                }
                BindResource::StorageArray { .. } => {
                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: wgpu::BindingResource::BufferArray(
                            buf_iter.next().unwrap().as_slice(),
                        ),
                    });
                }
                BindResource::Sampler { sampler } => {
                    bind_entries.push(wgpu::BindGroupEntry {
                        binding: item.binding,
                        resource: wgpu::BindingResource::Sampler(sampler.as_ref()),
                    });
                }
            }
        }

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
