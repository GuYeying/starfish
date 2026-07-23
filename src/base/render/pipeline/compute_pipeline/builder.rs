use std::sync::Arc;
use wgpu::{Device, PipelineLayoutDescriptor};
use crate::base::render::{bind_group::bind_group::BindGroup, pipeline::{ComputePipeline, PipelineCompilationConfig}, shader_module::shader_module::ShaderModule};




pub struct ComputePipelineBuilder {
    device: Arc<Device>,
    shader: Arc<ShaderModule>,
    /// Compute Shader入口
    entry: String,
    /// Pipeline Cache
    cache: Option<Arc<wgpu::PipelineCache>>,
    /// Shader 编译选项
    compilation_options: PipelineCompilationConfig,

}

impl ComputePipelineBuilder {
    /// 默认 cs_main
    pub fn new(
        device:&Arc<Device>,
        shader:&Arc<ShaderModule>,
    )->Self {
        Self {
            device:device.clone(),
            shader:shader.clone(),
            entry:"cs_main".into(),
            compilation_options:Default::default(),
            cache:None,
            
        }
    }

    /// 指定 Compute Shader 入口
    pub fn entry_point(
        mut self,
        entry:String
    )->Self {

        self.entry = entry.clone();

        self
    }

    pub fn compilation_options(
        mut self,
        options: PipelineCompilationConfig,
    )->Self {

        self.compilation_options = options;

        self
    }

    pub fn build(
        self,
        bind_groups:&[&BindGroup],
        label: Option<&str>,
    ) -> Arc<ComputePipeline> {


        let constants:Vec<(&str,f64)> = self.compilation_options.constants
            .iter()
            .map(|(k,v)| {(k.as_str(), *v)})
            .collect();
        let compilation_options = wgpu::PipelineCompilationOptions {
            constants:&constants,
            zero_initialize_workgroup_memory:
                self.compilation_options
                    .zero_initialize_workgroup_memory,
        };

        let bind_group_layouts:Vec<Option<&wgpu::BindGroupLayout>> = bind_groups
            .iter()
            .map(|g|Some(g.layout()))
            .collect();

        let layout = Arc::new(
        self.device.create_pipeline_layout(
            &PipelineLayoutDescriptor { 
                label: label,
                bind_group_layouts: bind_group_layouts.as_slice(),
                immediate_size: 0
                }
            )
        );

        let desc = wgpu::ComputePipelineDescriptor {
            label,
            layout:Some(&layout),
            module: &self.shader.shader,
            entry_point:Some(&self.entry),
            compilation_options:compilation_options,
            cache: self.cache.as_deref(),

        };

        let pipeline = self.device.create_compute_pipeline(&desc);

        Arc::new(
            ComputePipeline::new(
                pipeline,
                layout,
            )
        )
    }
}