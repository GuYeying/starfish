use wgpu::{
    Backends, CompositeAlphaMode, ExperimentalFeatures, Features, InstanceFlags, Limits, MemoryBudgetThresholds, MemoryHints, PowerPreference, PresentMode, SurfaceCapabilities, SurfaceColorSpace, SurfaceConfiguration, TextureFormat, TextureUsages, BackendOptions, DeviceDescriptor, InstanceDescriptor, RequestAdapterOptions, Surface, Adapter,
};

use super::features;

#[derive(Clone, Debug)]
pub struct SurfaceSettings {
    /// Swapchain 格式
    pub color_format: Option<TextureFormat>,
    pub depth_format: Option<TextureFormat>,
    pub usage: TextureUsages,
    pub present_mode: PresentMode,
    pub alpha_mode: Option<CompositeAlphaMode>,
    pub desired_maximum_frame_latency: u32,
    pub color_space: SurfaceColorSpace,
    /// 表面多重采样数（1 = 关闭 MSAA；常用 4）。渲染进该表面的管线需以
    /// 相同 `sample_count` 创建（`RenderPipelineBuilder::sample_count`）
    pub sample_count: u32,
}

impl Default for SurfaceSettings {
    fn default() -> Self {
        Self {
            color_format: None,
            depth_format: None,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_SRC,
            present_mode: PresentMode::Fifo,
            alpha_mode: None,
            desired_maximum_frame_latency: 2,
            color_space: SurfaceColorSpace::Auto,
            sample_count: 1,
        }
    }
}

impl SurfaceSettings {
    pub fn with_usage(mut self, usage: TextureUsages) -> Self {
        self.usage = usage;
        self
    }

    /// 开启多重采样（MSAA）。1 = 关闭；常用 4
    ///
    /// 渲染进该表面的所有管线需以相同 `sample_count` 创建；
    /// `RenderSurface::present` 时自动 resolve 到交换链。
    pub fn with_msaa(mut self, sample_count: u32) -> Self {
        self.sample_count = sample_count.max(1);
        self
    }

    pub fn with_alpha_mode(mut self, alpha_mode: CompositeAlphaMode) -> Self {
        self.alpha_mode = Some(alpha_mode);
        self
    }

    pub fn with_present_mode(mut self, mode: PresentMode) -> Self {
        self.present_mode = mode;
        self
    }

    pub fn with_color_format(mut self, format: TextureFormat) -> Self {
        self.color_format = Some(format);
        self
    }

    pub fn with_depth_format(mut self, format: TextureFormat) -> Self {
        self.depth_format = Some(format);
        self
    }


    pub fn with_frame_latency(mut self, latency: u32) -> Self {
        self.desired_maximum_frame_latency = latency.max(1);
        self
    }

    pub fn with_color_space(mut self, color_space: SurfaceColorSpace) -> Self {
        self.color_space = color_space;
        self
    }

    #[inline]
    fn default_view_formats(format: TextureFormat) -> Vec<TextureFormat> {
        match format {
            TextureFormat::Bgra8Unorm => vec![TextureFormat::Bgra8UnormSrgb],
            TextureFormat::Bgra8UnormSrgb => vec![TextureFormat::Bgra8Unorm],
            TextureFormat::Rgba8Unorm => vec![TextureFormat::Rgba8UnormSrgb],
            TextureFormat::Rgba8UnormSrgb => vec![TextureFormat::Rgba8Unorm],
            _ => Vec::new(),
        }
    }

    pub(crate) fn to_wgpu(
        self,
        caps: &SurfaceCapabilities,
        size: &(u32, u32),
        downlevel_caps: &wgpu::DownlevelCapabilities,
    ) -> SurfaceConfiguration {
        // format/alpha_mode 依赖 caps 运行时数据，只能保留 Option
        let format = self.color_format.unwrap_or(caps.formats[0]);
        let alpha_mode = self.alpha_mode.unwrap_or(caps.alpha_modes[0]);

        // usage / present_mode 走"许愿 → 掩码"：与 caps 实际能力取交集，
        // 请求超出时自动剥离/回落——WebGL2 表面通常只支持
        // RENDER_ATTACHMENT|TEXTURE_BINDING 与 Fifo，硬请求会炸 configure
        let usage = {
            let masked = self.usage & caps.usages;
            if masked.is_empty() {
                wgpu::TextureUsages::RENDER_ATTACHMENT
            } else {
                masked
            }
        };
        let present_mode = if caps.present_modes.contains(&self.present_mode) {
            self.present_mode
        } else {
            wgpu::PresentMode::Fifo
        };

        // view_formats（Unorm↔Srgb 重解释视图）需 DownlevelFlags::SURFACE_VIEW_FORMATS：
        // 桌面 Vulkan/DX12/Metal 支持；GLES/WebGL 与 Android Vulkan 均不支持
        // （wgpu 明确标注），configure 校验直接失败 → Web 恒置空（坑位 4），
        // native 按适配器能力运行时掩码（实测：Android 上未掩码时 configure 直接 panic）
        #[cfg(target_arch = "wasm32")]
        let view_formats = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        let view_formats = if downlevel_caps
            .flags
            .contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS)
        {
            Self::default_view_formats(format)
        } else {
            Vec::new()
        };

        SurfaceConfiguration {
            desired_maximum_frame_latency: self.desired_maximum_frame_latency,
            present_mode,
            alpha_mode,
            format,
            usage,
            color_space: self.color_space,
            view_formats,
            width: size.0.max(1),
            height: size.1.max(1),
        }
    }
}





























#[derive(Clone, Debug)]
pub struct GpuSettings {
    
    pub(crate) use_depth: bool,


    // InstanceDescriptor
    pub backends: Backends,
    pub flags: InstanceFlags,
    pub memory_budget_thresholds: MemoryBudgetThresholds,

    // RequestAdapterOptions
    pub power_preference: PowerPreference,
    pub force_fallback_adapter: bool,
    pub apply_limit_buckets: bool,

    // DeviceDescriptor
    /// 能力愿望清单（创建设备时经掩码取「愿望 & 硬件能力」，默认 = `features::core()`）
    pub desired_features: Features,
    /// 上限愿望（创建设备时钳制到硬件支持范围内）
    pub required_limits: Limits,
    pub memory_hints: MemoryHints,
}

impl Default for GpuSettings {
    fn default() -> Self {
        Self {
            use_depth: true,
            backends: Backends::all(),
            flags: if cfg!(debug_assertions) {
                InstanceFlags::DEBUG | InstanceFlags::VALIDATION
            } else {
                InstanceFlags::empty()
            },
            memory_budget_thresholds: MemoryBudgetThresholds::default(),
            power_preference: PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
            desired_features: features::core(),
            required_limits: Limits::defaults(),
            memory_hints: MemoryHints::Performance,
        }
    }
}

impl GpuSettings {

    pub fn with_depth(mut self, use_depth: bool) -> Self {
        self.use_depth = use_depth;
        self
    }

    pub fn with_backends(mut self, backends: Backends) -> Self {
        self.backends = backends;
        self
    }

    pub fn with_flags(mut self, flags: InstanceFlags) -> Self {
        self.flags = flags;
        self
    }

    pub fn with_memory_budget_thresholds(mut self, memory_budget_thresholds: MemoryBudgetThresholds) -> Self {
        self.memory_budget_thresholds = memory_budget_thresholds;
        self
    }

    pub fn with_power_preference(mut self, power_preference: PowerPreference) -> Self {
        self.power_preference = power_preference;
        self
    }

    pub fn with_fallback_adapter(mut self, enable: bool) -> Self {
        self.force_fallback_adapter = enable;
        self
    }

    pub fn with_limit_buckets(mut self, enable: bool) -> Self {
        self.apply_limit_buckets = enable;
        self
    }

    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.required_limits = limits;
        self
    }

    pub fn with_memory_hints(mut self, hints: MemoryHints) -> Self {
        self.memory_hints = hints;
        self
    }

    /// 替换能力愿望清单（创建设备时仍会经掩码取「愿望 & 硬件能力」）
    pub fn with_features(mut self, features: Features) -> Self {
        self.desired_features = features;
        self
    }

    /// 追加能力愿望（原始 `wgpu::Features` 位直通，见 `features` 模块速查表）
    pub fn enable_features(mut self, features: Features) -> Self {
        self.desired_features |= features;
        self
    }

    /// 摘除能力愿望
    pub fn disable_features(mut self, features: Features) -> Self {
        self.desired_features.remove(features);
        self
    }



    pub(crate) fn to_instance(&self) -> InstanceDescriptor {
        InstanceDescriptor {
            backends: self.backends,
            flags: self.flags,
            memory_budget_thresholds: self.memory_budget_thresholds.clone(),
            backend_options: BackendOptions::default(),
            display: None,
        }
    }

    pub(crate) fn to_adapter<'a>(
        &'a self,
        surface: &'a Surface<'a>,
    ) -> RequestAdapterOptions<'a, 'a> {
        RequestAdapterOptions {
            power_preference: self.power_preference,
            force_fallback_adapter: self.force_fallback_adapter,
            compatible_surface: Some(surface),
            apply_limit_buckets: self.apply_limit_buckets,
        }
    }

    /// 掩码后的实际能力位 = 愿望 & 硬件能力
    pub(crate) fn resolve_features(&self, adapter: &Adapter) -> Features {
        features::resolve(self.desired_features, adapter)
    }

    /// 钳制到硬件支持范围内的实际上限
    pub(crate) fn resolve_limits(&self, adapter: &Adapter) -> Limits {
        adapter.limits().or_worse_values_from(&self.required_limits)
    }

    pub(crate) fn to_device(&self, adapter: &Adapter) -> DeviceDescriptor<'_> {
        // 许愿 → 掩码：wgpu 30 的 DeviceDescriptor 无 optional_features，
        // 「愿望 & 硬件能力」掩码进 required 即等价实现，
        // 硬件不支持的位自动剥离，设备创建永不因愿望失败
        DeviceDescriptor {
            label: Some("Starfish Device"),
            required_features: self.resolve_features(adapter),
            required_limits: self.resolve_limits(adapter),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: self.memory_hints.clone(),
            trace: Default::default(),
        }
    }
}