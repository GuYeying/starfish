use std::sync::Arc;



// ==============================================
// 渲染器：管理Surface、设备、队列、管线、配置
// ==============================================


pub struct RenderContext {
    //以后提供非常原始的接口能力！！！！！
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    surface: Arc<wgpu::Surface<'static>>,          // 渲染表面（对应窗口）
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
            surface:surface.clone(),
            device:device.clone(),
            queue:queue.clone(),
            features,
            limits,
            adapter_info,
        }

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
