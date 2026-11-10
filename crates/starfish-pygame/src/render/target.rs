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

use starfish_render::bind_group::bind_group::BindGroup;
use starfish_render::render_entry::{RenderContextError, RenderEntry};
use starfish_render::render_pass::attachments::{ColorAttachment, DepthAttachment};
use starfish_render::render_pass::render_pass::RenderPass as SfRenderPass;
use starfish_render::render_resource_access::RenderResourceAccess;
use starfish_render::render_surface::RenderSurface;
use starfish_render::settings::{GpuSettings, SurfaceSettings};
use starfish_we::Window;
use crate::{Color, IRect};

use super::batch::{encode, Batch, BatchTopology, EncodeCtx, GeometryKind, SpriteVertex};
use super::camera::Camera;
use crate::camera::Camera as GameCamera;
use super::texture::Texture;
use super::{init_gpu, gpu, Coord, Gpu};
use starfish_gfx::{Geometry, ShapeVertex};

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
    /// 填充整个目标（恒用 Auto 兜底相机——盖满语义不受会话相机影响）
    fn fill(&self, color: impl Into<Color>);
    /// 画一张离屏面（渲染统一 draw 面的 texture 通道）：源面 quad
    /// (0,0)-(sw,sh) 按 **会话 transform**（`set_transform` 的 cur_model）
    /// 变换放置——2D 仿射（平移/旋转/缩放/镜像）与 3D 空间放置（四角
    /// 带 z，深度会话下正确遮挡）同一口；相机 = 会话 cur_mvp（段）。
    /// 源自动 flush。
    ///
    /// 默认管线 cull=none——镜像矩阵（det<0）绕序天然安全。
    fn draw_surface(&self, src: &Surface) {
        src.flush();
        let (hw, hh) = (src.size32().0 * 0.5, src.size32().1 * 0.5);
        self.with_batch(|b| {
            b.push_quad_corners_3d(
                [
                    [-hw, -hh, 0.0],
                    [hw, -hh, 0.0],
                    [hw, hh, 0.0],
                    [-hw, hh, 0.0],
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                src.blit_tint(),
                Some(&src.texture),
            );
        });
    }
    /// 中心锚画面（便利口：quad 中心对位 `center`——旋转帧等"中心对位"
    /// 场景的正典；仍按会话 transform 复合变换）
    fn draw_surface_centered(&self, src: &Surface, center: (f32, f32)) {
        // 中心锚后 draw_surface 本身就是中心对位的——只需额外平移到 center
        src.flush();
        let (hw, hh) = (src.size32().0 * 0.5, src.size32().1 * 0.5);
        let (cx, cy) = center;
        self.with_batch(|b| {
            b.push_quad_corners_3d(
                [
                    [cx - hw, cy - hh, 0.0],
                    [cx + hw, cy - hh, 0.0],
                    [cx + hw, cy + hh, 0.0],
                    [cx - hw, cy + hh, 0.0],
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                src.blit_tint(),
                Some(&src.texture),
            );
        });
    }
    /// 子区域画面（area 语义）：src 的 uv 子矩形贴为 `aw×ah` quad
    ///（aw/ah = 子区域像素尺寸），按会话 transform 放置。源自动 flush。
    fn draw_surface_uv(&self, src: &Surface, uv: [f32; 4], tint: impl Into<Color>) {
        src.flush();
        let (sw, sh) = src.size32();
        let (aw, ah) = ((uv[2] - uv[0]) * sw, (uv[3] - uv[1]) * sh);
        let (hw, hh) = (aw * 0.5, ah * 0.5);
        let mut color = src.blit_tint();
        let t = rgba(tint);
        color = [color[0] * t[0], color[1] * t[1], color[2] * t[2], color[3] * t[3]];
        self.with_batch(|b| {
            b.push_quad_corners_3d(
                [
                    [-hw, -hh, 0.0],
                    [hw, -hh, 0.0],
                    [hw, hh, 0.0],
                    [-hw, hh, 0.0],
                ],
                [[uv[0], uv[1]], [uv[2], uv[1]], [uv[2], uv[3]], [uv[0], uv[3]]],
                color,
                Some(&src.texture),
            );
        });
    }
    /// 显式矩阵画面（**非会话状态机形态**——矩阵全权承载放置，不复合
    /// 会话 transform；绑定层 PySurface 离屏对离屏贴图用）。源自动 flush。
    fn draw_surface_tf(&self, src: &Surface, mat: Mat4, tint: Option<[f32; 4]>) {
        src.flush();
        let (sw, sh) = src.size32();
        let (hw, hh) = (sw * 0.5, sh * 0.5);
        let mut color = src.blit_tint();
        if let Some(t) = tint {
            color = [color[0] * t[0], color[1] * t[1], color[2] * t[2], color[3] * t[3]];
        }
        let corner = |x: f32, y: f32| {
            let p = mat.transform_point3(glam::Vec3::new(x, y, 0.0));
            [p.x, p.y, p.z]
        };
        self.with_batch(|b| {
            b.push_quad_corners_3d(
                [corner(-hw, -hh), corner(hw, -hh), corner(hw, hh), corner(-hw, hh)],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                color,
                Some(&src.texture),
            );
        });
    }
    /// 任意带贴图三角网格（通用 draw 面——外部动画运行时适配层
    /// spine/Live2D/ozz 蒙皮等的消费形态）：positions/uvs 扁平 f32
    /// （uv = 纹理域直传——翻转语义由调用方自管），indices 三角索引按
    /// 3 展开为顶点三元组（z 恒 0——2D batch 域）；纹理 = 另一 Surface
    /// （其 CPU 脏区由本方法 flush）。
    fn draw_mesh(
        &self,
        texture: &Surface,
        positions: &[f32],
        uvs: &[f32],
        indices: &[u16],
        color: [f32; 4],
    ) {
        texture.flush();
        let mut verts: Vec<SpriteVertex> = Vec::with_capacity(indices.len());
        for tri in indices.chunks_exact(3) {
            for &i in tri {
                let vi = i as usize * 2;
                verts.push(SpriteVertex {
                    pos: [positions[vi], positions[vi + 1], 0.0],
                    uv: [uvs[vi], uvs[vi + 1]],
                    color,
                });
            }
        }
        self.with_batch(|b| {
            b.push_vertices(BatchTopology::Triangles, Some(texture.texture()), &verts);
        });
    }

    /// 点集直入 batch（topo 任选；z=0、uv 取中心、统一色）——过程式
    /// 几何的快速通道
    fn push_points(
        &self,
        topo: BatchTopology,
        texture: Option<&Texture>,
        points: &[(f32, f32)],
        color: impl Into<Color>,
    ) {
        let c = rgba(color);
        let verts: Vec<SpriteVertex> = points
            .iter()
            .map(|(x, y)| SpriteVertex {
                pos: [*x, *y, 0.0],
                uv: [0.5, 0.5],
                color: c,
            })
            .collect();
        self.with_batch(|b| {
            b.push_vertices(topo, texture, &verts);
        });
    }
    /// 索引点集直入 batch（kind 任选；push_geometry 内做 GLES 兼容的
    /// 退化四边形展开；indices 空 = 逐 3 顶点一切三角）
    fn push_points_indexed(
        &self,
        kind: GeometryKind,
        points: &[(f32, f32)],
        indices: &[u16],
        color: impl Into<Color>,
    ) {
        let c = rgba(color);
        let verts: Vec<ShapeVertex> = points
            .iter()
            .map(|(x, y)| ShapeVertex::new([*x, *y, 0.0], c))
            .collect();
        let geom = Geometry::indexed(verts, indices.to_vec());
        self.with_batch(|b| b.push_geometry(kind, &geom, [0.0, 0.0], c));
    }
    /// 目标尺寸（像素）
    fn size(&self) -> (u32, u32);
    /// batch 级绘制入口（`draw::*` / 自定义图元的执行通道；
    /// Screen 实现内含首帧自动 begin）
    fn with_batch(&self, f: impl FnOnce(&mut Batch));

    /// 当前会话变换矩阵（调用方组合用：`model() × local` 的便捷入口；
    /// 默认 = 恒等——各实现转发 batch 状态）
    fn model(&self) -> Mat4 {
        Mat4::IDENTITY
    }

    /// 会话相机切换（渲染统一 draw 面——per-draw 相机段；默认转发 batch）
    fn set_camera(&self, camera: &Camera) {
        self.with_batch(|b| b.set_mvp(Some(camera.mvp())));
    }

    /// 回 Auto 兜底相机
    fn reset_camera(&self) {
        self.with_batch(|b| b.reset_mvp());
    }

    /// 会话变换（后续绘制顶点 CPU 烘此矩阵）
    fn set_transform(&self, model: Mat4) {
        self.with_batch(|b| b.set_model(model));
    }

    /// 回恒等变换
    fn reset_transform(&self) {
        self.with_batch(|b| b.reset_model());
    }

    /// 当前 gfxdraw 绘制色（one-shot 状态；默认紫色）
    fn current_draw_color(&self) -> [f32; 4] {
        let mut out = super::batch::DEFAULT_DRAW_COLOR;
        self.with_batch(|b| out = b.cur_color);
        out
    }

    /// 设置 gfxdraw 绘制色（one-shot——下一次图元 draw 用此色，随后重置回紫色）
    fn set_draw_color(&self, color: [f32; 4]) {
        self.with_batch(|b| b.set_color(color));
    }

    /// 设置线框模式（one-shot——下一次图元 draw 渲染为三角网格边线）
    fn set_draw_wireframe(&self, enabled: bool) {
        self.with_batch(|b| b.set_wireframe(enabled));
    }
}

/// 相机 rig：uniform 缓冲 + bind group + 当前 MVP（**全局唯一**——
/// 挂 Gpu 槽；mvp 由 pack 按全局 CameraState 现写）
pub(crate) struct CameraRig {
    pub buf: Arc<wgpu::Buffer>,
    pub bind: BindGroup,
    pub mvp: Mat4,
}

impl CameraRig {
    /// 裸构造（uniform + bind；mvp 占位 identity——pack 时经
    /// [`Gpu::camera_for`] 按全局 CameraState 现写）
    pub fn raw(device: &Arc<wgpu::Device>, access: &RenderResourceAccess) -> Self {
        let buf = access.create_raw_buffer(
            Some("pygame_camera"),
            64,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let bind = access
            .bind_group_builder()
            .uniform_raw(0, buf.clone(), 64)
            .build(Some("pygame_camera_bind"));
        Self {
            buf,
            bind,
            mvp: Mat4::IDENTITY,
        }
    }
}

// ── Screen（交换链背板，进程唯一）────────────────────────────────

/// 屏幕绘制终点（P3 `display::set_mode` 的产物；探针可直接构造）
pub struct Screen {
    gpu: Arc<Gpu>,
    surface: RefCell<RenderSurface>,
    /// 本帧是否已 begin（交换链纹理已 acquire）
    begun: Cell<bool>,
    /// 帧进行中收到的 resize 请求（帧边界延迟应用——帧中重配置会因
    /// "深度新/颜色旧"附件尺寸不匹配导致命令缓冲无效，批次二十三实测）
    pending_resize: Cell<Option<(u32, u32)>>,
    batch: RefCell<Batch>,
    /// 裁剪区（`set_clip`；None = 不裁剪）
    clip: Cell<Option<IRect>>,
    format: wgpu::TextureFormat,
    samples: u32,
    /// 透明底模式（TRANSPARENT 创建旗标；begin_frame 清屏改 alpha 0
    ///——桌宠/悬浮件形态，配表面 alpha 合成）
    transparent_bg: Cell<bool>,
    /// 逻辑画布尺寸（脚本 set_mode 语义尺寸——Android 嵌入下物理表面
    /// 恒原生窗口、逻辑独立；呈现 = 等比缩放居中）
    logical: Cell<(u32, u32)>,
    /// 显式相机（阶段一 1.2：letterbox 内聚——MVP/坐标映射/裁剪全在此）
    pub(crate) camera: RefCell<GameCamera>,
    /// 首包映射参数打点哨兵（呈现排障；一次性）
    pack_logged: Cell<bool>,
    /// 延迟叠加队列（[`Screen::defer_overlay`] 的排程；present 时统一执行）
    pending_overlays: RefCell<Vec<Box<dyn FnOnce(&mut OverlayCtx)>>>,
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
        // 离屏 access：Rgba8Unorm（Surface/Texture 恒此格式——客人渲染
        // 器按目标选 access，见 Gpu::offscreen_access）
        let offscreen_access = Arc::new(
            context.resource_access(wgpu::TextureFormat::Rgba8Unorm, super::material::DEPTH_FORMAT),
        );
        let gpu = init_gpu(
            context.device().clone(),
            context.queue().clone(),
            Arc::new(access),
            offscreen_access,
        );
        let physical = surface.size();
        Ok(Self {
            gpu,
            surface: RefCell::new(surface),
            begun: Cell::new(false),
            pending_resize: Cell::new(None),
            batch: RefCell::new(Batch::new()),
            clip: Cell::new(None),
            format,
            samples,
            transparent_bg: Cell::new(false),
            logical: Cell::new(physical),
            camera: RefCell::new(GameCamera::ortho(physical, physical)),
            pack_logged: Cell::new(false),
            pending_overlays: RefCell::new(Vec::new()),
        })
    }

    /// 透明底模式（TRANSPARENT 创建旗标）：begin_frame 清屏改 alpha 0
    ///（桌宠/悬浮件形态；须配表面 alpha 合成——set_mode_ex 已设）
    pub(crate) fn set_transparent_bg(&self) {
        self.transparent_bg.set(true);
    }

    /// 主屏 RenderSurface 访问缝（绑定层"主屏 attachment"入口——
    /// 当前帧颜色/深度附件、格式与样本数查询经此触达；借用窗口内完成，
    /// 不得跨帧持有附件）
    pub fn with_surface<R>(&self, f: impl FnOnce(&RenderSurface) -> R) -> R {
        f(&self.surface.borrow())
    }

    /// 逻辑画布尺寸登记（set_mode 语义尺寸；Android 嵌入下 = 脚本请求
    /// 值，物理表面恒原生窗口——呈现按等比缩放居中映射）
    pub fn set_logical_size(&self, width: u32, height: u32) {
        self.logical.set((width, height));
        self.refresh_camera();
    }

    /// 重建 Camera（logical/physical/camera_kind 变化时调用——
    /// letterbox 数学的唯一来源）
    fn refresh_camera(&self) {
        let (lw, lh) = self.logical.get();
        let (pw, ph) = self.surface.borrow().size();
        let kind = *self.gpu.camera_kind.borrow();
        let cam = match kind {
            super::camera::CameraKind::Auto2d => GameCamera::ortho((lw, lh), (pw, ph)),
            super::camera::CameraKind::Auto3d { fov_deg, near, far } => {
                GameCamera::perspective(fov_deg, (lw, lh), (pw, ph), near, far)
            }
        };
        *self.camera.borrow_mut() = cam;
    }

    /// 逻辑画布尺寸（`get_size()` 的返回语义）
    pub fn logical_size(&self) -> (u32, u32) {
        self.logical.get()
    }

    /// 逻辑画布 → 物理呈现的 NDC 域拟合矩阵——**委托 Camera**（1.3
    /// letterbox 收敛：唯一计算点在 Camera 内部）
    fn fit_ndc(&self) -> Mat4 {
        self.camera.borrow().fit_ndc()
    }

    /// 呈现拟合矩阵（NDC 域 clip 空间仿射——**自管相机的 letterbox
    /// 对齐口**）：自绘管线经 write_buffer 直写的相机矩阵对引擎是不
    /// 透明字节，无法被拦截组合——脚本自行左乘本矩阵即与主绘制同一
    /// 映射：`final = present_fit() × 自管相机.mvp()`。逻辑==物理
    /// （桌面/全屏/离屏）恒等
    pub fn present_fit(&self) -> Mat4 {
        self.fit_ndc()
    }

    /// 呈现映射 MVP（逻辑 → 物理最终变换；egui 叠加合成消费——与 pack
    /// 主绘制同一映射，游戏内容与 UI 永远对齐）。Auto/Manual 相机统一
    /// 定约：相机一律解算在逻辑画布域（set_mode 语义尺寸），呈现
    /// letterbox 由 fit_ndc 统一施加——Manual 仍全权接管投影（视角/
    /// 纵横比用户定，按逻辑画布构造），仅输出被收进画布子矩形
    pub fn present_mvp(&self) -> Mat4 {
        self.fit_ndc() * crate::render::camera_mvp_for(self.logical.get())
    }

    /// 物理点 → 逻辑点（输入坐标映射；呈现映射的逆变换——等比缩放 +
    /// 居中）
    pub(crate) fn map_point_to_logical(&self, x: f64, y: f64) -> (f64, f64) {
        self.camera.borrow().physical_to_logical(x, y)
    }

    /// 窗口尺寸变化（交换链 + 深度重建；相机无需跟随——全局相机
    /// Auto 状态 pack 时按目标尺寸现算，resize 天然生效）
    pub fn resize(&self, width: u32, height: u32) {
        self.surface.borrow_mut().resize(width, height);
        self.refresh_camera();
    }

    /// 帧边界安全的 resize 请求（批次二十三）：帧进行中（begun）时挂起，
    /// present 后统一应用（一帧多次缩放取最后一次——last-wins）。
    /// 帧中直接重配置 = 深度新/颜色旧附件尺寸不匹配 → 命令缓冲无效。
    pub fn request_resize(&self, width: u32, height: u32) {
        // 批次二十四诊断
        starfish_debug::log_debug!(&format!(
            "[screen] request_resize {width}×{height} begun={}",
            self.begun.get()
        ));
        if self.begun.get() {
            self.pending_resize.set(Some((width, height)));
        } else {
            self.resize(width, height);
            starfish_debug::log_debug!("[screen] resize 已落地");
        }
    }

    /// 帧提交（对应 pygame `display.flip`）：打包残余 batch → 与 base
    /// 清屏命令合并提交 → 交换链 present；未 begin（帧未就绪）则空转
    pub fn present(&self) {
        // 批次二十四诊断： begun 状态 + batch 顶点数（呈现链路排障）
        let begun = self.begun.get();
        let batch_len = {
            let b = self.batch.borrow();
            b.len()
        };
        if !begun {
            starfish_debug::log_warn!("[screen] present 跳过（未 begin）");
            return;
        }
        self.pack(false);
        // 延迟叠加（定约见 defer_overlay）：本帧全部 pack 之后、present
        // 之前执行——登记序即画家序，恒在游戏内容之上。
        // ⚠️ 必须在 begun 复位**之前**运行：run_overlay 的 ensure_begun
        // 此时要短路（帧已 begin，不重复 acquire）——若 begun 先复位，
        // 叠加会在 present 中途触发 begin_frame 重新 acquire 交换链帧
        //（color_frame 被换成未绘制新帧 + begun=true 泄漏 → 下帧
        // ensure_begun 短路 → 绘制进已 present 的旧图 → 屏幕冻结在
        // 陈旧帧/黑屏，真机 Android 实测全症状）
        let deferred: Vec<_> = self.pending_overlays.borrow_mut().drain(..).collect();
        for f in deferred {
            self.run_overlay(f);
        }
        self.begun.set(false);
        // present 对"begin 未就绪无帧"自身免疫（跳过路径）
        self.surface.borrow_mut().present();
        self.batch.borrow_mut().clear();
        // 帧边界应用挂起的 resize（帧中收到的缩放在此安全落地）
        if let Some((w, h)) = self.pending_resize.take() {
            self.resize(w, h);
        }
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
    /// 相机经 [`super::set_default_camera`]（全局）换透视。
    pub fn render_depth(&self) -> RenderPass<'_> {
        self.ensure_begun();
        RenderPass::screen(self, true)
    }

    /// 帧缓冲取证回读（诊断专用）：7×5 网格采样最近一次成功呈现的
    /// 快照，返回 (x, y, [r,g,b,a])。letterbox 判定的客观像素证据
    ///（PrintWindow/合成器截图在 flip-model 下可能返回陈旧帧）。
    /// 数据源 = present 时刻的异步快照（RenderSurface::probe_snapshot_grid
    /// ）——Surface Texture present 后即毁，事后纹理拷贝 = DestroyedResource
    /// 毒提交（DX12 实测 2026-10-30），旧"事后拷贝"方案废弃。
    pub fn readback_probe(&self) -> Vec<(u32, u32, [u8; 4])> {
        let grid = self.surface.borrow().probe_snapshot_grid();
        if grid.is_empty() {
            starfish_debug::log_debug!(
                "[screen-probe] 无可用快照（首帧 / 映射未完成 / web 无 COPY_SRC）",
            );
        }
        grid
    }

    /// 裁剪区（scissor；作用于本目标全部绘制，None = 不裁剪）
    pub fn set_clip(&self, clip: Option<IRect>) {
        self.clip.set(clip);
    }

    /// 当前裁剪区
    pub fn get_clip(&self) -> Option<IRect> {
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

    // /// 填充（= [`DrawTarget::fill`]）
    // pub fn fill(&self, color: impl Into<Color>) {
    //     DrawTarget::fill(self, color)
    // }

    // /// blit（= [`DrawTarget::blit`]）
    // pub fn blit(&self, src: &Surface, dest: impl Into<Coord>) {
    //     DrawTarget::blit(self, src, dest)
    // }

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
                .map(|view| DepthAttachment {
                    view,
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                    stencil_ops: None,
                    depth_slice: None,
                })
        } else {
            None
        };
        // 裁剪区（pygame set_clip 语义）：脚本裁剪矩形经呈现映射（逻辑 →
        // 物理）后生效。
        // ⚠️ 画布裁剪（letterbox 下按画布子矩形裁剪的"画布外不可见"
        // 语义）**已删除**：GL/GLES 后端实测 set_scissor_rect 会使整条
        // batch pass 画不出任何内容（黑屏，A/B 实证，DX12 正常），机理
        // 待查。代价 = 越界绘制可见（会涂到黑边上），游戏侧自行剔除或
        // 覆盖；需要时从 git 历史（970faa9）恢复本段实现。
        let scissor = self.clip.get().and_then(|r| {
            let (pw, ph) = self.surface.borrow().size();
            let mapped = self.camera.borrow().map_scissor(r);
            super::batch::scissor_from(mapped, (pw, ph))
        });
        let cmd = {
            let batch = self.batch.borrow();
            let cam = self.camera.borrow();
            if !self.pack_logged.get() {
                self.pack_logged.set(true);
                let fit = cam.fit();
                let (lw, lh) = cam.logical_size();
                let (pw, ph) = cam.physical_size();
                starfish_debug::log_debug!(&format!(
                    "[present] 物理={pw}x{ph} 逻辑={lw}x{lh} scale={:.3} ox={:.1} oy={:.1} scissor={:?}",
                    fit.scale, fit.offset_x, fit.offset_y, scissor,
                ));
            }
            // 呈现映射（等比 letterbox，NDC 域统一组合）——**委托 Camera**
            // （1.3 letterbox 收敛：fit_ndc + Auto MVP 组合全在 Camera 内部）
            let cam = self.camera.borrow();
            let fit_ndc = cam.fit_ndc();
            let mut camera = self.gpu.camera_for(cam.logical_size());
            camera.mvp = cam.mvp(); // Camera::mvp = fit_ndc × projection × view
            let mut ctx = EncodeCtx {
                gpu: &self.gpu,
                camera: &camera,
                fit: fit_ndc, // 段级 set_camera 相机同一 letterbox（draw_stream 左乘）
                colors: vec![ColorAttachment {
                    view,
                    load: wgpu::LoadOp::Load, // 清屏由目标层负责（begin_frame）
                    store: wgpu::StoreOp::Store,
                    resolve_target: None,
                    depth_slice: None,
                }],
                formats: vec![self.format],
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

    // ── 叠加渲染通道（自绘内容直绘屏幕的通用出口）──────────────────
    //
    // **接口核心 = 通用能力**（2026-10-24 定约：先落引擎、再做绑定；
    // 不针对任何特定库设计）：任何"自持管线、借通道直绘"的外接渲染器
    // 都从这里落笔——本层 batch 是延迟提交，`render()` 会话期间没有活的
    // RenderPass，叠加通道在当前帧已绘内容之上开独立通道交给调用方
    // （颜色/深度附件均 LoadOp::Load，清屏归 begin_frame，本帧内多通道
    // 按提交序即画家序叠加）。次序定约：会话 pack 之后、present 之前
    // 调用 = 叠在游戏画面之上。
    //
    // 现役消费者（pygame-rs 绑定层）：egui-wgpu（UI 面板）、rapier
    // 物理调试线——仅是用例，接口面不感知它们（依赖单向红线）。

    /// 交换链颜色格式（外接渲染器管线创建期依赖）
    pub fn color_format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// 交换链 MSAA 采样数（外接渲染器管线须匹配，否则 wgpu 校验错）
    pub fn sample_count(&self) -> u32 {
        self.samples
    }

    /// 帧生命周期状态串（诊断探针——begun/color_frame/尺寸一屏可读）
    pub fn frame_state(&self) -> String {
        let surface = self.surface.borrow();
        format!(
            "begun={} frame={} {}x{}",
            self.begun.get(),
            surface.has_current_frame(),
            surface.size().0,
            surface.size().1
        )
    }

    /// 叠加渲染通道——**立即形态**（定约见上方注释块）：`f` 内经
    /// [`OverlayCtx::with_pass`] 开通道直绘、可多次；闭包返回即 finish +
    /// 提交（[`OverlayCtx::submit_before`] 收的命令先于全部绘制通道）。
    /// 未 begin（无帧）自动 begin；表面未就绪静默跳过（同 [`Self::present`]
    /// 语义）。⚠️ 次序敏感：须在游戏会话 pack 之后调用才不被反盖——
    /// 任意时点可调的排程形态用 [`Self::defer_overlay`]。
    pub fn with_overlay(&self, f: impl FnOnce(&mut OverlayCtx)) {
        self.run_overlay(f);
    }

    /// 叠加渲染通道——**延迟形态**（排程）：任意时点调用（含游戏会话内），
    /// [`Self::present`] 在本帧全部 batch pack 之后、交换链 present 之前
    /// 统一执行——多客人按登记序画家序叠加，恒在游戏内容之上（GUI 类
    /// 客人的推荐形态）。表面未就绪的帧（present 跳过）排程保留到下一帧。
    pub fn defer_overlay(&self, f: impl FnOnce(&mut OverlayCtx) + 'static) {
        self.pending_overlays.borrow_mut().push(Box::new(f));
    }

    /// 叠加执行体（with/defer 两形态共用）：组上下文 → 执行 → 收口提交
    /// （Box<dyn FnOnce> 本就实现 FnOnce，present 排程直接传入）
    fn run_overlay<F: FnOnce(&mut OverlayCtx)>(&self, f: F) {
        if !self.ensure_begun() {
            return;
        }
        let Some(color) = self.surface.borrow().get_current_color_attachment() else {
            return;
        };
        let depth = self.surface.borrow().get_current_depth_attachment();
        let mut ctx = OverlayCtx {
            encoder: self.gpu.access.create_command_encoder(),
            color: Some(color),
            depth,
            pre_pass_cmds: Vec::new(),
            device: self.gpu.device.clone(),
            queue: self.gpu.queue.clone(),
            access: self.gpu.access.clone(),
            gpu: self.gpu.clone(),
            size: self.size(),
            format: self.format,
            samples: self.samples,
        };
        f(&mut ctx);
        let mut cmds = std::mem::take(&mut ctx.pre_pass_cmds);
        cmds.push(ctx.encoder.finish());
        self.surface.borrow_mut().submit(cmds);
    }
}

/// 叠加通道上下文（[`Screen::with_overlay`] 闭包参数）：encoder 与当前帧
/// 附件/GPU 句柄由引擎收口，调用方按需开通道、写通道前命令——全程只接触
/// wgpu / starfish-render 公共类型（依赖单向红线的接口面）。
pub struct OverlayCtx {
    /// starfish 包装 encoder（`with_pass` 的宿主；收口时 finish 提交）
    encoder: starfish_render::command_encoder::CommandEncoder,
    color: Option<ColorAttachment>,
    depth: Option<DepthAttachment>,
    pre_pass_cmds: Vec<wgpu::CommandBuffer>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    access: Arc<RenderResourceAccess>,
    /// 目标规格（`blit_fullscreen` 合成用；内部字段——构造处引擎填）
    gpu: Arc<Gpu>,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    samples: u32,
}

impl OverlayCtx {
    /// 开一条叠加通道执行 `draw`（LoadOp::Load 承接已绘内容；通道借用以
    /// `&mut RenderPass` 交付调用方直绘）。`depth` = 附加深度附件（3D
    /// 内容深度测试用；本上下文的深度附件只供一条通道——第二条起为
    /// None）。**深度恒清 1.0**（不承接宿主深度）：客人叠加语义 = 恒在
    /// 游戏内容之上，3D 客人自管遮挡（LessEqual）——宿主深度恒不清的
    /// 场景（主屏 present 走 pack(false)）若 Load 承接 = wgpu 零初始化
    /// 0.0（最近平面）→ 3D 客人全被无声剔除（2026-10-24 实测：主屏 +
    /// Surface 的 3D 调试线从未显示的根因）。
    /// **深度契约按自绘管线分支**：管线深度关闭者（如 2D 线
    /// 管线）必须传 `false`——wgpu 对管线深度目标与通道附件做严格
    /// 相等校验。
    pub fn with_pass(
        &mut self,
        label: &str,
        depth: bool,
        draw: impl FnOnce(&mut SfRenderPass),
    ) {
        let mut depth_att = if depth { self.depth.take() } else { None };
        if let Some(d) = &mut depth_att {
            d.load = wgpu::LoadOp::Clear(1.0);
        }
        let color_refs: [&Option<ColorAttachment>; 1] = [&self.color];
        let mut pass = self
            .encoder
            .begin_render_pass(label, &color_refs, depth_att, None, None, None);
        draw(&mut pass);
        pass.end();
    }

    /// 开一条计算通道执行 `draw`（compute pipeline 直绘——Dispatch 语义；
    /// 无附件，资源经 bind group 访问；bind 按调用序自动排组位）
    pub fn with_compute_pass(
        &mut self,
        label: &str,
        draw: impl FnOnce(&mut starfish_render::render_pass::compute_pass::ComputePass),
    ) {
        let mut pass = self.encoder.begin_compute_pass(Some(label));
        draw(&mut pass);
    }

    /// 缓冲间拷贝（本帧编码内有序执行——compute 写 storage → copy 到
    /// uniform 的GPU 闭环直通口）
    pub fn copy_buffer_to_buffer(
        &mut self,
        src: &wgpu::Buffer,
        dst: &wgpu::Buffer,
        size: u64,
    ) {
        self.encoder.copy_buffer_to_buffer(src, 0, dst, 0, size);
    }

    /// 缓冲 → 2D 纹理拷贝（大 N 数据通路：storage → 采样纹理 → VS
    /// texelFetch；bytes_per_row 须 256 对齐，(copy_width, copy_height)
    /// = 本次拷贝区域）
    pub fn copy_buffer_to_texture(
        &mut self,
        src: &wgpu::Buffer,
        bytes_per_row: u32,
        dst: &wgpu::Texture,
        copy_width: u32,
        copy_height: u32,
    ) {
        self.encoder.copy_buffer_to_texture(
            src, bytes_per_row, dst, copy_width, copy_height,
        );
    }

    /// 在任意离屏面上开一条通道（**中间面客人通道**——自带管线的绘制
    /// 端（如 egui_wgpu）先画进面、再经 [`Self::blit_fullscreen`] 用引擎
    /// sprite 管线合成回目标：客人零改其管线即可吃全局相机定约）。
    /// 面恒 Rgba8 无深度（离屏画布定约）；`clear` = 本通道颜色清屏值。
    pub fn pass_on_surface(
        &mut self,
        label: &str,
        target: &Surface,
        clear: wgpu::Color,
        draw: impl FnOnce(&mut SfRenderPass),
    ) {
        let color = Some(ColorAttachment {
            view: target.view.clone(),
            load: wgpu::LoadOp::Clear(clear),
            store: wgpu::StoreOp::Store,
            resolve_target: None,
            depth_slice: None,
        });
        let color_refs: [&Option<ColorAttachment>; 1] = [&color];
        let mut pass = self
            .encoder
            .begin_render_pass(label, &color_refs, None, None, None, None);
        draw(&mut pass);
        pass.end();
    }

    /// 全屏纹理合成（**全局相机定约的客人侧落点**）：`src` 以整面 quad
    /// 经引擎 sprite 管线画到本上下文目标——MVP = [`super::camera_mvp_for`]
    ///（Auto = 按目标尺寸正交 → 像素级 1:1 全屏；`set_camera` 后全体
    /// 跟随）。中间面方案（egui 等自带管线客人）的合成段。
    /// **注意**：MVP 在**调用时点**现算——排程类客人（defer_overlay 到
    /// 收口才执行）若帧内多次 set_camera，收口时的全局相机可能已非排
    /// 程时的值；精确时序用 [`Self::blit_fullscreen_with`]（排程时快照）。
    pub fn blit_fullscreen(&mut self, src: &Surface) {
        let mvp = super::camera_mvp_for(self.size);
        self.blit_fullscreen_with(src, mvp.to_cols_array_2d());
    }

    /// 全屏纹理合成（**快照 MVP 形态**）：矩阵由调用方在排程时点解算
    /// 快照（如 `camera_mvp_for(目标尺寸)` 的产出），执行时点原样使用
    ///——"draw 时相机"的精确语义（同帧 A 相机画主屏、B 相机画离屏
    /// 各得其所）。
    pub fn blit_fullscreen_with(&mut self, src: &Surface, mvp: [[f32; 4]; 4]) {
        use wgpu::util::DeviceExt;
        let (sw, sh) = src.size32();
        let tint = src.blit_tint();
        // 与 encode() 同款一次性资源：相机 MVP / 顶点各一次内联 buffer
        //（多 pass 多 pack 互不覆盖——批次九定约）
        let cam_buf = self.gpu.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("overlay_camera_mvp"),
                contents: bytemuck::bytes_of(&mvp),
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let cam_bind = self
            .gpu
            .access
            .bind_group_builder()
            .uniform_raw(0, Arc::new(cam_buf), 64)
            .build(Some("overlay_camera_bind"));
        let v = |x: f32, y: f32, u: f32, t: f32| SpriteVertex {
            pos: [x, y, 0.0],
            uv: [u, 1.0 - t], // v 翻：egui 中间面（NDC y down 内容）合成到 y up 世界
            color: tint,
        };
        let (hw, hh) = (sw * 0.5, sh * 0.5);
        let verts = [
            v(-hw, -hh, 0.0, 0.0),
            v(hw, -hh, 1.0, 0.0),
            v(hw, hh, 1.0, 1.0),
            v(-hw, -hh, 0.0, 0.0),
            v(hw, hh, 1.0, 1.0),
            v(-hw, hh, 0.0, 1.0),
        ];
        let vbo = self.gpu.device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("overlay_compose_vbo"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            },
        );
        // 纹理 bind（缓存表 + 源面 flush 保险）
        src.flush();
        let tex_key = src.texture.cache_key();
        self.gpu.ensure_bind_view(tex_key, src.texture.view());
        // with_pass 借走 &mut self——闭包所需句柄先解构（借用纪律）
        let gpu = self.gpu.clone();
        let (format, samples) = (self.format, self.samples);
        let vbo = Arc::new(vbo);
        self.with_pass("overlay_compose", false, |pass| {
            let pipeline =
                gpu.material.pipeline(&gpu, &cam_bind, format, samples, false, false);
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &cam_bind);
            let binds = gpu.binds.borrow();
            if let Some(b) = binds.get(&tex_key) {
                pass.set_bind_group(1, b);
            }
            pass.set_vertex_buffer(0, &vbo);
            pass.draw(0..6, 0..1);
        });
    }

    /// 本上下文目标尺寸（中间面尺寸跟随用）
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// 通道前命令编码器（**原生 wgpu encoder**——绘制通道开始前须完成
    /// 的命令（资源上传 / 回调 prepare 等）记进它，`finish` 后交
    /// [`Self::submit_before`]，先于本上下文全部绘制通道执行）
    pub fn create_command_encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("overlay_pre_pass") })
    }

    /// 通道前命令提交（须先于本上下文全部绘制通道执行的命令缓冲）
    pub fn submit_before(&mut self, cmds: Vec<wgpu::CommandBuffer>) {
        self.pre_pass_cmds.extend(cmds);
    }

    /// GPU 设备（外接渲染器纹理/缓冲创建用）
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// GPU 队列（write_buffer / write_texture 即时上传）
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// 资源访问（外接渲染器创建自持管线 / bind group 用）
    pub fn access(&self) -> &RenderResourceAccess {
        &self.access
    }
}

impl DrawTarget for Screen {
    fn fill(&self, color: impl Into<Color>) {
        // fill 覆盖对象是脚本画布（set_mode 语义尺寸）——batch 顶点在
        // pack 时统一过呈现映射（正交(物理) × 平移 × 缩放），故必须
        // 用逻辑尺寸：桌面逻辑==物理恒等不受影响；Android 窗口化
        // 逻辑<物理，用物理尺寸会把 letterbox 右/下黑边一起涂成
        // 画布色（真机实测：黑边只剩左侧半条）。
        // **恒用 Auto 兜底段**（盖满语义不受会话相机影响——保存/恢复）
        let (w, h) = self.logical_size();
        self.with_batch(|b| {
            let saved = b.mvp_snapshot();
            b.reset_mvp();
            b.push_quad(-(w as f32) / 2.0, -(h as f32) / 2.0, w as f32, h as f32, rgba(color), None);
            b.set_mvp(saved);
        });
    }

    fn draw_surface(&self, src: &Surface) {
        src.flush();
        let (sw, sh) = src.size32();
        self.with_batch(|b| {
            b.push_quad_corners_3d(
                [
                    [-sw / 2.0, -sh / 2.0, 0.0],
                    [sw / 2.0, -sh / 2.0, 0.0],
                    [sw / 2.0, sh / 2.0, 0.0],
                    [-sw / 2.0, sh / 2.0, 0.0],
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                src.blit_tint(),
                Some(&src.texture),
            );
        });
    }

    fn size(&self) -> (u32, u32) {
        self.surface.borrow().size()
    }

    fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        // 显式走 inherent 方法（含 ensure_begun 的首帧自动 begin）
        Screen::with_batch(self, f);
    }

    fn model(&self) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        self.with_batch(|b| out = b.model());
        out
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
        batch: RefCell<Batch>,
    },
}

impl<'a> RenderPass<'a> {
    fn screen(target: &'a Screen, depth: bool) -> Self {
        let p = Self { kind: PassKind::Screen(target), depth };
        p.reset_draw_state();
        p
    }

    fn surface(target: &'a Surface, depth: bool) -> Self {
        let p = Self { kind: PassKind::Surface(target), depth };
        p.reset_draw_state();
        p
    }

    /// 会话开始重置绘制状态（Auto 兜底相机 + 恒等变换——新会话新状态，
    /// 不残留上一会话的 set_camera/set_transform）
    fn reset_draw_state(&self) {
        match &self.kind {
            PassKind::Screen(s) => s.with_batch(|b| {
                b.reset_mvp();
                b.reset_model();
            }),
            PassKind::Surface(s) => s.with_batch(|b| {
                b.reset_mvp();
                b.reset_model();
            }),
            PassKind::Mrt { batch, .. } => {
                let mut b = batch.borrow_mut();
                b.reset_mvp();
                b.reset_model();
            }
        }
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
        let PassKind::Mrt { gpu, targets, batch } = &self.kind else {
            return;
        };
        let cmd = {
            let b = batch.borrow();
            if b.is_empty() {
                return;
            }
            let camera = gpu.camera_for(self.size());
            let mut ctx = EncodeCtx {
                gpu,
                camera: &camera,
                fit: Mat4::IDENTITY, // MRT 渲染目标逻辑==物理
                colors: targets
                    .iter()
                    .map(|t| ColorAttachment {
                        view: t.view.clone(),
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                        resolve_target: None,
                        depth_slice: None,
                    })
                    .collect(),
                formats: vec![wgpu::TextureFormat::Rgba8Unorm; targets.len()],
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

    // ── 绘制状态机（渲染统一 draw 面：per-draw 相机/变换）──────────
    //
    // 会话级状态——每目标（Screen/Surface/MRT）独立持有，互不串线；
    // 旧全局 `set_default_camera`（CameraState::Manual）随此接口退役。

    /// 会话相机切换（后续 push 进新段；影响本会话剩余绘制——**仅本
    /// 会话/本目标**，其他会话与其他目标不受影响）
    pub fn set_camera(&self, camera: &Camera) {
        self.with_batch(|b| b.set_mvp(Some(camera.mvp())));
    }

    /// 回 Auto 兜底相机（按目标尺寸现算——resize 天然跟随）
    pub fn reset_camera(&self) {
        self.with_batch(|b| b.reset_mvp());
    }

    /// 会话变换（后续 push 的顶点 CPU 烘此矩阵——位置/旋转/缩放/3D
    /// 放置统一入口）
    pub fn set_transform(&self, model: Mat4) {
        self.with_batch(|b| b.set_model(model));
    }

    /// 回恒等变换
    pub fn reset_transform(&self) {
        self.with_batch(|b| b.reset_model());
    }

    /// 会话级延迟叠加（**通用客人直绘的会话形态**，2026-10-24——"画到
    /// 哪个目标"由会话对象表达；也是获取原生 RenderPass 的规范路径：
    /// 闭包经 [`OverlayCtx::with_pass`] 拿 `&mut RenderPass` 直绘）。
    /// 排程到会话目标的呈现时点——Screen 会话 = present 时本帧全部
    /// batch pack 之后（恒在游戏内容之上）；Surface 会话 = [`Surface::
    /// flush`] 时本面 batch pack 之后（画中画/离屏叠加通道）；MRT 会话
    /// v1 不支持（devlog 提示并忽略）。与 [`Screen::with_overlay`]
    /// （立即形态）同源执行体；泛型闭包零第三方感知（依赖单向红线）。
    pub fn defer_overlay(&self, f: impl FnOnce(&mut OverlayCtx) + 'static) {
        match &self.kind {
            PassKind::Screen(s) => s.defer_overlay(f),
            PassKind::Surface(s) => s.defer_overlay(f),
            PassKind::Mrt { .. } => {
                starfish_debug::log_warn!(
                    "[session] defer_overlay：MRT 会话暂不支持客人叠加（v1）",
                );
            }
        }
    }

    /// 会话级叠加通道——**立即形态**（镜像 [`Screen::with_overlay`]）：
    /// 闭包当场执行、encoder finish 即提交——先于本会话 batch 的 pack
    /// （调用点在绘制会话内时，客人产物**同帧**即可被本会话后续绘制
    /// 采样；live2d 蒙版中间面的采样时序前提）。
    pub fn with_overlay(&self, f: impl FnOnce(&mut OverlayCtx)) {
        match &self.kind {
            PassKind::Screen(s) => s.with_overlay(f),
            PassKind::Surface(s) => s.with_overlay(f),
            PassKind::Mrt { .. } => {
                starfish_debug::log_warn!(
                    "[session] with_overlay：MRT 会话暂不支持客人叠加（v1）",
                );
            }
        }
    }

    /// 裁剪矩形（会话内后续绘制生效；None = 解除）。
    /// MRT v1 无裁剪面（随 v2 自定义材质开放），静默忽略。
    pub fn set_clip(&self, clip: Option<IRect>) {
        match &self.kind {
            PassKind::Screen(s) => s.set_clip(clip),
            PassKind::Surface(s) => s.set_clip(clip),
            PassKind::Mrt { .. } => {}
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
    pub fn fill(&self, color: impl Into<Color>) {
        DrawTarget::fill(self, color)
    }

    /// 画离屏面（= [`DrawTarget::draw_surface`]）
    pub fn draw_surface(&self, src: &Surface) {
        DrawTarget::draw_surface(self, src)
    }

    /// 当前会话变换矩阵（调用方组合用：`model() × local` 的便捷入口）
    pub fn model(&self) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        self.with_batch(|b| out = b.model());
        out
    }
}

impl Drop for RenderPass<'_> {
    fn drop(&mut self) {
        self.finish();
    }
}

impl DrawTarget for RenderPass<'_> {
    fn fill(&self, color: impl Into<Color>) {
        match &self.kind {
            PassKind::Screen(s) => s.fill(color),
            PassKind::Surface(s) => s.fill(color),
            PassKind::Mrt { .. } => {
                // MRT：全部绘制走会话自己的 batch（一次 pass 写所有目标）——
                // 不可绕道各 Surface 的 fill（各自 batch 会被后续 draw_surface
                // 的自动 flush 二次提交，覆盖 MRT pass 输出，批次十五实测）。
                // 恒默认段（盖满语义）
                let (w, h) = (self.size().0 as f32, self.size().1 as f32);
                self.with_batch(|b| {
                    let saved = b.mvp_snapshot();
                    b.reset_mvp();
                    b.push_quad(-w / 2.0, -h / 2.0, w, h, rgba(color), None);
                    b.set_mvp(saved);
                });
            }
        }
    }
    fn draw_surface(&self, src: &Surface) {
        match &self.kind {
            PassKind::Screen(s) => s.draw_surface(src),
            PassKind::Surface(s) => s.draw_surface(src),
            PassKind::Mrt { .. } => {
                src.flush();
                let (sw, sh) = src.size32();
                self.with_batch(|b| {
                    b.push_quad_corners_3d(
                        [
                            [-sw / 2.0, -sh / 2.0, 0.0],
                            [sw / 2.0, -sh / 2.0, 0.0],
                            [sw / 2.0, sh / 2.0, 0.0],
                            [-sw / 2.0, sh / 2.0, 0.0],
                        ],
                        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                        src.blit_tint(),
                        Some(&src.texture),
                    );
                });
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

    fn model(&self) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        self.with_batch(|b| out = b.model());
        out
    }
}

// ── Surface（纹理背板，用户自由创建；可选深度）──────────────────

/// 离屏绘制面（RenderTarget + Texture 封装，契约 ADR-1）
///
/// CPU alpha 影子（每像素 1 字节；mask 的**构建期零回读**地基）
///
/// 语义 = **创建期快照**：只在像素天然过手的出生路径留存
/// （from_rgba8 / image 解码 / font 光栅化），绘制与 fill 不回写——
/// 运行期真值走 GPU 回读路径（原生同步 / web 异步）。
#[derive(Clone, Debug)]
pub(crate) struct CpuAlpha {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) data: Vec<u8>,
}

impl CpuAlpha {
    /// 从 RGBA8 像素抽取 alpha 平面（stride 4）
    pub(crate) fn from_rgba8(size: (u32, u32), rgba: &[u8]) -> Self {
        Self {
            width: size.0,
            height: size.1,
            data: rgba.chunks_exact(4).map(|px| px[3]).collect(),
        }
    }
}

/// 透明底创建（契约 §七）；可整只 blit 到别处（纹理采样）；
/// `with_depth` 附带深度缓冲（3D 内容 / 深度会话）。
pub struct Surface {
    gpu: Arc<Gpu>,
    pub(crate) texture: Texture,
    view: Arc<wgpu::TextureView>,
    depth: Option<(Arc<wgpu::Texture>, Arc<wgpu::TextureView>)>,
    size: (u32, u32),
    batch: RefCell<Batch>,
    /// 裁剪区（`set_clip`；None = 不裁剪）
    clip: Cell<Option<IRect>>,
    /// 表面级 alpha（`set_alpha`；255 = 不透明。blit 时乘进顶点色
    /// alpha——着色器纹理×顶点色按分量相乘，pygame blit 语义）
    alpha: Cell<u8>,
    /// CPU alpha 影子（CPU 出生路径留存；GPU 出生面 = None）
    cpu_alpha: Option<CpuAlpha>,
    /// 延迟叠加排程（`defer_overlay`；flush 在 batch pack 之后统一执行
    /// ——batch 空也执行，纯 UI/纯调试线的表面由此成立）
    pending_overlays: RefCell<Vec<Box<dyn FnOnce(&mut OverlayCtx)>>>,
}

impl Drop for Surface {
    /// 纹理销毁即入队摘除（2026-11-08 泄漏三段）：每帧临时纹理
    /// （font.render 等）死亡即排队出缓存。⚠️ 不立即摘——延迟提交时序
    /// 下临时面死于 flush 前，立即摘了 encode 尾 ensure_bind_view 会把
    /// 已死纹理当新键重建插回（孤儿条目 → FIFO cap 满仓平台期）；
    /// 摘除统一在 encode 尾 drain（binds 条目数 = 活纹理数不变式）。
    /// 被摘 BindGroup 的 wgpu 生命周期由命令缓冲引用保障（已编码
    /// 命令不受影响）。
    fn drop(&mut self) {
        self.gpu.queue_unbind(self.texture.cache_key());
    }
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

    /// 从 RGBA8 像素构造（**留 CPU alpha 影子**——mask 同步构建地基）
    pub fn from_rgba8(size: (u32, u32), pixels: &[u8]) -> Self {
        let mut this = Self::from_texture(Texture::from_rgba8(size, pixels));
        this.cpu_alpha = Some(CpuAlpha::from_rgba8(size, pixels));
        this
    }

    /// 从已有纹理构造（零拷贝；背板即该纹理，无深度；**GPU 出生面——
    /// 无 alpha 影子**，mask 走回读路径）
    pub fn from_texture(texture: Texture) -> Self {
        let gpu = gpu().clone();
        let size = texture.size();
        let view = texture.view();
        Self {
            gpu,
            texture,
            view,
            depth: None,
            size,
            batch: RefCell::new(Batch::new()),
            clip: Cell::new(None),
            alpha: Cell::new(255),
            cpu_alpha: None,
            pending_overlays: RefCell::new(Vec::new()),
        }
    }

    /// CPU alpha 影子（创建期快照；GPU 出生面 = None）
    pub(crate) fn cpu_alpha(&self) -> Option<&CpuAlpha> {
        self.cpu_alpha.as_ref()
    }

    /// 附加 CPU alpha 影子（image 解码等持有 RGBA 字节的路径用）
    pub(crate) fn set_cpu_alpha(&mut self, alpha: CpuAlpha) {
        self.cpu_alpha = Some(alpha);
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
    pub fn blit_tint(&self) -> [f32; 4] {
        [1.0, 1.0, 1.0, self.alpha.get() as f32 / 255.0]
    }

    fn build(size: (u32, u32), depth: bool) -> Self {
        let (w, h) = (size.0.max(1), size.1.max(1));
        // 绘制面 = **GPU 出生**（无 alpha 影子）：透明底经 from_texture
        // 构造，不附影子——绘制面的运行期真值走回读路径（若误用
        // from_rgba8 会附上全零影子，mask from_surface 将短路成空位面）
        let mut this = Self::from_texture(Texture::from_rgba8((w, h), &vec![0u8; (w * h * 4) as usize]));
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

    /// 裸 wgpu 纹理句柄（GPU 回读取证 / 外接管线直引用用——与
    /// [`Self::texture_view`] 同级的原生出口）
    pub fn raw_texture(&self) -> Arc<wgpu::Texture> {
        self.texture.raw.clone()
    }

    /// 背板纹理视图（**Rust 内部缝**——绑定层 ResourceAccess.bind_group
    /// 的 "texture" 绑定取样用；wgpu 裸对象不出 Python 界）
    pub fn texture_view(&self) -> Arc<wgpu::TextureView> {
        self.view.clone()
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

    /// 裁剪区（scissor；作用于本面全部绘制，None = 不裁剪）
    pub fn set_clip(&self, clip: Option<IRect>) {
        self.clip.set(clip);
    }

    /// 当前裁剪区
    pub fn get_clip(&self) -> Option<IRect> {
        self.clip.get()
    }

    /// 显式提交点（ADR-5；通常无需调用——blit 源自动触发）。
    /// **延迟叠加收口**：batch pack 之后统一执行 `defer_overlay` 排程
    /// （登记序即画家序，恒在本面游戏内容之上；batch 空/帧无绘制也执行
    /// ——纯 UI / 纯调试线表面由此成立）。
    pub fn flush(&self) {
        self.pack(self.has_depth());
        let deferred: Vec<_> = self.pending_overlays.borrow_mut().drain(..).collect();
        for f in deferred {
            self.run_overlay(f);
        }
    }

    /// 叠加渲染通道——**延迟形态**（Surface 版，镜像 [`Screen::defer_overlay`]）：
    /// 任意时点调用（含绘制会话内），[`Self::flush`]（含 blit 源自动触发）
    /// 在 batch pack 之后统一执行。颜色附件 = 本面纹理（Rgba8Unorm 离屏，
    /// Load 承接已绘内容）；深度附件 = `with_depth` 表面自持深度（Load 承接
    /// 本面 pack 清后的深度——3D 客人管线 depth=true 用）。
    pub fn defer_overlay(&self, f: impl FnOnce(&mut OverlayCtx) + 'static) {
        self.pending_overlays.borrow_mut().push(Box::new(f));
    }

    /// 叠加渲染通道——**立即形态**（Surface 版，镜像 [`Screen::
    /// with_overlay`]）：闭包当场执行、encoder finish 即提交（先于本面
    /// batch 的 pack——绘制会话内调用时客人产物同帧可采样）。
    pub fn with_overlay(&self, f: impl FnOnce(&mut OverlayCtx)) {
        self.run_overlay(f);
    }

    /// 叠加执行体（镜像 [`Screen::run_overlay`]；无 begin/present 概念
    /// ——encoder finish 后直接 queue 提交）
    fn run_overlay(&self, f: impl FnOnce(&mut OverlayCtx)) {
        let color = Some(ColorAttachment {
            view: self.view.clone(),
            resolve_target: None,
            load: wgpu::LoadOp::Load,
            store: wgpu::StoreOp::Store,
            depth_slice: None,
        });
        let depth = self.depth.as_ref().map(|(_, view)| {
            starfish_render::render_pass::attachments::DepthAttachment {
                view: view.clone(),
                load: wgpu::LoadOp::Load, // Load 承接本面 pack 清后的深度
                store: wgpu::StoreOp::Store,
                stencil_ops: None,
                depth_slice: None,
            }
        });
        let mut ctx = OverlayCtx {
            encoder: self.gpu.access.create_command_encoder(),
            color,
            depth,
            pre_pass_cmds: Vec::new(),
            device: self.gpu.device.clone(),
            queue: self.gpu.queue.clone(),
            access: self.gpu.access.clone(),
            gpu: self.gpu.clone(),
            size: self.size,
            format: wgpu::TextureFormat::Rgba8Unorm, // ADR-3 离屏格式
            samples: 1,
        };
        f(&mut ctx);
        let mut cmds = std::mem::take(&mut ctx.pre_pass_cmds);
        cmds.push(ctx.encoder.finish());
        self.gpu.queue.submit(cmds);
    }

    /// 指令打包（depth = 本批按深度测试管线编码，pass 附深度附件并清深度）
    pub(crate) fn pack(&self, depth: bool) {
        let cmd = {
            let batch = self.batch.borrow();
            if batch.is_empty() {
                return;
            }
            let camera = self.gpu.camera_for(self.size);
            let depth_att = if depth {
                self.depth.as_ref().map(|(_, view)| DepthAttachment {
                    view: view.clone(),
                    load: wgpu::LoadOp::Clear(1.0), // 每次提交独立：深度随本 pass 清 1.0
                    store: wgpu::StoreOp::Store,
                    stencil_ops: None,
                    depth_slice: None,
                })
            } else {
                None
            };
            let mut ctx = EncodeCtx {
                gpu: &self.gpu,
                camera: &camera,
                fit: Mat4::IDENTITY, // 离屏 Surface 逻辑==物理
                colors: vec![ColorAttachment {
                    view: self.view.clone(),
                    load: wgpu::LoadOp::Load, // 承接已绘内容（透明底画布）
                    store: wgpu::StoreOp::Store,
                    resolve_target: None,
                    depth_slice: None,
                }],
                formats: vec![wgpu::TextureFormat::Rgba8Unorm], // ADR-3 离屏格式
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

    /// 当前变换矩阵（调用方组合用：`model() × local` 的便捷入口）
    pub fn model(&self) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        self.with_batch(|b| out = b.model());
        out
    }

    // ── 固有形态（教程代码免 trait 导入）──

    // /// 填充（= [`DrawTarget::fill`]）
    // pub fn fill(&self, color: impl Into<Color>) {
    //     DrawTarget::fill(self, color)
    // }

    // /// blit（= [`DrawTarget::blit`]）
    // pub fn blit(&self, src: &Surface, dest: impl Into<Coord>) {
    //     DrawTarget::blit(self, src, dest)
    // }

    pub fn size32(&self) -> (f32, f32) {
        (self.size.0 as f32, self.size.1 as f32)
    }

    fn assert_not_self(&self, src: &Surface) {
        if Arc::as_ptr(&self.texture.raw) == Arc::as_ptr(&src.texture.raw) {
            panic!("Surface 不能 blit 到自身（同 pass 采样自绘目标为 wgpu 校验违例）");
        }
    }
}

impl DrawTarget for Surface {
    fn fill(&self, color: impl Into<Color>) {
        let (w, h) = self.size32();
        self.with_batch(|b| {
            let saved = b.mvp_snapshot();
            b.reset_mvp();
            b.push_quad(-w / 2.0, -h / 2.0, w, h, rgba(color), None);
            b.set_mvp(saved);
        });
    }

    fn draw_surface(&self, src: &Surface) {
        self.assert_not_self(src);
        src.flush();
        let (sw, sh) = src.size32();
        self.with_batch(|b| {
            b.push_quad_corners_3d(
                [
                    [-sw / 2.0, -sh / 2.0, 0.0],
                    [sw / 2.0, -sh / 2.0, 0.0],
                    [sw / 2.0, sh / 2.0, 0.0],
                    [-sw / 2.0, sh / 2.0, 0.0],
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                src.blit_tint(),
                Some(&src.texture),
            );
        });
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn with_batch(&self, f: impl FnOnce(&mut Batch)) {
        Surface::with_batch(self, f);
    }

    fn model(&self) -> Mat4 {
        let mut out = Mat4::IDENTITY;
        self.with_batch(|b| out = b.model());
        out
    }
}

// ── MRT（多渲染目标会话）────────────────────────────────────────

/// 多渲染目标会话（N≥2 个 Surface 一次 pass 同时写入；v1 = 镜像双写
/// 着色器、无深度——异构输出/深度 MRT 随 v2 自定义材质开放）。
///
/// Arc 目标版 render_targets（thread-local 驻留形态）：`RenderPass`
/// 'static 化——目标 `Arc` 由 [`super::session`] 的 TARGETS_KEEPALIVE
/// 保活至 `end_targets()`（begin_targets/end_targets 成对使用即兑现
/// 本 SAFETY；自行裸调本函数者自担保活责任）。
pub fn render_targets_arc(targets: &[std::sync::Arc<Surface>]) -> RenderPass<'static> {
    let refs: Vec<&Surface> = targets.iter().map(|s| s.as_ref()).collect();
    // SAFETY: 生命周期收口依赖上述保活契约（桥在 finish 时同步清空
    // keepalive），Pass 存续期内目标必然存活
    unsafe { std::mem::transmute::<RenderPass<'_>, RenderPass<'static>>(render_targets(&refs)) }
}

/// Python 绑定形态（兼容层模块架构 §3.1 预言）：`with pygame.targets([a, b]):`
pub fn render_targets<'a>(targets: &[&'a Surface]) -> RenderPass<'a> {
    assert!(targets.len() >= 2, "render_targets: 至少 2 个颜色目标");
    let size = targets[0].size();
    assert!(
        targets.iter().all(|t| t.size() == size),
        "render_targets: 全部目标尺寸须一致"
    );
    let gpu = gpu().clone();
    RenderPass {
        depth: false,
        kind: PassKind::Mrt {
            gpu,
            targets: targets.to_vec(),
            batch: RefCell::new(Batch::new()),
        },
    }
}
