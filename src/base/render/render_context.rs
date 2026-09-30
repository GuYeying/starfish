use std::sync::Arc;



// ==============================================
// 渲染器：管理Surface、设备、队列、管线、配置
// ==============================================


pub struct RenderContext {
    //以后提供非常原始的接口能力！！！！！
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// 渲染表面（对应窗口）。**Option = 设备/表面生命周期解绑**（wgpu 本形
    /// 初始化序）：设备资源（纹理等）可在窗口存在前创建（[`Self::new_headless`]），
    /// 表面由 `RenderEntry::surface_from_context` 后挂。
    surface: Option<Arc<wgpu::Surface<'static>>>,
    device: Arc<wgpu::Device>,                // GPU逻辑设备（核心）
    queue: Arc<wgpu::Queue>,                  // GPU命令队列

    /// 掩码后实际获得的能力位（开发者能力分支的依据，见 `features` 模块）
    features: wgpu::Features,
    /// 钳制到硬件后实际生效的上限
    limits: wgpu::Limits,
    /// 适配器信息（后端 / 显卡名）
    adapter_info: wgpu::AdapterInfo,
}

impl RenderContext{
    pub(crate) fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        surface: &Arc<wgpu::Surface<'static>>,          // 渲染表面（对应窗口）
        device: &Arc<wgpu::Device>,                // GPU逻辑设备（核心）
        queue: &Arc<wgpu::Queue>,                  // GPU命令队列
        features: wgpu::Features,                  // 掩码后实际获得的能力位
        limits: wgpu::Limits,                      // 实际生效的上限
        adapter_info: wgpu::AdapterInfo,           // 适配器信息
    )->Self{
        Self {
            instance:instance,
            adapter,
            surface:Some(surface.clone()),
            device:device.clone(),
            queue:queue.clone(),
            features,
            limits,
            adapter_info,
        }

    }

    /// 无窗口独立上下文（设备先行形态）：instance/adapter/device/queue 就位，
    /// 表面留空——纹理/采样器/bind group 等设备资源即刻可建（与格式无关），
    /// 窗口就位后经 `RenderEntry::surface_from_context` 挂表面。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_headless(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        features: wgpu::Features,
        limits: wgpu::Limits,
        adapter_info: wgpu::AdapterInfo,
    ) -> Self {
        Self {
            instance,
            adapter,
            surface: None,
            device,
            queue,
            features,
            limits,
            adapter_info,
        }
    }

    /// 独立资源访问层（本 context 的 device/queue + 指定默认格式）。
    ///
    /// 设备先行形态用：窗口未挂时以**占位格式**构造（bind group/纹理/采样器
    /// 与格式无关，照常创建）；**管线必须以真实表面格式**的 access 构建
    /// （颜色目标格式随表面），表面挂载后由表面格式重建。
    pub fn resource_access(
        &self,
        default_color_format: wgpu::TextureFormat,
        default_depth_format: wgpu::TextureFormat,
    ) -> crate::base::render::render_resource_access::RenderResourceAccess {
        crate::base::render::render_resource_access::RenderResourceAccess::new(
            self.device.clone(),
            self.queue.clone(),
            default_color_format,
            default_depth_format,
        )
    }

    /// 实际获得的能力位（愿望经掩码后的结果）
    ///
    /// 开发者据此自行分支（引擎不实现降级路径），例：
    /// `if ctx.features().contains(wgpu::Features::SHADER_F16) { ... }`
    pub fn features(&self) -> wgpu::Features {
        self.features
    }

    /// 实际生效的资源上限（已钳制到硬件支持范围）
    pub fn limits(&self) -> wgpu::Limits {
        self.limits.clone()
    }

    /// 适配器信息（后端 / 显卡名 / 驱动）
    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.adapter_info
    }

    /// 适配器（原始访问；表面能力查询等场景）
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    /// GPU 逻辑设备（原始访问）
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// 命令队列（原始访问）
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }

    pub(crate) fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }
}
