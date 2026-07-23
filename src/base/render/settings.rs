use wgpu::{
    Backends, CompositeAlphaMode, ExperimentalFeatures, Features, InstanceFlags, Limits, MemoryBudgetThresholds, MemoryHints, PowerPreference, PresentMode, SurfaceCapabilities, SurfaceColorSpace, SurfaceConfiguration, TextureFormat, TextureUsages, BackendOptions, DeviceDescriptor, InstanceDescriptor, RequestAdapterOptions, Surface,
};

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
        }
    }
}

impl SurfaceSettings {
    pub fn with_usage(mut self, usage: TextureUsages) -> Self {
        self.usage = usage;
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
    ) -> SurfaceConfiguration {
        // format/alpha_mode 依赖 caps 运行时数据，只能保留 Option
        let format = self.color_format.unwrap_or(caps.formats[0]);
        let alpha_mode = self.alpha_mode.unwrap_or(caps.alpha_modes[0]);

        SurfaceConfiguration {
            desired_maximum_frame_latency: self.desired_maximum_frame_latency,
            present_mode: self.present_mode,
            alpha_mode,
            format,
            usage: self.usage,
            color_space: self.color_space,
            view_formats: Self::default_view_formats(format),
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
    pub required_features: Features,
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
            required_features: Features::default(),
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

    pub fn with_features(mut self, features: Features) -> Self {
        self.required_features = features;
        self
    }

    pub fn enable_features(mut self, features: Features) -> Self {
        self.required_features |= features;
        self
    }

    pub fn disable_features(mut self, features: Features) -> Self {
        self.required_features.remove(features);
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

    pub(crate) fn to_device<'a>(&self) -> DeviceDescriptor<'a> {
        DeviceDescriptor {
            label: Some("Starfish Device"),
            required_features: self.required_features.clone(),
            required_limits: self.required_limits.clone(),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: self.memory_hints.clone(),
            trace: Default::default(),
        }
    }
}