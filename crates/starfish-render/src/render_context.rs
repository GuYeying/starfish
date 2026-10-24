use std::sync::Arc;



// ==============================================
// 渲染上下文：instance/adapter/device/queue（设备族，进程一份）
// ==============================================
// 多窗口形态（2026-10-23 定稿）：上下文不持有表面——表面（RenderSurface）
// 每窗一份、共享本上下文（见 `RenderEntry::new_context` → `new_surface`）。

pub struct RenderContext {
    //以后提供非常原始的接口能力！！！！！
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
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
    /// 无窗口独立上下文：instance/adapter/device/queue 就位，表面按窗口
    /// 另建（`RenderEntry::new_surface`）——纹理/采样器/bind group 等设备
    /// 资源即刻可建（与格式无关）；**管线须以真实表面格式**的 access 构建
    /// （颜色目标格式随表面）。
    pub(crate) fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: &Arc<wgpu::Device>,                // GPU逻辑设备（核心）
        queue: &Arc<wgpu::Queue>,                  // GPU命令队列
        features: wgpu::Features,                  // 掩码后实际获得的能力位
        limits: wgpu::Limits,                      // 实际生效的上限
        adapter_info: wgpu::AdapterInfo,           // 适配器信息
    )->Self{
        Self {
            instance,
            adapter,
            device: device.clone(),
            queue: queue.clone(),
            features,
            limits,
            adapter_info,
        }

    }

    /// 独立资源访问层（本 context 的 device/queue + 指定默认格式）。
    ///
    /// 多窗口共享约定：各窗口 [`crate::settings::SurfaceSettings`] 一致
    /// （默认即一致）时，一份 access 全窗口共享；颜色格式/MSAA 不同的
    /// 窗口以本方法按新格式另建。设备先行形态用：窗口未挂时以**占位格式**
    /// 构造（bind group/纹理/采样器与格式无关，照常创建）；**管线必须以
    /// 真实表面格式**的 access 构建（颜色目标格式随表面），表面挂载后由
    /// 表面格式重建。
    pub fn resource_access(
        &self,
        default_color_format: wgpu::TextureFormat,
        default_depth_format: wgpu::TextureFormat,
    ) -> crate::render_resource_access::RenderResourceAccess {
        crate::render_resource_access::RenderResourceAccess::new(
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
