use std::collections::HashMap;

use wgpu::{BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, Color, CommandEncoder, Device, LoadOp, Operations, PipelineLayout, PipelineLayoutDescriptor, RenderPassColorAttachment, RenderPassDescriptor, RenderPipeline, Sampler, SamplerBorderColor, ShaderModuleDescriptor, ShaderSource, StoreOp, TextureAspect, TextureFormat};
use wgpu::ShaderModule as WgpuShaderModule;

use wgpu::SamplerDescriptor;

use crate::base::render::texture::{Texture, ViewKey};


//这是一个标准简易版的mipmap生成，用于提供wgpu生成mipmap的模块，但是功能太过于基础，所以就只能作为拓展提供
//如果mimpmap达到一定成熟会作为标准功能放入context里!
pub struct Mipmap {
    pipelines: HashMap<TextureFormat, RenderPipeline>,
    sampler: Sampler,
    layout: BindGroupLayout,
    pipeline_layout: PipelineLayout,
    shader: WgpuShaderModule,
}


impl Mipmap {

    pub fn new(
        device: &Device,
        anisotropy: u16,
    ) -> Self {
        let source = "
            struct VSOut {
                @builtin(position) pos: vec4<f32>,
                @location(0) uv: vec2<f32>,
            };

            @vertex
            fn vs_main(
                @builtin(vertex_index) vertex_index: u32
            ) -> VSOut {

                var pos = array<vec2<f32>, 3>(
                    vec2<f32>(-1.0, -1.0),
                    vec2<f32>( 3.0, -1.0),
                    vec2<f32>(-1.0,  3.0),
                );

                var uv = array<vec2<f32>, 3>(
                    vec2<f32>(0.0, 0.0),
                    vec2<f32>(2.0, 0.0),
                    vec2<f32>(0.0, 2.0),
                );

                var out: VSOut;
                out.pos = vec4<f32>(pos[vertex_index], 0.0, 1.0);
                out.uv = uv[vertex_index];

                return out;
            }



            @group(0) @binding(0)
            var src_tex: texture_2d<f32>;

            @group(0) @binding(1)
            var src_sampler: sampler;

            @fragment
            fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
                return textureSample(
                    src_tex,
                    src_sampler,
                    in.uv
                );
            }";
        let desc = ShaderModuleDescriptor {
            label:Some("mipmap_shader"),
            source: ShaderSource::Wgsl(std::borrow::Cow::Borrowed(source)),
        };
        let shader = device.create_shader_module(desc);



        let layout = device.create_bind_group_layout(
            &BindGroupLayoutDescriptor {
                    label: Some("mipmap_bgl"),
                    entries:&[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility:
                            wgpu::ShaderStages::FRAGMENT,

                        ty:
                            wgpu::BindingType::Texture {
                                sample_type:
                                    wgpu::TextureSampleType::Float {
                                        filterable: true,
                                    },

                                view_dimension:
                                    wgpu::TextureViewDimension::D2,

                                multisampled: false,
                            },

                        count: None,
                    },

                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility:
                            wgpu::ShaderStages::FRAGMENT,

                        ty:
                            wgpu::BindingType::Sampler(
                                wgpu::SamplerBindingType::Filtering
                            ),

                        count: None,
                    },
                    ],
                }
            );
                

        let pipeline_layout =
            device.create_pipeline_layout(
            &PipelineLayoutDescriptor { 
                label: Some("mipmap_pipeline_layout"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
                }
            );

        let sampler = {

            let config = SamplerDescriptor::default();

            device.create_sampler(&SamplerDescriptor {
                // 调试标签，用于GPU调试器识别对象（无运行时影响）
                label: Some("mipmap_sampler"),

                // ========== 纹理坐标包裹模式 ==========
                // U 轴（x轴）纹理坐标超出 [0,1] 范围时如何处理
                address_mode_u: config.address_mode_u,
                // V 轴（y轴）纹理坐标超出 [0,1] 范围时如何处理
                address_mode_v: config.address_mode_v,
                // W 轴（z轴）3D纹理用，2D纹理无所谓，默认Repeat即可
                address_mode_w: config.address_mode_w,

                // ========== 纹理过滤模式 ==========
                // 纹理缩小（物体离相机远，纹理看起来变小）时的过滤方式
                min_filter: config.min_filter,
                // 纹理放大（物体离相机近，纹理看起来变大）时的过滤方式
                mag_filter: config.mag_filter,
                // Mipmap 之间切换时的过滤方式（Linear = 三线性过滤，引擎默认）
                mipmap_filter: config.mipmap_filter,
                

                //保证安全就够了，所以不需要修改
                // ========== LOD 层级限制 ==========
                // 允许使用的最小Mip层级（默认0，不限制）
                lod_min_clamp: 0.0,
                // 允许使用的最大Mip层级（默认100，不限制）
                lod_max_clamp: 100.0,

                // ========== 高级功能 ==========
                // 深度比较模式（阴影贴图专用，普通贴图填None）
                compare: config.compare,
                // 各向异性过滤（1=关闭，适合地面/墙面，默认关闭省性能）,context设置
                anisotropy_clamp: anisotropy,
                // 边界颜色（只有使用ClampToBorder才需要，默认不用）,使用白色，更容易醒目。黑色容易以为没有打光问题被跳过。
                border_color: Some(SamplerBorderColor::OpaqueWhite),
                }
            )
        };
    

        let pipelines =
            HashMap::new();

        Self {
            pipelines,
            sampler,
            layout,
            pipeline_layout,
            shader,
        }
    }

    pub fn generate(
        &self,
        device: &Device,
        encoder: &mut CommandEncoder,
        texture: &Texture){

        let format = texture.format();
        let msg = format!("The mipmap rendering pipeline for texture format {format:?} does not exist. Please register it in advance");
        
        let pipeline = self
            .pipelines
            .get(&format)
            .expect(&msg);

        let layer_count =
            texture
                .size()
                .depth_or_array_layers;

        for mip in 1..texture.mip_count() {

            let width =
                (texture.size().width >> mip)
                    .max(1);

            let height =
                (texture.size().height >> mip)
                    .max(1);

            for layer in 0..layer_count {

                let src_view = texture.get_view(&ViewKey {
                    base_mip: mip - 1,
                    mip_count: 1,
                    base_layer: layer,
                    layer_count: Some(1),

                    dimension: texture.desc().view_dimension(),
                    aspect: TextureAspect::All,
                });

                let dst_view = texture.get_view(&ViewKey {
                    base_mip: mip,
                    mip_count: 1,
                    base_layer: layer,
                    layer_count: Some(1),

                    dimension: texture.desc().view_dimension(),
                    aspect: TextureAspect::All,
                });

                let bind_group =
                        device.create_bind_group(&BindGroupDescriptor {
                            label: Some("mipmap_bg"),
                            //借用只在这一行执行，执行完借用消失 → 冻结立即解除，不影响后续任何操作。
                            layout: &self.layout,
                            entries:&[
                            BindGroupEntry {
                                binding: 0,
                                resource:
                                    BindingResource::TextureView(
                                        &src_view,
                                    ),
                            },
                            BindGroupEntry {
                                binding: 1,
                                resource:
                                    BindingResource::Sampler(
                                        &self.sampler,
                                    ),
                            },
                        ]});


                let mut pass =
                    encoder.begin_render_pass(
                        &RenderPassDescriptor {
                            label: Some("mipmap"),
                            color_attachments: &[
                                Some(
                                    RenderPassColorAttachment {
                                        view: &dst_view,
                                        resolve_target: None,
                                        ops: Operations {
                                            load: LoadOp::Clear(
                                                Color::BLACK
                                            ),
                                            store: StoreOp::Store,
                                        },
                                        depth_slice: None,
                                    }
                                )
                            ],
                            depth_stencil_attachment: None,
                            ..Default::default()
                        }
                    );

                pass.set_pipeline(
                    &pipeline
                );

                pass.set_bind_group(
                    0,
                    &bind_group,
                    &[],
                );

                pass.set_viewport(
                    0.0,
                    0.0,
                    width as f32,
                    height as f32,
                    0.0,
                    1.0,
                );

                pass.draw(
                    0..3,
                    0..1,
                );
            }
        }
    }
}