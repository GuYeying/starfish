//! pygame::render 绘制终点：DrawTarget / Screen / Surface / 渲染会话 / MRT
//!
//! | | `Screen`（set_mode 产物） | `Surface`（离屏面） |
//! |---|---|---|
//! | 背板 | swapchain（窗口呈现） | GPU 纹理（可选深度） |
//! | 生命周期 | 进程唯一 | 用户自由创建 N 个 |
//! | 作 blit 源 | ❌（呈现缓冲不回读） | ✅ 纹理采样即 blit |
//!
//! 统一方式 = [`DrawTarget`]（fill/blit/size）；绘制方法 **`&self`**（ADR-6
//! 拍板：内部 RefCell——`let screen = display.get_screen()` 拿一次到处用的
//! 教程形态 + Python 绑定零翻译）。执行 = ADR-5：即时 API 积攒进 batch，
//! [`Screen::present`] / [`Surface::flush`] 统一提交。
//!
//! **渲染会话（ADR-5 v1.2，Rust 版 with 语法）**：`screen.render()` /
//! `surface.render()` 产出会话对象——创建即开始指令打包，`end()`/Drop
//! 完成打包；`finish(&self)` 为幂等打包口（**Python `__exit__` 的绑定
//! 形态**——`with screen:` 的 `__enter__` 返回会话句柄、`__exit__` 调
//! finish，Drop 兜底回收）。深度变体 = `render_depth()`；多渲染目标 =
//! [`render_targets`]（N≥2 颜色 + 双输出着色器）。

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use glam::Mat4;

use crate::base::render::bind_group::bind_group::BindGroup;
use crate::base::render::render_entry::{RenderContextError, RenderEntry};
use crate::base::render::render_surface::RenderSurface;
use crate::base::render::settings::{GpuSettings, SurfaceSettings};
use crate::base::window::Window;
use crate::pygame::{Color, Rect};

use super::batch::{encode, Batch, EncodeCtx, PassColor, PassDepth};
use super::camera::Camera;
use super::texture::Texture;
use super::{init_gpu, gpu, Coord, Gpu};

/// 纯色路径的顶点色（白纹理 × 白色 = 原色）
// blit 顶点色统一走 `Surface::blit_tint()`（白 × 表面 alpha）——
// 不透明基准色 [1.0; 4] 已并入该函数

fn rgba(c: impl Into<Color>) -> [f32; 4] {
    let (r, g, b, a) = c.into().normalize();
    [r, g, b, a]
}

/// 绘制终点统一抽象（契约 ADR-2）
///
/// ⚠️ 执行语义 = **指令打包 + 延迟提交**（ADR-5）：fill/blit/draw::*
/// 全部即时调用、内部录制进目标 batch，真正的 GPU 提交发生在
/// [`Screen::present`] / [`Surface::flush`] / 会话 [`end`](ScreenFrame::end)
/// ——不是即时上屏。需要显式打包边界用 [`Screen::render`] /
/// [`Surface::render`]（Rust 版 with 语法）。
pub trait DrawTarget {
    /// 填充整个目标（返回受影响区域）
    fn fill(&self, color: impl Into<Color>) -> Rect;
    /// 把离屏面画到目标 `(x, y)` 处（源自动 flush；返回目标 Rect）
    fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect;
    /// 目标尺寸（像素）
    fn size(&self) -> (u32, u32);
    /// batch 级绘制入口（`draw::*` / 自定义图元的执行通道；
    /// Screen 实现内含首帧自动 begin）
    fn with_batch(&self, f: impl FnOnce(&mut Batch));
}

/// 相机 rig：uniform 缓冲 + bind group + 当前 MVP（每目标一套）
pub(crate) struct CameraRig {
    pub buf: Arc<wgpu::Buffer>,
    pub bind: BindGroup,
    pub mvp: Mat4,
}

impl CameraRig {
    pub fn ortho(gpu: &Gpu, size: (u32, u32)) -> Self {
        let buf = gpu.access.create_raw_buffer(
            Some("pygame_camera"),
            64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let bind = gpu
            .access
            .bind_group_builder()
            .uniform_raw(0, buf.clone(), 64)
            .build(Some("pygame_camera_bind"));
        let mut rig = Self {
            buf,
            bind,
            mvp: Mat4::IDENTITY,
        };
        rig.set_ortho(size);
        rig
    }

    pub fn set_ortho(&mut self, size: (u32, u32)) {
        self.mvp = Camera::ortho(size.0.max(1) as f32, size.1.max(1) as f32).mvp();
    }

    /// 替换为自定义相机（透视 / 位姿——深度会话与 3D 内容用）
    pub fn set(&mut self, camera: &Camera) {
        self.mvp = camera.mvp();
    }
}

// ── Screen（交换链背板，进程唯一）────────────────────────────────

/// 屏幕绘制终点（P3 `display::set_mode` 的产物；探针可直接构造）
pub struct Screen {
    gpu: Arc<Gpu>,
    surface: RefCell<RenderSurface>,
    /// 本帧是否已 begin（交换链纹理已 acquire）
    begun: Cell<bool>,
    batch: RefCell<Batch>,
    camera: RefCell<CameraRig>,
    /// 裁剪区（`set_clip`；None = 不裁剪）
    clip: Cell<Option<Rect>>,
    format: wgpu::TextureFormat,
    samples: u32,
    /// 透明底模式（TRANSPARENT 创建旗标；begin_frame 清屏改 alpha 0
    /// ——桌宠/悬浮件形态，配表面 alpha 合成）
    transparent_bg: Cell<bool>,
}

impl Screen {
    /// 建窗 + GPU 装配（默认配置；进程一次）
    pub async fn new(window: &Window) -> Result<Self, RenderContextError> {
        Self::new_with(window, SurfaceSettings::default(), GpuSettings::default()).await
    }

    /// [`Self::new`] 的全配置版（MSAA / 呈现模式等经 SurfaceSettings）
    pub async fn new_with(
        window: &Window,
        surface_settings: SurfaceSettings,
        gpu_settings: GpuSettings,
    ) -> Result<Self, RenderContextError> {
        let (context, access, surface) =
            RenderEntry::async_new(window, surface_settings, gpu_settings).await?;
        let format = surface.color_format();
        let samples = surface.sample_count();
        let size = surface.size();
        let gpu = init_gpu(context.device().clone(), context.queue().clone(), Arc::new(access));
        let camera = CameraRig::ortho(&gpu, size);
        Ok(Self {
            gpu,
            surface: RefCell::new(surface),
            begun: Cell::new(false),
            batch: RefCell::new(Batch::new()),
            camera: RefCell::new(camera),
            clip: Cell::new(None),
            format,
            samples,
            transparent_bg: Cell::new(false),
        })
    }

    /// 透明底模式（TRANSPARENT 创建旗标）：begin_frame 清屏改 alpha 0
    ///（桌宠/悬浮件形态；须配表面 alpha 合成——set_mode_ex 已设）
    pub(crate) fn set_transparent_bg(&self) {
        self.transparent_bg.set(true);
    }

    /// 窗口尺寸变化（交换链 + 深度重建 + 相机正交跟随）
    pub fn resize(&self, width: u32, height: u32) {
        self.surface.borrow_mut().resize(width, height);
        self.camera.borrow_mut().set_ortho((width, height));
    }

    /// 帧提交（对应 pygame `display.flip`）：打包残余 batch → 与 base
    /// 清屏命令合并提交 → 交换链 present；未 begin（帧未就绪）则空转
    pub fn present(&self) {
        if !self.begun.get() {
            return;
        }
        self.begun.set(false);
        self.pack(false);
        // present 对"begin 未就绪无帧"自身免疫（跳过路径）
        self.surface.borrow_mut().present();
        self.batch.borrow_mut().clear();
    }

    /// 渲染通道会话（**Rust 版 with 语法**，ADR-5 v1.2）：创建即开始指令
    /// 打包（顺带 acquire 交换链纹理），[`RenderPass::end`] / Drop 完成
    /// 打包（编码挂入待提交队列），[`Self::present`] 上屏。Python 绑定
    /// 形态 = `with screen:`（`__enter__`=本方法 / `__exit__`=finish）。
    pub fn render(&self) -> RenderPass<'_> {
        self.ensure_begun();
        RenderPass::screen(self, false)
    }

    /// 深度会话：绘制走深度测试管线（Standard + 写入），深度缓冲由
    /// base 帧清屏（每帧 begin 清 1.0）。3D 内容 / 2.5D 遮挡用；
    /// 相机经 [`Self::set_camera`] 换透视。
    pub fn render_depth(&self) -> RenderPass<'_> {
        self.ensure_begun();
        RenderPass::screen(self, true)
    }

    /// 替换相机（正交默认；`Camera::perspective` + 位姿供深度会话）
    pub fn set_camera(&self, camera: &Camera) {
        self.camera.borrow_mut().set(camera);
    }

    /// 裁剪区（scissor；作用于本目标全部绘制，None = 不裁剪）
    pub fn set_clip(&self, clip: Option<Rect>) {
        self.clip.set(clip);
    }

    /// 当前裁剪区
    pub fn get_clip(&self) -> Option<Rect> {
        self.clip.get()
    }

    /// batch 级绘制入口（`draw::*` 模块 P3 在其上包 pygame 词汇）
    ///
    /// 首次交互自动 begin 帧（透明清屏——`fill` 随后以 quad 覆盖，语义统一）
    pub fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        if self.ensure_begun() {
            f(&mut self.batch.borrow_mut());
        }
    }

    fn ensure_begun(&self) -> bool {
        if self.begun.get() {
            return true;
        }
        // 清屏色用不透明黑：fill 是覆盖 quad（消除"首个 fill 走清屏"特例）。
        // 曾用透明清屏——Android 合成器把未初始化/空帧合成时常显示为白
        // （2026-09-29 批次 D 真机白闪根因之一）；黑与 Web 页黑背景一致，
        // 视觉无差异。
        // 透明底模式清屏 alpha 0（TRANSPARENT 创建旗标——透明合成透出桌面）
        let clear = if self.transparent_bg.get() {
            wgpu::Color::TRANSPARENT
        } else {
            wgpu::Color::BLACK
        };
        let ok = self
            .surface
            .borrow_mut()
            .begin_frame(clear, 1.0);
        self.begun.set(ok);
        ok
    }

    // ── 固有形态（教程代码免 trait 导入：screen.fill(..) 直接可用）──

    /// 填充（= [`DrawTarget::fill`]）
    pub fn fill(&self, color: impl Into<Color>) -> Rect {
        DrawTarget::fill(self, color)
    }

    /// blit（= [`DrawTarget::blit`]）
    pub fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect {
        DrawTarget::blit(self, src, dest)
    }

    /// 指令打包：当前 batch 编码为渲染通道，挂入本帧待提交队列
    /// （batch 空 / 帧未就绪 = 无操作）。`depth` = 本批按深度测试管线
    /// 打包（会话模式决定；同帧多会话各自打包，顺序即画家序）。
    pub(crate) fn pack(&self, depth: bool) {
        let Some(view) = self.surface.borrow().get_current_color_texture_view() else {
            self.batch.borrow_mut().clear();
            return;
        };
        let depth_att = if depth {
            // base 帧清屏已把深度清为 1.0——这里恒 Load 承接
            self.surface
                .borrow()
                .get_current_depth_texture_view()
                .map(|view| PassDepth { view, clear: false })
        } else {
            None
        };
        let scissor = self.clip.get().and_then(|r| {
            super::batch::scissor_from(r, self.surface.borrow().size())
        });
        let cmd = {
            let batch = self.batch.borrow();
            let camera = self.camera.borrow();
            let mut ctx = EncodeCtx {
                gpu: &self.gpu,
                camera: &camera,
                colors: vec![PassColor {
                    view,
                    format: self.format,
                }],
                depth: depth_att,
                samples: self.samples,
                scissor,
                white_bind: &self.gpu.white_bind,
            };
            encode(&batch, &mut ctx)
        };
        if let Some(cmd) = cmd {
            // 挂入 pending（与 base 清屏命令统一提交，保证 clear→draw 序）；
            // 顶点数据已内联一次性 buffer（batch.rs），多 pack 互不覆盖
            self.surface.borrow_mut().submit_single(cmd);
        }
        self.batch.borrow_mut().clear();
    }
}

impl DrawTarget for Screen {
    fn fill(&self, color: impl Into<Color>) -> Rect {
        let (w, h) = self.size();
        self.with_batch(|b| {
            b.push_quad(0.0, 0.0, w as f32, h as f32, rgba(color), None);
        });
        Rect::new(0, 0, w as i32, h as i32)
    }

    fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect {
        let (x, y) = dest.into();
        let (sw, sh) = src.size32();
        src.flush();
        self.with_batch(|b| {
            b.push_quad(x as f32, y as f32, sw, sh, src.blit_tint(), Some(&src.texture));
        });
        Rect::new(x, y, sw as i32, sh as i32)
    }

    fn size(&self) -> (u32, u32) {
        self.surface.borrow().size()
    }

    fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        // 显式走 inherent 方法（含 ensure_begun 的首帧自动 begin）
        Screen::with_batch(self, f);
    }
}

// ── 渲染通道会话（Rust 版 with 语法，ADR-5 v1.2）────────────────
//
// **统一类型 `RenderPass`**——与 base 的 `begin_render_pass →
// RenderPass → end()` 同一设计语言；三形态一类型：
// `screen.render()/render_depth()` · `surface.render()/render_depth()` ·
// `render_targets(&[&a, &b])`。
//
// 创建 = 开始打包；`end()` / Drop = 完成打包（编码挂入提交路径，
// flip/flush 上屏）。漏写 end 也安全——Drop 兜底同路径（Python
// `with ... :` 的 __exit__ 对应物；base RenderPass::end 的按值消费
// 同一设计语言的延续）。DrawTarget 全量实现——draw::* 直接吃会话。
//
// Python 绑定映射（定约）：`with screen:` → `__enter__`=render() /
// `__exit__`=finish(&self)（幂等，句柄持有无碍）——**返回对象即本类型**。

/// 渲染通道会话（`with` 的返回对象；三形态统一）
pub struct RenderPass<'a> {
    kind: PassKind<'a>,
    depth: bool,
}

enum PassKind<'a> {
    Screen(&'a Screen),
    Surface(&'a Surface),
    Mrt {
        gpu: Arc<Gpu>,
        targets: Vec<&'a Surface>,
        camera: RefCell<CameraRig>,
        batch: RefCell<Batch>,
    },
}

impl<'a> RenderPass<'a> {
    fn screen(target: &'a Screen, depth: bool) -> Self {
        Self { kind: PassKind::Screen(target), depth }
    }

    fn surface(target: &'a Surface, depth: bool) -> Self {
        Self { kind: PassKind::Surface(target), depth }
    }

    /// 显式完成指令打包（按值消费——结束后不可再画，类型级保证）
    pub fn end(self) {
        self.finish();
    }

    /// 幂等打包口（**Python `__exit__` 的绑定形态**；重复调用无副作用）
    pub fn finish(&self) {
        match &self.kind {
            PassKind::Screen(s) => s.pack(self.depth),
            PassKind::Surface(s) => s.pack(self.depth),
            PassKind::Mrt { .. } => self.pack_mrt(),
        }
    }

    fn pack_mrt(&self) {
        let PassKind::Mrt { gpu, targets, camera, batch } = &self.kind else {
            return;
        };
        let cmd = {
            let b = batch.borrow();
            if b.is_empty() {
                return;
            }
            let cam = camera.borrow();
            let mut ctx = EncodeCtx {
                gpu,
                camera: &cam,
                colors: targets
                    .iter()
                    .map(|t| PassColor {
                        view: t.view.clone(),
                        format: wgpu::TextureFormat::Rgba8Unorm,
                    })
                    .collect(),
                depth: None,
                samples: 1,
                scissor: None,
                white_bind: &gpu.white_bind,
            };
            encode(&b, &mut ctx)
        };
        if let Some(cmd) = cmd {
            gpu.queue.submit([cmd]);
        }
        batch.borrow_mut().clear();
    }

    /// 替换相机（会话内后续绘制生效）
    pub fn set_camera(&self, camera: &Camera) {
        match &self.kind {
            PassKind::Screen(s) => s.set_camera(camera),
            PassKind::Surface(s) => s.set_camera(camera),
            PassKind::Mrt { camera: rig, .. } => rig.borrow_mut().set(camera),
        }
    }

    /// 目标尺寸
    pub fn size(&self) -> (u32, u32) {
        match &self.kind {
            PassKind::Screen(s) => s.size(),
            PassKind::Surface(s) => s.size(),
            PassKind::Mrt { targets, .. } => targets[0].size(),
        }
    }

    // ── 固有形态（教程代码免 trait 导入）──

    /// 填充（= [`DrawTarget::fill`]）
    pub fn fill(&self, color: impl Into<Color>) -> Rect {
        DrawTarget::fill(self, color)
    }

    /// blit（= [`DrawTarget::blit`]）
    pub fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect {
        DrawTarget::blit(self, src, dest)
    }
}

impl Drop for RenderPass<'_> {
    fn drop(&mut self) {
        self.finish();
    }
}

impl DrawTarget for RenderPass<'_> {
    fn fill(&self, color: impl Into<Color>) -> Rect {
        match &self.kind {
            PassKind::Screen(s) => s.fill(color),
            PassKind::Surface(s) => s.fill(color),
            PassKind::Mrt { .. } => {
                // MRT：全部绘制走会话自己的 batch（一次 pass 写所有目标）——
                // 不可绕道各 Surface 的 fill（各自 batch 会被后续 blit 的
                // 自动 flush 二次提交，覆盖 MRT pass 输出，批次十五实测）
                let (w, h) = (self.size().0 as f32, self.size().1 as f32);
                self.with_batch(|b| b.push_quad(0.0, 0.0, w, h, rgba(color), None));
                Rect::new(0, 0, self.size().0 as i32, self.size().1 as i32)
            }
        }
    }
    fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect {
        let d: Coord = dest.into();
        match &self.kind {
            PassKind::Screen(s) => s.blit(src, d),
            PassKind::Surface(s) => s.blit(src, d),
            PassKind::Mrt { .. } => {
                src.flush();
                let (sw, sh) = src.size32();
                self.with_batch(|b| {
                    b.push_quad(d.0 as f32, d.1 as f32, sw, sh, src.blit_tint(), Some(&src.texture));
                });
                Rect::new(d.0, d.1, sw as i32, sh as i32)
            }
        }
    }
    fn size(&self) -> (u32, u32) {
        match &self.kind {
            PassKind::Screen(s) => s.size(),
            PassKind::Surface(s) => s.size(),
            PassKind::Mrt { targets, .. } => targets[0].size(),
        }
    }
    fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        match &self.kind {
            PassKind::Screen(s) => s.with_batch(f),
            PassKind::Surface(s) => s.with_batch(f),
            PassKind::Mrt { batch, .. } => f(&mut batch.borrow_mut()),
        }
    }
}

// ── Surface（纹理背板，用户自由创建；可选深度）──────────────────

/// 离屏绘制面（RenderTarget + Texture 封装，契约 ADR-1）
///
/// 透明底创建（契约 §七）；可整只 blit 到别处（纹理采样）；
/// `with_depth` 附带深度缓冲（3D 内容 / 深度会话）。
pub struct Surface {
    gpu: Arc<Gpu>,
    pub(crate) texture: Texture,
    view: Arc<wgpu::TextureView>,
    depth: Option<(Arc<wgpu::Texture>, Arc<wgpu::TextureView>)>,
    size: (u32, u32),
    batch: RefCell<Batch>,
    camera: RefCell<CameraRig>,
    /// 裁剪区（`set_clip`；None = 不裁剪）
    clip: Cell<Option<Rect>>,
    /// 表面级 alpha（`set_alpha`；255 = 不透明。blit 时乘进顶点色
    /// alpha——着色器纹理×顶点色按分量相乘，pygame blit 语义）
    alpha: Cell<u8>,
}

impl Surface {
    /// 透明底离屏面（须先建立 GPU 槽——`Screen::new` / P3 `set_mode`）
    pub fn new(size: (u32, u32)) -> Self {
        Self::build(size, false)
    }

    /// 透明底离屏面 + 深度缓冲（Depth24Plus；[`Self::render_depth`] 用）
    pub fn with_depth(size: (u32, u32)) -> Self {
        Self::build(size, true)
    }

    /// 从 RGBA8 像素构造
    pub fn from_rgba8(size: (u32, u32), pixels: &[u8]) -> Self {
        Self::from_texture(Texture::from_rgba8(size, pixels))
    }

    /// 从已有纹理构造（零拷贝；背板即该纹理，无深度）
    pub fn from_texture(texture: Texture) -> Self {
        let gpu = gpu().clone();
        let size = texture.size();
        let view = texture.view();
        let camera = RefCell::new(CameraRig::ortho(&gpu, size));
        Self {
            gpu,
            texture,
            view,
            depth: None,
            size,
            batch: RefCell::new(Batch::new()),
            camera,
            clip: Cell::new(None),
            alpha: Cell::new(255),
        }
    }

    /// 表面级 alpha（0..255；blit 时乘进顶点色——pygame set_alpha 语义）
    pub fn alpha(&self) -> u8 {
        self.alpha.get()
    }

    /// 设置表面级 alpha（下一个 blit 起生效）
    pub fn set_alpha(&self, value: u8) {
        self.alpha.set(value);
    }

    /// blit 顶点色（白 × 表面 alpha）
    pub(crate) fn blit_tint(&self) -> [f32; 4] {
        [1.0, 1.0, 1.0, self.alpha.get() as f32 / 255.0]
    }

    fn build(size: (u32, u32), depth: bool) -> Self {
        let (w, h) = (size.0.max(1), size.1.max(1));
        let mut this = Self::from_rgba8((w, h), &vec![0u8; (w * h * 4) as usize]);
        if depth {
            let depth_tex = this.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pygame_surface_depth"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: super::material::DEPTH_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let view = Arc::new(depth_tex.create_view(&Default::default()));
            this.depth = Some((Arc::new(depth_tex), view));
        }
        this
    }

    /// 背板纹理（blit 源的本体；可继续 `from_texture` 回环）
    pub fn texture(&self) -> &Texture {
        &self.texture
    }

    /// 是否带深度缓冲
    pub fn has_depth(&self) -> bool {
        self.depth.is_some()
    }

    /// 渲染会话（2D：无深度，画家算法按提交序）
    pub fn render(&self) -> RenderPass<'_> {
        RenderPass::surface(self, false)
    }

    /// 深度会话（构造须 `with_depth`；每次打包清深度）
    pub fn render_depth(&self) -> RenderPass<'_> {
        assert!(
            self.depth.is_some(),
            "render_depth: Surface 无深度缓冲——用 Surface::with_depth 创建"
        );
        RenderPass::surface(self, true)
    }

    /// 替换相机（正交默认；透视/位姿供深度会话与 3D 内容）
    pub fn set_camera(&self, camera: &Camera) {
        self.camera.borrow_mut().set(camera);
    }

    /// 裁剪区（scissor；作用于本面全部绘制，None = 不裁剪）
    pub fn set_clip(&self, clip: Option<Rect>) {
        self.clip.set(clip);
    }

    /// 当前裁剪区
    pub fn get_clip(&self) -> Option<Rect> {
        self.clip.get()
    }

    /// 显式提交点（ADR-5；通常无需调用——blit 源自动触发）
    pub fn flush(&self) {
        self.pack(self.has_depth());
    }

    /// 指令打包（depth = 本批按深度测试管线编码，pass 附深度附件并清深度）
    pub(crate) fn pack(&self, depth: bool) {
        let cmd = {
            let batch = self.batch.borrow();
            if batch.is_empty() {
                return;
            }
            let camera = self.camera.borrow();
            let depth_att = if depth {
                self.depth.as_ref().map(|(_, view)| PassDepth {
                    view: view.clone(),
                    clear: true, // 每次提交独立：深度随本 pass 清 1.0
                })
            } else {
                None
            };
            let mut ctx = EncodeCtx {
                gpu: &self.gpu,
                camera: &camera,
                colors: vec![PassColor {
                    view: self.view.clone(),
                    format: wgpu::TextureFormat::Rgba8Unorm, // ADR-3 离屏格式
                }],
                depth: depth_att,
                samples: 1,
                scissor: self
                    .clip
                    .get()
                    .and_then(|r| super::batch::scissor_from(r, self.size)),
                white_bind: &self.gpu.white_bind,
            };
            encode(&batch, &mut ctx)
        };
        if let Some(cmd) = cmd {
            self.gpu.queue.submit([cmd]);
        }
        self.batch.borrow_mut().clear();
    }

    /// batch 级绘制入口（同 [`Screen::with_batch`]，无 begin 概念）
    pub fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        f(&mut self.batch.borrow_mut());
    }

    // ── 固有形态（教程代码免 trait 导入）──

    /// 填充（= [`DrawTarget::fill`]）
    pub fn fill(&self, color: impl Into<Color>) -> Rect {
        DrawTarget::fill(self, color)
    }

    /// blit（= [`DrawTarget::blit`]）
    pub fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect {
        DrawTarget::blit(self, src, dest)
    }

    pub(crate) fn size32(&self) -> (f32, f32) {
        (self.size.0 as f32, self.size.1 as f32)
    }

    fn assert_not_self(&self, src: &Surface) {
        if Arc::as_ptr(&self.texture.raw) == Arc::as_ptr(&src.texture.raw) {
            panic!("Surface 不能 blit 到自身（同 pass 采样自绘目标为 wgpu 校验违例）");
        }
    }
}

impl DrawTarget for Surface {
    fn fill(&self, color: impl Into<Color>) -> Rect {
        let (w, h) = self.size32();
        self.with_batch(|b| b.push_quad(0.0, 0.0, w, h, rgba(color), None));
        Rect::new(0, 0, self.size.0 as i32, self.size.1 as i32)
    }

    fn blit(&self, src: &Surface, dest: impl Into<Coord>) -> Rect {
        self.assert_not_self(src);
        let (x, y) = dest.into();
        let (sw, sh) = src.size32();
        src.flush();
        self.with_batch(|b| {
            b.push_quad(x as f32, y as f32, sw, sh, src.blit_tint(), Some(&src.texture));
        });
        Rect::new(x, y, sw as i32, sh as i32)
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        Surface::with_batch(self, f);
    }
}

// ── MRT（多渲染目标会话）────────────────────────────────────────

/// 多渲染目标会话（N≥2 个 Surface 一次 pass 同时写入；v1 = 镜像双写
/// 着色器、无深度——异构输出/深度 MRT 随 v2 自定义材质开放）。
///
/// Python 绑定形态（兼容层模块架构 §3.1 预言）：`with pygame.targets([a, b]):`
pub fn render_targets<'a>(targets: &[&'a Surface]) -> RenderPass<'a> {
    assert!(targets.len() >= 2, "render_targets: 至少 2 个颜色目标");
    let size = targets[0].size();
    assert!(
        targets.iter().all(|t| t.size() == size),
        "render_targets: 全部目标尺寸须一致"
    );
    let gpu = gpu().clone();
    let camera = RefCell::new(CameraRig::ortho(&gpu, size));
    RenderPass {
        depth: false,
        kind: PassKind::Mrt {
            gpu,
            targets: targets.to_vec(),
            camera,
            batch: RefCell::new(Batch::new()),
        },
    }
}
