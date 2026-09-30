//! pygame::render —— **创新模块**：单一通用渲染管线（契约 ADR-4 / §六）
//!
//! 与 `font`（文字）/`gfx`（几何）殊途同归的引擎能力模块，为 pygame 的
//! 绘制世界而设：**不分 2D/3D**——一切绘制都是"顶点 + MVP 变换 + 材质"
//! 走同一条管线（[`shader/sprite.wgsl`]），2D 只是正交 MVP + 精灵 quad。
//!
//! 模块面（契约 §六 签名级契约的实现落位）：
//! - [`Texture`]：GPU 纹理（Rgba8Unorm，上传/采样/离屏三位一体）
//! - [`Screen`] / [`Surface`] / [`DrawTarget`]：绘制终点统一抽象
//!   （Screen = 交换链背板进程唯一；Surface = 纹理背板用户自由创建）
//! - [`Camera`]：MVP 组装（ortho/perspective 同型，唯一上传变换量）
//! - [`Batch`]：批处理执行体（纹理切换即段边界；即时 API × 延迟提交）
//! - [`Material`]：默认带纹理 alpha 混合材质（管线变体矩阵惰性缓存）
//! - [`BufferProxy`]：通用数据桥（ADR-10；v1 = Texture↔CPU 最小路径）
//!
//! GPU 槽（ADR-6 display 全局槽的前置形态）：[`Screen::new`] 建立
//! 进程级 Gpu 句柄，[`Surface`]/[`Texture`]/pygame::font 等经 [`gpu`]
//! 取用——pygame 签名不带 GPU 参数的依托。

pub mod batch;
pub mod buffer_proxy;
pub mod camera;
pub mod material;
pub mod target;
pub mod texture;

pub use batch::{Batch, GeometryKind, SpriteVertex};
pub use buffer_proxy::{BufferProxy, Layout, PixelFormat};
pub use camera::Camera;
pub use material::{Material, SPRITE_MRT_WGSL, SPRITE_WGSL};
pub use target::{render_targets, DrawTarget, RenderPass, Screen, Surface};
pub use texture::Texture;

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use crate::base::render::bind_group::bind_group::BindGroup;
use crate::base::render::render_resource_access::RenderResourceAccess;
use crate::base::render::sampler_desc::SamplerDescriptor;

/// blit 目标坐标（像素，y 向下；左上角为原点）
pub type Coord = (i32, i32);

/// 进程级 GPU 句柄：设备/队列 + 资源访问 + 材质 + 默认采样器 + 白纹理
pub(crate) struct Gpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub access: Arc<RenderResourceAccess>,
    pub material: Material,
    /// 默认采样器（linear clamp——pygame 贴图平滑语义）
    pub sampler: Arc<wgpu::Sampler>,
    /// 1×1 白纹理（纯色路径：fill/draw::* 的纹理入端）
    pub white: Texture,
    /// 白纹理 bind 本体（管线布局推导示例；不进缓存表）
    pub white_bind: BindGroup,
    /// 纹理 bind 缓存（texture 0 + sampler 1；按纹理指针键控）
    binds: RefCell<HashMap<usize, BindGroup>>,
    white_key: usize,
}

// GPU 槽 = 线程局部（主线程契约的类型级表达）：wasm 的 wgpu 类型非
// Send/Sync（Rc 底座），静态槽过不了 Sync 界；桌面/Android/web 三平台
// 的 pygame 调用本就全部落在主线程（ADR-6），TLS 顺带把越线程访问
// 挡在编译期。
thread_local! {
    static GPU_SLOT: RefCell<Option<Arc<Gpu>>> = const { RefCell::new(None) };
}

/// 建立（或复用）GPU 槽（`Screen::new` 调用；首次为准）
pub(crate) fn init_gpu(
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    access: Arc<RenderResourceAccess>,
) -> Arc<Gpu> {
    GPU_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Arc::new(Gpu::new(device, queue, access)));
        }
        slot.as_ref().expect("GPU 槽刚写入").clone()
    })
}

/// GPU 句柄（未初始化 = 契约违例 panic，ADR-9）
pub(crate) fn gpu() -> Arc<Gpu> {
    GPU_SLOT.with(|slot| {
        slot.borrow().as_ref().cloned().expect(
            "pygame 渲染未初始化：须先建立 GPU 槽（P2 = Screen::new；P3 = display.set_mode）",
        )
    })
}

impl Gpu {
    fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        access: Arc<RenderResourceAccess>,
    ) -> Self {
        let material = Material::new(&access);
        let sampler = Arc::new(
            access.create_sampler("pygame_sampler", &SamplerDescriptor::linear_clamp()),
        );
        let white = texture::upload_rgba8(&device, &queue, (1, 1), &[255, 255, 255, 255]);
        let white_key = white.cache_key();
        let white_bind = access
            .bind_group_builder()
            .texture_view(0, white.view())
            .sampler(1, sampler.clone())
            .build(Some("pygame_white_bind"));
        let gpu = Self {
            device,
            queue,
            access,
            material,
            sampler,
            white,
            white_bind,
            binds: RefCell::new(HashMap::new()),
            white_key,
        };
        gpu
    }

    /// 纹理 bind group 缓存（texture 0 + sampler 1；按纹理指针键控。
    /// 白纹理 bind 不入表——本体常驻 [`Gpu::white_bind`] 字段）
    pub(crate) fn ensure_bind_view(&self, key: usize, view: Arc<wgpu::TextureView>) {
        let mut binds = self.binds.borrow_mut();
        if let Entry::Vacant(e) = binds.entry(key) {
            e.insert(
                self.access
                    .bind_group_builder()
                    .texture_view(0, view)
                    .sampler(1, self.sampler.clone())
                    .build(Some("pygame_tex_bind")),
            );
        }
    }

    pub(crate) fn white_key(&self) -> usize {
        self.white_key
    }
}
