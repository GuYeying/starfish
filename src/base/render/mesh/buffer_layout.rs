use wgpu::VertexStepMode;


// WGPU 渲染时必须告诉 GPU：
// 每个顶点占用多少字节（stride）
// 是逐顶点数据，还是逐实例数据（Vertex / Instance）
// 每个属性在顶点内部的字节偏移、着色器 location、数据格式

// 核心逻辑：
// 两个入口构造：
// BufferLayout::vertex(attrs)：构建普通顶点属性布局
// BufferLayout::instance(attrs)：构建实例化渲染的实例属性布局
// from_attributes 自动计算：
// 遍历属性，依次累加字节offset，算出每个属性在顶点内的内存偏移
// 自动分配 shader_location（按数组顺序从 0 自增）
// 自动计算单顶点总字节 stride
// .layout() 方法：对外输出 WGPU 原生VertexBufferLayout<'_>，直接传给渲染管线。


#[derive(Debug,Clone)]
pub(crate)  struct BufferLayout {
    stride:u32,
    step_mode: VertexStepMode,
    attributes: Vec<wgpu::VertexAttribute>,
}

impl BufferLayout{
    
    pub(crate) fn layout(&self) -> wgpu::VertexBufferLayout<'_> {
        wgpu::VertexBufferLayout {
            array_stride: self.stride as u64,
            step_mode: self.step_mode,
            attributes: &self.attributes,
        }
    }

    pub(crate) fn vertex(attrs: &[wgpu::VertexFormat]) -> Self {
        Self::from_attributes(attrs, VertexStepMode::Vertex)
    }

    pub(crate) fn instance(attrs: &[wgpu::VertexFormat]) -> Self {
        Self::from_attributes(attrs, VertexStepMode::Instance)
    }


    #[inline]
    fn from_attributes(
        attrs: &[wgpu::VertexFormat],
        step_mode: VertexStepMode,
    ) -> Self {
        let mut offset = 0;
        let mut gpu_attributes = Vec::with_capacity(attrs.len());

        for (location, format) in attrs.iter().enumerate() {
            //let format = attr.format;

            gpu_attributes.push(wgpu::VertexAttribute {
                format: *format,
                offset,
                shader_location: location as u32,
            });

            offset += format.size() as u64;
        }

        Self {
            stride: offset as u32,
            step_mode,
            attributes: gpu_attributes,
        }
    }


    pub(crate) fn array_stride(&self) -> u64 {
        self.stride as u64
    }

}