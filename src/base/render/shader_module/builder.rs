use std::sync::Arc;
use wgpu::{Device, ShaderModuleDescriptor, ShaderSource};

use crate::base::{resources::shader::Shader, render::shader_module::shader_module::ShaderModule};


//该builder是用来过度类型使用的，因为外部有个以文件形式的Shader，我不想将pipeline污染，所以使用了一个针对shader的builder隔绝
pub struct  ShaderModuleBuilder{
    device: Arc<Device>,
    source:String
}

impl ShaderModuleBuilder {

    pub(crate) fn new(
        device: &Arc<Device>,
        shader:Shader,
    ) -> Self {

        Self {
            device:device.clone(),
            source:shader.source,
        }
    }

    pub fn build(self, label: Option<&str>) -> Arc<ShaderModule>{
        let desc = 
            ShaderModuleDescriptor {
                label:label,
                source: ShaderSource::Wgsl(self.source.into()),
        };
        Arc::new(
            ShaderModule::new(
                self.device.create_shader_module(desc)
            )
        )
    }

}