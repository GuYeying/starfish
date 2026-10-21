

pub struct ShaderModule{
    pub(crate) shader:wgpu::ShaderModule

}



impl ShaderModule{
    pub fn new(shader:wgpu::ShaderModule)->Self{
        Self{
            shader:shader
        }
    }

    pub fn as_inner_ref(&self)->&wgpu::ShaderModule{
        &self.shader
    }
}