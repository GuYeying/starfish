use std::{num::NonZeroU32, sync::Arc};
use crate::base::render::{bind_group::bind_group::BindGroup, mesh::mesh::Mesh, pipeline::{PipelineCompilationConfig, RenderPipeline}, shader_module::shader_module::ShaderModule};
use wgpu::{ColorTargetState, DepthBiasState, StencilState, TextureFormat};
use wgpu::{Device, PipelineLayoutDescriptor};
pub use wgpu::{BlendState,PrimitiveTopology,CompareFunction};


#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Default)]
pub enum BlendMode {
    None,
    Opaque,
    #[default]
    Alpha,
    Additive,
    Multiply,
    Custom(wgpu::BlendState)
}


#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Default)]
pub enum CullMode {
    None,
    #[default]
    Back,
    Front,
}

// Standard Z
// CompareFunction::Less
// Reverse Z
// CompareFunction::GreaterEqual
// 你的：
// depth_compare
// 虽然能实现，但是开发者体验不好。
///下方为解决方案
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Default)]
pub enum DepthMode {
    /// 不使用深度
    Disabled,
    /// 标准深度
    ///
    /// near=0
    /// far=1
    #[default]
    Standard,
    /// 反向深度
    ///
    /// near=1
    /// far=0
    Reverse,
}


pub struct PipelineAdvancedState {
    pub stencil: Option<StencilState>,
    pub depth_bias: Option<DepthBiasState>,
    /// 天空盒、无限远投影使用
    pub unclipped_depth: bool,
    /// 体素化、GPU碰撞等
    pub conservative_raster: bool,

}

impl Default for PipelineAdvancedState {
    fn default()->Self{
        Self{
            stencil:None,
            depth_bias:None,
            unclipped_depth:false,
            conservative_raster:false,
        }
    }
}


pub struct RenderPipelineBuilder {

    device: Arc<Device>,

    default_color_format:TextureFormat,
    default_depth_format:TextureFormat,
    
    color_targets:Vec<Option<ColorTargetState>>,

    shader: Arc<ShaderModule>,


    // ==========================
    // Raster State
    // ==========================
    /// 是否开启深度测试
    // pub depth_test: bool,
    // /// 是否写入深度
    // pub depth_write: bool,
    // /// 深度比较方式
    // pub depth_compare: CompareFunction,
    //深度模式
    pub depth_mode: DepthMode,
    //深度写入
    pub depth_write: bool,

    /// 面剔除
    pub cull: CullMode,
    /// 三角形拓扑
    pub topology: PrimitiveTopology,
    /// 线框模式
    pub wireframe: bool,
    // ==========================
    // Blend State
    // ==========================
    pub blend: BlendMode,
    // ==========================
    // Advanced Raster
    // ==========================
    pub advanced: PipelineAdvancedState,
    // ==========================
    // MSAA
    // ==========================
    pub sample_count: u32,
    pub alpha_to_coverage: bool,
    // ==========================
    // Shader Entry
    // ==========================
    pub vertex_entry: String,
    pub fragment_entry: String,
    // ==========================
    // Pipeline Cache
    // ==========================
    pub cache: Option<Arc<wgpu::PipelineCache>>,

    pub multiview_mask : Option<u32>,

    /// Shader 编译选项
    compilation_options: PipelineCompilationConfig,

}





impl RenderPipelineBuilder {

    /// 3D 通用默认管线（模型/地形/3D物体）
    pub(crate) fn new_3d(
        device:&Arc<Device>,
        default_color_format:TextureFormat,
        default_depth_format:TextureFormat,
        shader:&Arc<ShaderModule>
    )->Self{


        Self{
            device:device.clone(),
            default_color_format,
            default_depth_format,
            color_targets:Vec::new(),
            shader:shader.clone(),
            // depth
            depth_mode:DepthMode::Standard,
            depth_write:true,
            // raster
            cull:CullMode::Back,
            topology:PrimitiveTopology::TriangleList,
            wireframe:false,
            // blend
            blend:BlendMode::Opaque,
            // advanced
            advanced:PipelineAdvancedState::default(),
            // msaa
            sample_count:1,
            alpha_to_coverage:false,
            // shader
            vertex_entry:"vs_main".into(),
            fragment_entry:"fs_main".into(),
            // cache
            cache:None,
            multiview_mask:None,
            compilation_options:Default::default(),
        }

    }

    /// 2D 通用默认管线（UI、Sprite、2D平面贴图）
    pub(crate) fn new_2d(
        device:&Arc<Device>,
        default_color_format:TextureFormat,
        default_depth_format:TextureFormat,
        shader:&Arc<ShaderModule>
    )->Self{

        Self{
            device:device.clone(),
            default_color_format,
            default_depth_format,
            color_targets:Vec::new(),
            shader:shader.clone(),
            depth_mode:DepthMode::Disabled,
            depth_write:false,
            cull:CullMode::None,
            topology:PrimitiveTopology::TriangleList,
            wireframe:false,
            blend:BlendMode::Alpha,
            advanced:PipelineAdvancedState::default(),
            sample_count:1,
            alpha_to_coverage:false,
            vertex_entry:"vs_main".into(),
            fragment_entry:"fs_main".into(),
            cache:None,
            multiview_mask:None,
            compilation_options:Default::default(),
        }
    }
    
    pub fn depth_mode(
        mut self,
        mode: DepthMode,
    ) -> Self {
        self.depth_mode = mode;
        self
    }
    pub fn depth_write(mut self,enable: bool,) -> Self {self.depth_write = enable;self}
    pub fn blend(mut self,blend: BlendMode,) -> Self {self.blend = blend;self}
    pub fn cull(mut self,cull: CullMode,) -> Self {self.cull = cull;self}
    pub fn wireframe(mut self,wireframe: bool,) -> Self {self.wireframe = wireframe;self}
    pub fn topology(mut self,topology: wgpu::PrimitiveTopology,) -> Self {self.topology = topology;self}
    pub fn sample_count(mut self,count: u32,) -> Self {self.sample_count = count.max(1);self}
    pub fn stencil(
        mut self,
        stencil: wgpu::StencilState,
    ) -> Self {
        self.advanced.stencil = Some(stencil);
        self
    }
    pub fn depth_bias(
        mut self,
        bias: wgpu::DepthBiasState,
    ) -> Self {
        self.advanced.depth_bias = Some(bias);
        self
    }
    pub fn unclipped_depth(
        mut self,
        enable: bool,
    ) -> Self {

        self.advanced.unclipped_depth = enable;

        self
    }
    pub fn conservative_raster(
        mut self,
        enable: bool,
    ) -> Self {

        self.advanced.conservative_raster = enable;

        self
    }
    pub fn alpha_to_coverage(
        mut self,
        enable: bool,
    ) -> Self {

        self.alpha_to_coverage = enable;

        self
    }
    pub fn vertex_entry(
        mut self,
        entry: String,
    ) -> Self {

        self.vertex_entry = entry.clone();

        self
    }
    pub fn fragment_entry(
        mut self,
        entry: String,
    ) -> Self {

        self.fragment_entry = entry.clone();

        self
    }
    pub fn cache(
        mut self,
        cache: Arc<wgpu::PipelineCache>,
    ) -> Self {

        self.cache = Some(cache);

        self
    }
    pub fn multiview(
        mut self,
        mask:u32
    )->Self
    {
        self.multiview_mask = Some(mask);
        self
    }
    pub fn compilation_options(
        mut self,
        options: PipelineCompilationConfig,
    )->Self {

        self.compilation_options = options;

        self
    }

    pub fn color_targets(&mut self,targets:&Vec<Option<ColorTargetState>>){
        self.color_targets = targets.clone()
    }
    
    pub fn build(
        self,
        //ctx: &RenderContext,
        bind_groups: &[&BindGroup],
        mesh: &Mesh,
        label: Option<&str>,
    ) -> Arc<RenderPipeline>{

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

        let bind_group_layouts: Vec<Option<&wgpu::BindGroupLayout>> = bind_groups
            .iter()
            .map(|g| Some(g.layout()))
            .collect();

        // 推导各种 State
        let primitive = wgpu::PrimitiveState {
            topology: self.topology,

            strip_index_format: None,

            front_face: wgpu::FrontFace::Ccw,

            cull_mode: match self.cull {
                CullMode::Front => Some(wgpu::Face::Front),
                CullMode::Back => Some(wgpu::Face::Back),
                CullMode::None => None,
            },

            polygon_mode: if self.wireframe {
                wgpu::PolygonMode::Line
            } else {
                wgpu::PolygonMode::Fill
            },

            unclipped_depth: self.advanced.unclipped_depth,

            conservative: self.advanced.conservative_raster,
        };

        let depth_compare = match self.depth_mode {
            DepthMode::Disabled => None,
            DepthMode::Standard => Some(wgpu::CompareFunction::Less),
            DepthMode::Reverse => Some(wgpu::CompareFunction::GreaterEqual),
        };
        let depth_stencil = depth_compare.map(|compare| {

            wgpu::DepthStencilState {
                format:self.default_depth_format,
                depth_write_enabled: Some(self.depth_write),
                depth_compare: Some(compare),
                stencil:
                    self.advanced
                        .stencil
                        .unwrap_or_default(),
                bias:
                    self.advanced
                        .depth_bias
                        .unwrap_or_default(),
            }

        });

        let blend = match self.blend {
            BlendMode::None => None,

            BlendMode::Opaque => Some(wgpu::BlendState::REPLACE),

            BlendMode::Alpha => Some(wgpu::BlendState::ALPHA_BLENDING),

            BlendMode::Additive => Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            }),

            BlendMode::Multiply => Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Dst,
                    dst_factor: wgpu::BlendFactor::Zero,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent::OVER,
            }),
             
            BlendMode::Custom(state) => Some(state)

        };

        let color_targets = if self.color_targets.is_empty() {
            vec![
                Some(wgpu::ColorTargetState {
                    format: self.default_color_format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            ]
        } else {
            self.color_targets
        };

        let multisample = wgpu::MultisampleState {
            count: self.sample_count,
            mask: !0,
            alpha_to_coverage_enabled: self.alpha_to_coverage,
        };

        let binding = mesh.layout();
        let vertex_layouts = vec![Some(binding.layout())];

        let layout = Arc::new(
        self.device.create_pipeline_layout(
            &PipelineLayoutDescriptor { 
                label,
                bind_group_layouts: &bind_group_layouts,
                immediate_size: 0//注意这个features这个是绝对不支持的！！！
                }
            )
        );

        let vertex_entry = self.vertex_entry.as_str();
        let fragment_entry = self.fragment_entry.as_str();

        let desc = wgpu::RenderPipelineDescriptor {
            label: label,
            layout:Some(&layout),

            vertex: wgpu::VertexState {
                module: &self.shader.shader,
                entry_point: Some(vertex_entry),//可以考虑加入可修改的行列里！！！！！先看是否支持多个入口情况
                buffers: vertex_layouts.as_slice(),
                //开发者不用复制多份几乎一样的 shader 文件，运行时通过 compilation_options.defines 传入宏，动态编译变体。对材质系统、材质变体、渲染管线复用是刚需
                compilation_options: compilation_options.clone(),//绝对保留，compilation_options：写一份通用 PBR 光照 shader，通过宏开关区分功能
            },

            fragment: Some(wgpu::FragmentState {
                module: &self.shader.shader,
                entry_point: Some(fragment_entry),
                targets: &color_targets,
                compilation_options: compilation_options,
            }),

            primitive:primitive,
            depth_stencil: depth_stencil,
            multisample: multisample,
            cache: self.cache.as_deref(),//缓存功能我觉得必须有
            multiview_mask: self.multiview_mask.and_then(NonZeroU32::new),
        };

        let pipeline = self.device.create_render_pipeline(&desc);
        
        println!("{:#?}", vertex_layouts);
        println!("{:#?}", desc);
        Arc::new(
            RenderPipeline::new(pipeline, layout)
        )

    }
}

