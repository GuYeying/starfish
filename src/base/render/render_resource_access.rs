use std::sync::Arc;
use wgpu::{Sampler, SamplerBorderColor, TextureFormat};
use wgpu::SamplerDescriptor as WgpuSamplerDescriptor;
use crate::base::error::Error;
use crate::base::render::bind_group::builder::BindGroupBuilder;
use crate::base::render::bind_group::storage_buffer::StorageBuffer;
use crate::base::render::bind_group::struct_layout::StructLayout;
use crate::base::render::bind_group::uniform_buffer::UniformBuffer;
use crate::base::render::command_encoder::CommandEncoder;
use crate::base::render::mesh::builder::MeshBuilder;
use crate::base::render::mesh::mesh::Mesh;
use crate::base::render::pipeline::{ComputePipelineBuilder, RenderPipelineBuilder};
use crate::base::render::sampler_desc::SamplerDescriptor;
use crate::base::render::shader_module::builder::ShaderModuleBuilder;
use crate::base::render::shader_module::shader_module::ShaderModule;
use crate::base::resources::image::{Image, ImageData};
use crate::base::resources::shader::Shader;
use super::texture::{Texture,TextureDescriptor};



pub struct RenderResourceAccess{
    device: Arc<wgpu::Device>,                // GPU逻辑设备（核心）
    queue: Arc<wgpu::Queue>,                  // GPU命令队列
    default_color_format:TextureFormat,
    default_depth_format:TextureFormat,
}


impl RenderResourceAccess {
    pub(crate) fn new(
        device: Arc<wgpu::Device>,                // GPU逻辑设备（核心）
        queue:  Arc<wgpu::Queue>,                  // GPU命令队列 
        default_color_format:TextureFormat,
        default_depth_format:TextureFormat,
    )->Self{
        device.on_uncaptured_error(
            Arc::new(|err| {
                eprintln!("\n================ WGPU 底层验证错误 ================");
                eprintln!("错误类型: {:?}", err);
                eprintln!("完整详情:\n{err:#?}");
                eprintln!("===================================================\n");
            })
        );
        Self{
            device: device,
            queue:  queue,
            default_color_format,
            default_depth_format,
        }
    }





    pub fn update_uniform_buffer(&self,buffer:&mut UniformBuffer){
        buffer.upload(&self.queue);
    }

    pub fn update_storage_buffer(&self,buffer:&mut StorageBuffer){
        buffer.upload(&self.queue);
    }


    pub fn write_vertex_buffer(&self,mesh:&Mesh,data:&[u8]){
        mesh.write_vertex_data(&self.queue,data)
    }

    pub fn write_index_buffer(&self,mesh:&Mesh,data:&[u8])->Result<(),Error>{
         mesh.write_index_data(&self.queue,data)
    }








    pub fn create_uniform_buffer(&self,layout: &Arc<StructLayout>)->UniformBuffer{
        UniformBuffer::new(&self.device, layout)
    }

    pub fn create_storage_buffer(&self,layout: &Arc<StructLayout>, count: usize, label: Option<&str>)->StorageBuffer{
        StorageBuffer::new(&self.device, layout, count, label)
    }



    pub fn create_sampler(&self,label: &str,config:&SamplerDescriptor)->Sampler{
        let border_color = if config.use_white_border() {
            Some(SamplerBorderColor::OpaqueWhite)
        } else {
            None
        };

        let desc = WgpuSamplerDescriptor {
            label: Some(label),
            address_mode_u: config.address_u(),
            address_mode_v: config.address_v(),
            address_mode_w: config.address_w(),

            min_filter: config.min_filter(),
            mag_filter: config.mag_filter(),
            mipmap_filter: config.mipmap_filter(),

            lod_min_clamp: 0.0,
            lod_max_clamp: 100.0,

            compare: config.compare(),
            anisotropy_clamp: config.anisotropy(),
            border_color,
        };

        self.device.create_sampler(&desc)
    }

    /**
    该函数等价于create_texture_from_rgba8_2d
    */
    pub fn create_texture(&self,label: &str,image_data: &ImageData,desc: TextureDescriptor)->Texture{
        let  image = &Image::new(image_data);
        Texture::from_rgba8_2d(&self.device, &self.queue, label, image, desc)
    }

    pub fn create_texture_from_rgba8_1d(&self,label: &str,image_data: &ImageData,desc: TextureDescriptor)->Texture{
        let  image = &Image::new(image_data);
        Texture::from_rgba8_1d(&self.device,  &self.queue, label, image, desc)
    }

    pub fn create_texture_from_rgba8_2d(
        &self,
        label: &str,
        image_data: &ImageData,
        desc: TextureDescriptor,
    ) -> Texture
    {   

        let  image = &Image::new(image_data);

        let texture =
            Texture::from_rgba8_2d(
                &self.device,
                &self.queue,
                label,
                image,
                desc,
            );
        texture
    }

    pub fn create_texture_from_rgba8_3d(
        &self,
        label: &str,
        images: &[&ImageData],
        desc: TextureDescriptor
    ) -> Texture {
        // 先收集所有权，存到Vec，生命周期贯穿整个函数
        let owned_images: Vec<Image> = images.iter()
            .map(|data| Image::new(data))
            .collect();
        // 再传切片引用
        Texture::from_rgba8_3d(
            &self.device,
            &self.queue,
            label,
            &owned_images,
            desc
        )
    }

    pub fn create_texture_from_rgba8_cube(
        &self,
        label: &str,
        faces: [&ImageData; 6],
        desc: TextureDescriptor
    ) -> Texture {
        // 生成栈上固定数组，持有所有权
        let face_images = faces.map(|data| Image::new(data));
        Texture::from_rgba8_cube(&self.device, &self.queue, label, &face_images, desc)
    }

    pub fn create_texture_from_rgba8_array(
        &self,
        label: &str,
        images: &[&ImageData],
        desc: TextureDescriptor
    ) -> Texture {
        let owned_images: Vec<Image> = images.iter()
            .map(|data| Image::new(data))
            .collect();
        Texture::from_rgba8_array(
            &self.device,
            &self.queue,
            label,
            &owned_images,
            desc
        )
    }


    pub fn create_command_encoder(&self)->CommandEncoder{
        CommandEncoder::new(self.device.create_command_encoder(&Default::default()))
    }
    

    // 产出MeshBuilder，内部自带上下文引用
    pub fn shader_module_builder(&self,shader:Shader)->ShaderModuleBuilder{
        ShaderModuleBuilder::new(&self.device, shader)
    }

    pub fn mesh_builder(&self,layout: Vec<wgpu::VertexFormat>,vertices: Vec<u8>,) -> MeshBuilder {
        MeshBuilder::new(&self.device,layout,vertices)
    }

    pub fn bind_group_builder(&self) -> BindGroupBuilder {
        BindGroupBuilder::new(&self.device)
    }

    pub fn compute_pipeline_builder(&self,shader: &Arc<ShaderModule>)->ComputePipelineBuilder{
        ComputePipelineBuilder::new(&self.device,shader)
    }


    pub fn render_pipeline_builder_2d(
        &self, 
        shader: &Arc<ShaderModule>,
    ) -> RenderPipelineBuilder {
        RenderPipelineBuilder::new_2d(
            &self.device,
            self.default_color_format,
            self.default_depth_format,
            &shader,
        )
    }

    pub fn render_pipeline_builder_3d(&self,  shader: &Arc<ShaderModule>) -> RenderPipelineBuilder {
        RenderPipelineBuilder::new_3d(
            &self.device,
            self.default_color_format,
            self.default_depth_format,
            &shader
        )
    }
}