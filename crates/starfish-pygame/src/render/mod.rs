//! pygame::render —— **创新模块**：单一通用渲染管线（契约 ADR-4 / §六）
//!
//! 与 `font`（文字）/`gfx`（几何）殊途同归的引擎能力模块，为 pygame 的
//! 绘制世界而设：**不分 2D/3D**——一切绘制都是"顶点 + MVP 变换 + 材质"
//! 走同一条管线（[`shader/sprite.wgsl`]），2D 只是正交 MVP + 精灵 quad。
//!
//! 模块面（契约 §六 签名级契约的实现落位）：
//! - [`Texture`]：GPU 纹理（Rgba8Unorm，上传/采样/离屏三位一体）
//! - [`Screen`] / [`Surface`] / [`DrawTarget`]：绘制终点统一抽象
//!   （Screen = 交换链背板进程唯一；Surface = 离屏画布 + 空间可放置
//!   纹理对象——无相机字段，全局相机隐式）
//! - [`Camera`] / [`CameraState`]：MVP 组装（ortho/perspective 同型，
//!   唯一上传变换量）+ 全局相机状态机（Auto2d/Auto3d/Manual）
//! - [`Batch`]：批处理执行体（纹理切换即段边界；即时 API × 延迟提交）
//! - [`SpriteMaterial`]（内部）：pygame 固定渲染管线的状态材质（管线
//!   变体矩阵惰性缓存；不对外暴露——定制管线走 RenderResourceAccess）
//!
//! **全局相机定约**：相机 uniform（`@group(0)@binding(0) 64B mat4x4
//! MVP`）全局唯一——pack 时按 [`CameraState`] 解出（Auto = 按目标尺寸
//! 隐式；Manual = [`set_default_camera`] override）。sprite/调试线/font 管线
//! 同构共享；egui 客人经适配层跟随。一处设置、全体生效。
//!
//! GPU 槽（ADR-6 display 全局槽的前置形态）：[`Screen::new`] 建立
//! 进程级 Gpu 句柄，[`Surface`]/[`Texture`]/pygame::font 等经 [`gpu`]
//! 取用——pygame 签名不带 GPU 参数的依托。

pub mod batch;
pub mod camera;
pub mod material;
pub mod session;
pub mod target;
pub mod texture;

pub use batch::{Batch, BatchTopology, GeometryKind, SpriteVertex};
pub use camera::{Camera, CameraState};
pub use material::{SpriteBlend, SPRITE_MRT_WGSL, SPRITE_WGSL};
pub use session::{begin_screen, begin_targets, end_screen, end_targets, with_screen,
    with_targets, NoActiveSession};
pub use target::{render_targets, render_targets_arc, DrawTarget, OverlayCtx, RenderPass, Screen,
    Surface};
pub(crate) use target::CpuAlpha;
pub use texture::Texture;

// ── starfish-render 资源门面（重导出——绑定层包装与客人自建管线用）──
// 与 pygame 同名类型（starfish Texture / RenderPass 包装）冲突者不入
// 本层；wgpu 裸对象（Device/Queue/Buffer）不出 Python 界（暴露面收缩
// 定约：ResourceAccess + 主屏 RenderSurface 两入口）。
pub use starfish_render::bind_group::bind_group::BindGroup;
pub use starfish_render::bind_group::builder::BindGroupBuilder;
pub use starfish_render::mesh::mesh::Mesh;
pub use starfish_render::pipeline::RenderPipeline;
pub use starfish_render::render_pass::attachments::{ColorAttachment, DepthAttachment};
pub use starfish_render::render_resource_access::RenderResourceAccess;
pub use starfish_render::render_surface::RenderSurface;
pub use starfish_render::shader_module::shader_module::ShaderModule;

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use glam::Mat4;
use starfish_render::pipeline::CullMode;
use starfish_render::sampler_desc::SamplerDescriptor;

use material::SpriteMaterial;

/// blit 目标坐标（像素，y 向下；左上角为原点）
pub type Coord = (i32, i32);

/// 整型矩形 `[x, y, w, h]`（GPU 几何管线内部用：clip/scissor/blit_area
/// ——**纯数组别名，无 pygame.Rect 的方法面**）。GPU 全帧 present 下
/// "描述 surface 信息"的 Rect 门面已退役（2026-10-24 定约：碰撞/触发器
/// 归物理引擎）；本别名只承载裁剪/区域搬运的必要几何。
pub type IRect = [i32; 4];

/// 进程级 GPU 句柄：设备/队列 + 资源访问 + 材质 + 默认采样器 + 白纹理
/// + **全局相机 rig**（全局相机定约：全局唯一 uniform/bind，pack 时按
/// CameraState 解 MVP——Screen/Surface 无相机字段）
pub(crate) struct Gpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    /// 主屏 access（默认颜色格式 = 交换链格式）
    pub access: Arc<RenderResourceAccess>,
    /// 离屏 access（默认颜色格式 = Rgba8Unorm——Surface/Texture 离屏
    /// 目标恒此格式）。客人渲染器（UI 库/物理调试线等自持管线者）按
    /// **渲染目标**选 access 建管线：wgpu 附件格式严格相等校验，交换链
    /// 格式随平台漂移（Web/macOS = Bgra8、Windows = Rgba8）而离屏恒
    /// Rgba8——"管线按 (color_format, sample_count) 分套"多窗口定约的
    /// 叠加目标版。
    pub offscreen_access: Arc<RenderResourceAccess>,
    /// 固定管线状态材质（cull/blend 创建期愿望定型）
    pub material: SpriteMaterial,
    /// 全局相机 rig（uniform + bind；mvp 由 pack 按状态现写）
    pub camera: RefCell<target::CameraRig>,
    /// 全局相机状态机（Auto2d / Auto3d / Manual）
    pub camera_state: RefCell<CameraState>,
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
    /// 渲染愿望槽（pygame.render.init / pygame.init(dim)——set_mode 前
    /// 配置，Gpu 装配时消费一次；后设优先）
    static WISH_SLOT: RefCell<RenderWish> = RefCell::new(RenderWish::DEFAULT);
}

/// 渲染愿望（创建期细粒度控制——剔除/混合/相机维；Gpu 装配消费一次）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderWish {
    /// dim=3（透视默认相机）；false = 2D 正交（默认）
    pub dim3d: bool,
    /// Auto3d 透视参数（fov 度数制）
    pub fov_deg: f32,
    pub near: f32,
    pub far: f32,
    /// 剔除（默认 None——镜像矩阵绕序天然安全；开 back/front 后镜像
    /// quad 背面消失属物理正确）
    pub cull: CullMode,
    /// 混合（默认 Alpha）
    pub blend: SpriteBlend,
}

impl RenderWish {
    pub const DEFAULT: Self = Self {
        dim3d: false,
        fov_deg: 60.0,
        near: 0.1,
        far: 100.0,
        cull: CullMode::None,
        blend: SpriteBlend::Alpha,
    };

    /// 相机状态初值（Gpu 装配时定型）
    fn camera_state(&self) -> CameraState {
        if self.dim3d {
            CameraState::Auto3d { fov_deg: self.fov_deg, near: self.near, far: self.far }
        } else {
            CameraState::Auto2d
        }
    }
}

/// 渲染愿望写入（pygame.render.init——set_mode 前配置；整只覆盖）
pub fn store_wish(wish: RenderWish) {
    WISH_SLOT.with(|w| *w.borrow_mut() = wish);
}

/// 当前渲染愿望（含未配置的默认值）
pub fn current_wish() -> RenderWish {
    WISH_SLOT.with(|w| *w.borrow())
}

/// 建立（或复用）GPU 槽（`Screen::new` 调用；首次为准——愿望随首次
/// 装配消费定型）
pub(crate) fn init_gpu(
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    access: Arc<RenderResourceAccess>,
    offscreen_access: Arc<RenderResourceAccess>,
) -> Arc<Gpu> {
    GPU_SLOT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Arc::new(Gpu::new(device, queue, access, offscreen_access)));
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

/// 外接渲染器创建期所需的 GPU 句柄快照（**接口核心 = 通用能力**：
/// 任何自持管线的直绘消费者——UI 库、物理调试线等——的初始化依赖；
/// 全程只暴露 wgpu / starfish-render 公共类型，依赖单向红线的接口面）
pub struct GpuInfo {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub access: Arc<RenderResourceAccess>,
    /// 离屏目标 access（Rgba8Unorm；见 [`Gpu::offscreen_access`]）
    pub offscreen_access: Arc<RenderResourceAccess>,
}

/// GPU 句柄快照（[`Screen::new`] 建立槽之后可用；未建立 = `None`——
/// 绑定层映射为"须先 set_mode"的用法错误，引擎侧不 panic）
pub fn gpu_info() -> Option<GpuInfo> {
    GPU_SLOT.with(|slot| {
        slot.borrow().as_ref().map(|g| GpuInfo {
            device: g.device.clone(),
            queue: g.queue.clone(),
            access: g.access.clone(),
            offscreen_access: g.offscreen_access.clone(),
        })
    })
}

// ── 全局相机 API（一处设置、全体生效）──────────────────────────────

/// 全局相机 override（→ Manual：所有后续渲染生效——本帧剩余批次、其它
/// Surface、后续帧；直到 [`reset_camera`]）。会话形态
/// `RenderPass::set_default_camera` 同语义转发。
pub fn set_default_camera(camera: &Camera) {
    let g = gpu();
    *g.camera_state.borrow_mut() = CameraState::Manual(*camera);
}

/// 回自动相机（Auto2d / Auto3d——按创建期愿望）
pub fn reset_camera() {
    let g = gpu();
    *g.camera_state.borrow_mut() = current_wish().camera_state();
}

/// 当前全局相机状态
pub fn camera_state() -> CameraState {
    *gpu().camera_state.borrow()
}

/// 当前全局 MVP（Auto 状态按主屏尺寸现算——resize 天然跟随；
/// Manual = 用户相机）
pub fn camera_mvp() -> Mat4 {
    let size = crate::display::get_screen().size();
    camera_mvp_for(size)
}

/// 按目标尺寸解全局 MVP（客人合成用——Auto 状态按给定尺寸隐式、
/// Manual = 用户 override 与尺寸无关：与目标层 pack 同一解算，
/// "一处设置、全体生效"定约的客人侧入口）
pub fn camera_mvp_for(size: (u32, u32)) -> Mat4 {
    let g = gpu();
    let state = *g.camera_state.borrow();
    state.resolve_mvp((size.0.max(1) as f32, size.1.max(1) as f32))
}

impl Gpu {
    fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        access: Arc<RenderResourceAccess>,
        offscreen_access: Arc<RenderResourceAccess>,
    ) -> Self {
        // 愿望消费（一次定型：剔除/混合进材质，相机维进状态机初值）
        let wish = current_wish();
        let material = SpriteMaterial::new(&access, wish.cull, wish.blend);
        let camera_state = RefCell::new(wish.camera_state());
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
        // 全局相机 rig：uniform/bind 全局唯一；mvp 由 pack 按 CameraState
        // 现写（初始 ortho(1,1) 仅占位）
        let gpu0 = Self {
            device: device.clone(),
            queue: queue.clone(),
            access: access.clone(),
            offscreen_access,
            material,
            camera: RefCell::new(target::CameraRig::raw(
                &device,
                &access,
            )),
            camera_state,
            sampler,
            white,
            white_bind,
            binds: RefCell::new(HashMap::new()),
            white_key,
        };
        gpu0
    }

    /// pack 期相机解算：按当前状态解出目标尺寸的 MVP 写入全局 rig
    ///（Auto 状态随目标隐式；Manual 与尺寸无关）——返回 rig 的借用
    /// 供 EncodeCtx 消费。
    pub(crate) fn camera_for(
        &self,
        size: (u32, u32),
    ) -> std::cell::RefMut<'_, target::CameraRig> {
        let mut rig = self.camera.borrow_mut();
        let state = *self.camera_state.borrow();
        rig.mvp = state.resolve_mvp((size.0.max(1) as f32, size.1.max(1) as f32));
        rig
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
