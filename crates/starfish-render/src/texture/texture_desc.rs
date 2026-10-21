use wgpu::{TextureAspect, TextureFormat, TextureUsages, TextureViewDimension};


/// 贴图语义：描述这张贴图是什么用途（Albedo/Normal/Depth/HDR）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureSemantic {
    Color,
    Normal,
    Data,
    Depth,
    DepthStencil,  // 新增：深度+模板复合纹理
    Hdr,
}

/// GPU资源用途：采样/渲染目标/阴影贴图/存储纹理
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureUsage {
    Sampled,
    RenderTarget,
    ShadowMap,
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipmapPolicy {
    Enabled,
    Disabled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureDim {
    D1,
    D2,
    D2Array,
    D3,
    Cube,
}

/// 新版纹理描述符：拆分语义+用途，支持手动覆盖GPU格式
/// 我认为最终 SamplerDescriptor 应该负责的内容
// ✔ GPU Format
// ✔ GPU Usage
// ✔ TextureDimension
// ✔ TextureViewDimension
// ✔ TextureSampleType
// ✔ BindingType
// ✔ SamplerBindingType
// ✔ 是否支持过滤
// ✔ 是否支持 Sampler
// ✔ 是否为 RenderTarget
// ✔ 是否为 StorageTexture
// ✔ 是否为 DepthTexture
// ✔ 是否需要 Mipmap
#[derive(Clone, Copy)]
pub struct TextureDescriptor {
    pub semantic: TextureSemantic,
    pub usage: TextureUsage,
    pub gpu_format_override: Option<TextureFormat>, // 手动覆盖格式，优先级最高
    pub dimension: TextureDim,
    pub mipmap: MipmapPolicy,
    pub multisample: Option<u32>,
    pub array_layers: u32,
}

impl TextureDescriptor {
    pub fn new(
        semantic: TextureSemantic,
        usage: TextureUsage,
        dimension: TextureDim,
        mimap: Option<MipmapPolicy>,
        array_layers: Option<u32>,
    ) -> Self {
        let desc = Self {
            semantic,
            usage,
            gpu_format_override: None,
            dimension,
            mipmap: mimap.unwrap_or(MipmapPolicy::Disabled),
            multisample: None,
            array_layers: array_layers.unwrap_or(1),
        };

        // 校验：只有RenderTarget/ShadowMap/Depth才允许MSAA
        let is_rt = matches!(usage, TextureUsage::RenderTarget | TextureUsage::ShadowMap);
        if !is_rt && desc.multisample.is_some() {
            eprintln!(
                "[WARN] 纹理语义 {:?} 非渲染目标，MSAA配置被忽略",
                desc.semantic
            );
        }
        if is_rt && desc.multisample.is_none() {
            eprintln!("[INFO] 渲染目标纹理未指定MSAA，默认关闭抗锯齿");
        }
        desc
    }

    /// 手动覆盖纹理格式
    pub fn with_format_override(mut self, fmt: TextureFormat) -> Self {
        self.gpu_format_override = Some(fmt);
        self
    }

    pub fn sample_count(&self) -> u32 {
        if matches!(self.usage, TextureUsage::RenderTarget | TextureUsage::ShadowMap) {
            self.multisample.unwrap_or(1)
        } else {
            1
        }
    }

    pub fn supports_mipmap(&self) -> bool {
        matches!(
            self.semantic,
            TextureSemantic::Color
                | TextureSemantic::Data
                | TextureSemantic::Normal
                | TextureSemantic::Hdr
        )
    }

    pub fn mip_level_count(&self, width: u32, height: u32) -> u32 {
        match self.mipmap {
            MipmapPolicy::Disabled => 1,
            MipmapPolicy::Enabled => (width.max(height) as f32).log2().floor() as u32 + 1,
        }
    }

    /// 优先使用手动覆盖格式，否则根据语义自动推导
    pub fn format(&self) -> TextureFormat {
        if let Some(fmt) = self.gpu_format_override {
            return fmt;
        }
        match self.semantic {
            TextureSemantic::Color => TextureFormat::Rgba8UnormSrgb,
            TextureSemantic::Data => TextureFormat::R8Unorm,
            TextureSemantic::Normal => TextureFormat::Rg8Unorm,
            TextureSemantic::Hdr => TextureFormat::Rgba16Float,
            TextureSemantic::Depth => TextureFormat::Depth24Plus,
            // 新增深度模板复合格式
            TextureSemantic::DepthStencil => TextureFormat::Depth24PlusStencil8,
        }
    }
    /// 自动生成wgpu TextureUsages
    pub fn usage_flags(&self) -> TextureUsages {
        let mut usage = match self.usage {
            TextureUsage::Sampled => TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            TextureUsage::RenderTarget => TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            TextureUsage::ShadowMap => TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            TextureUsage::Storage => TextureUsages::STORAGE_BINDING | TextureUsages::COPY_DST,
        };

        // 开启mipmap必须开启COPY_SRC + RENDER_ATTACHMENT用于生成mip链
        if matches!(self.mipmap, MipmapPolicy::Enabled) && self.supports_mipmap() {
            usage |= TextureUsages::COPY_SRC | TextureUsages::RENDER_ATTACHMENT;
        }
        usage
    }

    pub fn gpu_dimension(&self) -> wgpu::TextureDimension {
        match self.dimension {
            TextureDim::D1 => wgpu::TextureDimension::D1,
            TextureDim::D3 => wgpu::TextureDimension::D3,
            _ => wgpu::TextureDimension::D2,
        }
    }

    pub fn depth_or_array_layers(&self) -> u32 {
        match self.dimension {
            TextureDim::Cube => 6,
            TextureDim::D2Array => self.array_layers,
            TextureDim::D1 | TextureDim::D2 | TextureDim::D3 => 1,
        }
    }

    pub fn view_dimension(&self) -> TextureViewDimension {
        match self.dimension {
            TextureDim::D1 => TextureViewDimension::D1,
            TextureDim::D2 => TextureViewDimension::D2,
            TextureDim::D2Array => TextureViewDimension::D2Array,
            TextureDim::Cube => TextureViewDimension::Cube,
            TextureDim::D3 => TextureViewDimension::D3,
        }
    }

    // 只读取深度
    pub fn depth_aspect(&self) -> TextureAspect {
        match self.semantic {
            TextureSemantic::Depth | TextureSemantic::DepthStencil => TextureAspect::DepthOnly,
            _ => TextureAspect::All,
        }
    }

    // 只读取模板
    pub fn stencil_aspect(&self) -> TextureAspect {
        match self.semantic {
            TextureSemantic::DepthStencil => TextureAspect::StencilOnly,
            _ => TextureAspect::All,
        }
    }
    
    pub fn default_visibility(&self) -> wgpu::ShaderStages {
        wgpu::ShaderStages::VERTEX_FRAGMENT
    }

    pub fn supports_filtering(&self) -> bool {
        matches!(
            self.semantic,
            TextureSemantic::Color
                | TextureSemantic::Normal
                | TextureSemantic::Hdr
        )
    }

    pub fn sample_type(&self) -> wgpu::TextureSampleType {
        match self.semantic {
            TextureSemantic::Depth
            | TextureSemantic::DepthStencil => {
                wgpu::TextureSampleType::Depth
            }

            TextureSemantic::Color
            | TextureSemantic::Normal
            | TextureSemantic::Data
            | TextureSemantic::Hdr => {
                wgpu::TextureSampleType::Float {
                    filterable: true,
                }
            }
        }
    }

    pub fn binding_type(&self) -> wgpu::BindingType {
        match self.usage {
            TextureUsage::Storage => {
                wgpu::BindingType::StorageTexture {
                    access: wgpu::StorageTextureAccess::ReadWrite,
                    format: self.format(),
                    view_dimension: self.view_dimension(),
                }
            }

            _ => {
                let sample_type = match self.semantic {
                    TextureSemantic::Depth
                    | TextureSemantic::DepthStencil => {
                        wgpu::TextureSampleType::Depth
                    }

                    TextureSemantic::Hdr => {
                        wgpu::TextureSampleType::Float {
                            filterable: true,
                        }
                    }

                    TextureSemantic::Color
                    | TextureSemantic::Normal
                    | TextureSemantic::Data => {
                        wgpu::TextureSampleType::Float {
                            filterable: true,
                        }
                    }
                };

                wgpu::BindingType::Texture {
                    sample_type,
                    view_dimension: self.view_dimension(),
                    multisampled: self.sample_count() > 1,
                }
            }
        }
    }

    pub fn sampler_binding_type(&self)
        -> wgpu::SamplerBindingType
    {
        match self.semantic {

            TextureSemantic::Depth
            | TextureSemantic::DepthStencil => {
                wgpu::SamplerBindingType::Comparison
            }

            _ => {
                wgpu::SamplerBindingType::Filtering
            }
        }
    }

    pub fn supports_sampler(&self) -> bool {
        !matches!(self.usage, TextureUsage::Storage)
    }

    pub fn is_depth(&self) -> bool {
        matches!(
            self.semantic,
            TextureSemantic::Depth
                | TextureSemantic::DepthStencil
        )
    }

    pub fn is_storage(&self) -> bool {
        matches!(
            self.usage,
            TextureUsage::Storage
        )
    }

    pub fn is_render_target(&self) -> bool {
        matches!(
            self.usage,
            TextureUsage::RenderTarget
                | TextureUsage::ShadowMap
        )
    }

}