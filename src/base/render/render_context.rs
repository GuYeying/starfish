use std::sync::Arc;
use raw_window_handle::HandleError;



// ==============================================
// 渲染器：管理Surface、设备、队列、管线、配置
// ==============================================


pub struct RenderContext {
    //以后提供非常原始的接口能力！！！！！
    instance: wgpu::Instance,
    surface: Arc<wgpu::Surface<'static>>,          // 渲染表面（对应窗口）
    device: Arc<wgpu::Device>,                // GPU逻辑设备（核心）
    queue: Arc<wgpu::Queue>,                  // GPU命令队列

    // render_frame: RenderFrame<'a>,
    // render_resource_access: RenderResourceAccess,
}

impl RenderContext{
    pub(crate) fn new(
        instance: wgpu::Instance,
        surface: &Arc<wgpu::Surface<'static>>,          // 渲染表面（对应窗口）
        device: &Arc<wgpu::Device>,                // GPU逻辑设备（核心）
        queue: &Arc<wgpu::Queue>,                  // GPU命令队列
    )->Self{
        Self { 
            instance:instance, 
            surface:surface.clone(), 
            device:device.clone(), 
            queue:queue.clone() 
        }

    }
}
