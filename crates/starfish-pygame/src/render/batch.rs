//! 批处理执行体（契约 §六 Batch / ADR-5 即时 API × 延迟提交）
//!
//! 即时绘制的 CPU 侧积攒：顶点按**纹理 × 相机（MVP）切换**切段（同纹理
//! 同相机连续绘制合并，任一切换 = flush 边界），`encode` 时一条 render
//! pass 走完（填充流 + 线流各一次管线切换）。quad 与 mesh 同路（统一
//! 顶点 pos3+uv2+color4）。
//!
//! **per-draw 相机/变换（渲染统一 draw 面）**：
//! - `set_mvp(Some(mvp))` = 会话级相机切换（段边界；`None` = Auto 兜底
//!   ——pack 时按目标尺寸现算）；group0 相机 bind 从 pass 级降到**段级**
//!   （先例 = group1 纹理段级切换；material 管线缓存 key 不含相机 bind
//!   ——bind 仅作布局推导，零膨胀）
//! - `set_model(m)` = 会话级变换状态机：**CPU 烘顶点**（`blit_transform`
//!   先例路径——WebGL2 无 push constants，图元/文字顶点量小，零兼容坑）
//!
//! 纯色路径（`texture: None`）解析到 Gpu 的 1×1 白纹理——三路（精灵/
//! 图元/文字图集）同一着色器同一管线族（见 `shader/sprite.wgsl`）。
//!
//! v1 简化（P3 优化位）：顶点缓冲按需增长常驻（[`VertexPool`]），非
//! 环形复用；图元几何来自 `base::gfx::geometry::shape2d` 的 [`Geometry`]。

use std::sync::Arc;

use glam::Mat4;
use starfish_gfx::geometry::Geometry;
use starfish_render::bind_group::bind_group::BindGroup;
use starfish_render::render_pass::attachments::{ColorAttachment, DepthAttachment};
use starfish_render::render_pass::render_pass::RenderPass;
use wgpu::util::DeviceExt;

use super::texture::Texture;
use super::Gpu;

/// 图元几何的拓扑归类（base [`Geometry`] 不携带拓扑，由调用方声明）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryKind {
    /// 三角填充（shape2d 的 rect/circle/ellipse/polygon/capsule 填充版）
    Filled,
    /// 线段（LineList，1px——rect_outline/circle_outline/line/polyline）
    Line,
}

/// 精灵顶点：pos3 + uv2 + color4（"统一顶点布局"；与 base::font::TextVertex
/// 同布局——同一管线语义族）
#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct SpriteVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl SpriteVertex {
    /// 顶点布局（`mesh_builder` / 管线模板用）
    pub fn layout() -> Vec<wgpu::VertexFormat> {
        vec![
            wgpu::VertexFormat::Float32x3,
            wgpu::VertexFormat::Float32x2,
            wgpu::VertexFormat::Float32x4,
        ]
    }

    /// 顶点字节大小（32）
    pub const SIZE: usize = std::mem::size_of::<Self>();
}

/// 一个纹理段：同纹理 + 同相机的连续顶点区间（任一切换 = 段边界）
///
/// `key/view = None` 即纯色段（白纹理，encode 时解析到 [`Gpu::white`]）；
/// `mvp = None` 即 Auto 兜底段（encode 时按目标尺寸现算）。
pub(crate) struct Segment {
    pub key: Option<usize>,
    pub view: Option<Arc<wgpu::TextureView>>,
    pub mvp: Option<Mat4>,
    pub start: u32,
    pub end: u32,
}

/// 单拓扑顶点流（填充流 / 线流）
#[derive(Default)]
pub(crate) struct Stream {
    vertices: Vec<SpriteVertex>,
    segments: Vec<Segment>,
}

impl Stream {
    fn new() -> Self {
        Self {
            vertices: Vec::new(),
            segments: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.vertices.is_empty()
    }

    /// 顶点组入流（段边界 = 纹理切换 ∪ 相机切换；返回本组区间 [start, end)）
    fn push(
        &mut self,
        texture: Option<&Texture>,
        mvp: Option<Mat4>,
        vs: impl IntoIterator<Item = SpriteVertex>,
    ) -> std::ops::Range<u32> {
        let vs: Vec<_> = vs.into_iter().collect();
        if vs.is_empty() {
            let end = self.vertices.len() as u32;
            return end..end;
        }
        // 段边界判定：纹理切换 ∪ 相机切换（含 None ↔ Some）即开新段
        let key = texture.map(Texture::cache_key);
        let need_new = match (self.segments.last(), &key, &mvp) {
            (None, _, _) => true,
            (Some(last), k, m) => last.key != *k || last.mvp != *m,
        };
        let start = self.vertices.len() as u32;
        self.vertices.extend(vs);
        let end = self.vertices.len() as u32;
        if need_new {
            self.segments.push(Segment {
                key,
                view: texture.map(Texture::view),
                mvp,
                start,
                end,
            });
        } else if let Some(last) = self.segments.last_mut() {
            last.end = end;
        }
        start..end
    }

    fn clear(&mut self) {
        self.vertices.clear();
        self.segments.clear();
    }

    fn vertex_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.vertices)
    }
}

/// 顶点组拓扑（基层接口的流选择 + 管线解释约定）
///
/// 流按拓扑分仓：Triangles 入填充流（三角形列表管线）、Lines 入线流
/// （线段列表管线）——同流内段边界只由纹理/相机切换决定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchTopology {
    /// 三角列表：vertices 按 **3 个一组**解释（一顶点组 = N 个三角）
    Triangles,
    /// 线段列表：vertices 按 **2 个一组**解释
    Lines,
}

/// 顶点批处理（pygame 词汇 draw::* 的执行体；base 风格公开面——
/// 契约 §六 标注 pub(crate)，因验收探针在 examples/ 独立 crate 需要
/// 公开面，P3 draw::* 仍在其上包 pygame 词汇，见 changelog 批次六）
///
/// **绘制状态机（渲染统一 draw 面）**：`set_mvp` / `set_model` 为会话级
/// 状态（Batch 挂在目标上，状态跨 push 持续；会话开始由 RenderPass 重置
/// 为 Auto/Identity 兜底）——后续全部 push 按 `cur_mvp`（段边界）分组、
/// 按 `cur_model` CPU 烘顶点。
#[derive(Default)]
pub struct Batch {
    tris: Stream,
    lines: Stream,
    cur_mvp: Option<Mat4>,
    cur_model: Mat4,
}

impl Batch {
    pub fn new() -> Self {
        Self {
            tris: Stream::new(),
            lines: Stream::new(),
            cur_mvp: None,
            cur_model: Mat4::IDENTITY,
        }
    }

    // ── 绘制状态机（per-draw 相机/变换）────────────────────────────

    /// 会话级相机切换（后续 push 进新段；`None` = Auto 兜底——pack 时按
    /// 目标尺寸现算）
    pub fn set_mvp(&mut self, mvp: Option<Mat4>) {
        self.cur_mvp = mvp;
    }

    /// 回 Auto 兜底相机
    pub fn reset_mvp(&mut self) {
        self.cur_mvp = None;
    }

    /// 当前相机状态快照（fill 等"恒默认段"语义的保存/恢复口）
    pub fn mvp_snapshot(&self) -> Option<Mat4> {
        self.cur_mvp
    }

    /// 会话级变换（后续 push 的顶点 CPU 烘此矩阵——`pos → model × pos`）
    pub fn set_model(&mut self, model: Mat4) {

        self.cur_model = model;
    }

    /// 回恒等变换
    pub fn reset_model(&mut self) {
        self.cur_model = Mat4::IDENTITY;
    }

    /// 当前 model 矩阵（调用方组合用：`cur_model × local` 的便捷入口）
    pub fn model(&self) -> Mat4 {
        self.cur_model
    }

    /// 是否无待提交绘制
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty() && self.lines.is_empty()
    }

    /// 清空（present/flush 后由目标调用；**状态不清**——跨 pack 的绘制
    /// 状态机持续，会话开始由 RenderPass 显式重置）
    pub fn clear(&mut self) {
        self.tris.clear();
        self.lines.clear();
    }

    /// draw 消费收口：**one-shot transform**——顶层绘制口执行完自动回
    /// 恒等（物体级变换零泄漏：忘 set 不会复用上一个物体的矩阵；同变换
    /// 多物体就重复 set——64B 矩阵写，廉价）。camera **不在此清**（视角
    /// 级语义：一个会话一个视角，多视角靠 set_camera 分段）。
    fn end_draw(&mut self) {
        self.cur_model = Mat4::IDENTITY;
    }

    /// model 变换一个点（收集层 CPU 烘的单点入口）
    fn bake(&self, x: f32, y: f32, z: f32) -> [f32; 3] {
        let p = self.cur_model.transform_point3(glam::Vec3::new(x, y, z));

        [p.x, p.y, p.z]
    }

    // ── 基层接口：顶点组直入 ────────────────────────────────────────
    // quad/triangle/线段/几何 全部经此入流——上层只负责把图元展开成
    // 顶点组；纹理切段、相机切段、拓扑分仓、打包上屏由流统一处理。

    /// 顶点组直入（基层接口；顶点按 `cur_model` 烘、按 `cur_mvp` 分段）。
    ///
    /// - `topo`：Triangles = 每 **3** 顶点一三角（三角形列表管线）；
    ///   Lines = 每 **2** 顶点一线段。不对齐尾组会被管线丢弃——上层
    ///   展开时保证整组。
    /// - `texture`：None = 1×1 白纹理（纯色路径）。
    /// - 段边界：同流内纹理/相机切换；返回本组流内区间（诊断/高级用法）。
    pub fn push_vertices(
        &mut self,
        topo: BatchTopology,
        texture: Option<&Texture>,
        vertices: &[SpriteVertex],
    ) -> std::ops::Range<u32> {
        let mvp = self.cur_mvp;
        let baked: Vec<SpriteVertex> = vertices
            .iter()
            .map(|v| {
                let p = self.bake(v.pos[0], v.pos[1], v.pos[2]);
                SpriteVertex { pos: p, ..*v }
            })
            .collect();
        match topo {
            BatchTopology::Triangles => self.tris.push(texture, mvp, baked),
            BatchTopology::Lines => self.lines.push(texture, mvp, baked),
        }
    }

    /// 矩形 quad（x/y/w/h，中心原点坐标系；全 uv）——位置由
    /// `cur_model` 承载（`set_transform` 的执行原语）
    ///
    /// `texture = None` → 1×1 白纹理（纯色路径）
    pub fn push_quad(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        color: [f32; 4],
        texture: Option<&Texture>,
    ) {
        self.push_quad_uv(x, y, w, h, [0.0, 0.0, 1.0, 1.0], color, texture);
    }

    /// 矩形 quad + 自定 uv 子区域（图集/字体地基）——基层展开糖：
    /// 两三角 (0,1,2)(0,2,3) 六顶点入 Triangles 流（剔除关闭，绕向不敏感）
    pub fn push_quad_uv(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv: [f32; 4],
        color: [f32; 4],
        texture: Option<&Texture>,
    ) {
        let v = |px: f32, py: f32, u: f32, t: f32| SpriteVertex {
            pos: [px, py, 0.0],
            uv: [u, 1.0 - t], // v 翻：y up 世界 BL 角配纹理底（调用方 uv = 旧 TL 序）
            color,
        };
        let (x1, y1) = (x + w, y + h);
        let [u0, v0, u1, v1] = uv;
        self.push_vertices(
            BatchTopology::Triangles,
            texture,
            &[
                v(x, y, u0, v0),
                v(x1, y, u1, v0),
                v(x1, y1, u1, v1),
                v(x, y, u0, v0),
                v(x1, y1, u1, v1),
                v(x, y1, u0, v1),
            ],
        );
        self.end_draw();
    }

    /// 任意四角 quad（2D 形态——仿射变形的执行原语；
    /// 角序 TL/TR/BR/BL，两三角 (0,1,2)(0,2,3)）——基层展开糖
    pub fn push_quad_corners(
        &mut self,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
        color: [f32; 4],
        texture: Option<&Texture>,
    ) {
        let corners3: [[f32; 3]; 4] = corners.map(|p| [p[0], p[1], 0.0]);
        self.push_quad_corners_3d(corners3, uvs, color, texture);
    }

    /// 任意四角 quad（**3D 形态**——角带 z；顶点再按 `cur_model` 烘——
    /// 两级变换复合：调用方局部放置 × 会话 transform。角序 TL/TR/BR/BL，
    /// 两三角 (0,1,2)(0,2,3)）
    pub fn push_quad_corners_3d(
        &mut self,
        corners: [[f32; 3]; 4],
        uvs: [[f32; 2]; 4],
        color: [f32; 4],
        texture: Option<&Texture>,
    ) {
        let v = |p: [f32; 3], t: [f32; 2]| SpriteVertex {
            pos: self.bake(p[0], p[1], p[2]),
            uv: [t[0], 1.0 - t[1]], // v 翻：y up 世界 BL 角配纹理底（调用方 uv = 旧 TL 序）
            color,
        };
        let [c0, c1, c2, c3] = corners;
        let [u0, u1, u2, u3] = uvs;
        self.push_vertices(
            BatchTopology::Triangles,
            texture,
            &[
                v(c0, u0),
                v(c1, u1),
                v(c2, u2),
                v(c0, u0),
                v(c2, u2),
                v(c3, u3),
            ],
        );
        self.end_draw();
    }

    /// base 图元几何入批（`shape2d` 家族产出；顶点先按 `cur_model` 烘
    /// （[`Geometry::transformed`] 复用）、z 透传、颜色烘焙为调用色）
    ///
    /// **GLES 兼容（批次二十四）**：三角形以“退化四边形”入队——每三角形
    /// (a,b,c) 展开为 quad_corners [a,b,c,c]（第二三角 acc 退化零面积），
    /// 与 quad 同一条被实证的顶点路径；直接三角段在部分 GLES 驱动上
    /// 静默不显示（Android 真机实测，quad 路径正常）。
    pub fn push_geometry(
        &mut self,
        kind: GeometryKind,
        geometry: &Geometry,
        offset: [f32; 2],
        color: [f32; 4],
    ) {
        // 会话 transform 烘进几何（恒等时零拷贝快路径——transformed 返回新
        // Vec，恒等判断跳过分配）
        let owned;
        let geometry = if self.cur_model == Mat4::IDENTITY {
            geometry
        } else {
            owned = geometry.transformed(&self.cur_model);
            &owned
        };
        let v = |sv: &starfish_gfx::ShapeVertex| SpriteVertex {
            pos: [sv.pos[0] + offset[0], sv.pos[1] + offset[1], sv.pos[2]],
            // 白纹理全域白，uv 取值不影响纯色输出
            uv: [0.5, 0.5],
            color,
        };
        match kind {
            GeometryKind::Filled => {
                // 三角形 → 退化四边形序列（GLES 兼容——直接三角段曾在
                // 部分驱动上静默不显示，Android 真机实测；直三角直入待
                // 真机复验后可切 push_vertices(Triangles) 免退化顶点）
                let mut tris: Vec<[SpriteVertex; 3]> = Vec::new();
                if geometry.indices.is_empty() {
                    for t in geometry.vertices.chunks(3) {
                        if t.len() == 3 {
                            tris.push([v(&t[0]), v(&t[1]), v(&t[2])]);
                        }
                    }
                } else {
                    for idx in geometry.indices.chunks(3) {
                        if idx.len() == 3 {
                            tris.push([
                                v(&geometry.vertices[idx[0] as usize]),
                                v(&geometry.vertices[idx[1] as usize]),
                                v(&geometry.vertices[idx[2] as usize]),
                            ]);
                        }
                    }
                }
                for tri in &tris {
                    self.push_vertices(
                        BatchTopology::Triangles,
                        None,
                        &[
                            tri[0], tri[1], tri[2], // (a,b,c)
                            tri[0], tri[2], tri[2], // (a,c,c)=退化
                        ],
                    );
                }
            }
            GeometryKind::Line => {
                let verts: Vec<SpriteVertex> = if geometry.indices.is_empty() {
                    geometry.vertices.iter().map(|sv| v(sv)).collect()
                } else {
                    geometry
                        .indices
                        .iter()
                        .map(|&i| v(&geometry.vertices[i as usize]))
                        .collect()
                };
                self.push_vertices(BatchTopology::Lines, None, &verts);
            }
        }
        self.end_draw();
    }

    /// 待提交 quad 数（诊断用；图元按 3 顶点折算）
    pub fn len(&self) -> usize {
        (self.tris.vertices.len() + self.lines.vertices.len()) / 6
    }

    /// 首个顶点位置（测试观察绘制顺序用；空批 = None）
    #[cfg(test)]
    pub fn first_vertex_pos(&self) -> Option<[f32; 3]> {
        self.tris.vertices.first().map(|v| v.pos)
    }
}

// ── 提交（目标层调用；pub(crate) 面）─────────────────────────────

/// 编码上下文（目标层拼装；附件直用 starfish-render 类型——与
/// OverlayCtx 同轨，无中间转换层。`formats` 平行携带颜色格式供管线
/// 变体选取：附件本体无格式字段）
pub(crate) struct EncodeCtx<'a> {
    pub gpu: &'a Gpu,
    pub camera: &'a super::target::CameraRig,
    /// 颜色附件（1 个 = 普通 pass；N 个 = MRT）
    pub colors: Vec<ColorAttachment>,
    /// 颜色附件格式（平行数组——管线选取用）
    pub formats: Vec<wgpu::TextureFormat>,
    pub depth: Option<DepthAttachment>,
    pub samples: u32,
    /// 裁剪矩形（scissor；`set_clip` 的执行形态）
    pub scissor: Option<[u32; 4]>,
    /// 白纹理 bind（纯色段直引 Gpu 本体字段；不经缓存表）
    pub white_bind: &'a BindGroup,
}

/// clip Rect → 合法 scissor（钳入目标界内；零面积 → None）
pub(crate) fn scissor_from(clip: crate::IRect, size: (u32, u32)) -> Option<[u32; 4]> {
    let x0 = clip[0].max(0) as u32;
    let y0 = clip[1].max(0) as u32;
    if x0 >= size.0 || y0 >= size.1 || clip[2] <= 0 || clip[3] <= 0 {
        return None;
    }
    let w = (clip[2] as u32).min(size.0 - x0);
    let h = (clip[3] as u32).min(size.1 - y0);
    Some([x0, y0, w, h])
}

/// 段相机 bind（一次性 uniform + bind——段数 = 纹理 × 相机组合，量小；
/// 与 "每 pack 一次性 buffer" 同款防覆盖定约，按段复制使各段命令引用
/// 各自独立数据）
fn seg_cam_bind(gpu: &Gpu, mvp: &Mat4) -> BindGroup {
    let cam_buf = Arc::new(gpu.device.create_buffer_init(
        &wgpu::util::BufferInitDescriptor {
            label: Some("pygame_camera_mvp_seg"),
            contents: bytemuck::bytes_of(&mvp.to_cols_array_2d()),
            usage: wgpu::BufferUsages::UNIFORM,
        },
    ));
    gpu.access
        .bind_group_builder()
        .uniform_raw(0, cam_buf, 64)
        .build(Some("pygame_camera_bind_seg"))
}

/// 批 → 命令缓冲（一条 render pass：填充流 + 线流）。空批返回 None。
///
/// 深度格式恒 Depth24Plus（base SurfaceSettings 默认同源）；
/// 颜色目标 >1 时切 MRT 管线（双输出着色器）。
pub(crate) fn encode(batch: &Batch, ctx: &mut EncodeCtx) -> Option<wgpu::CommandBuffer> {
    if batch.is_empty() {
        return None;
    }

    // 纹理 bind group 预建（段级，按纹理指针缓存；纯色段 = 白纹理本体直引）
    for seg in batch.tris.segments.iter().chain(&batch.lines.segments) {
        if let (Some(key), Some(view)) = (&seg.key, &seg.view) {
            ctx.gpu.ensure_bind_view(*key, view.clone());
        }
    }

    let mrt = ctx.colors.len() > 1;
    let color_attachments: Vec<Option<ColorAttachment>> =
        ctx.colors.iter().map(|c| Some(c.clone())).collect();
    let depth_attachment = ctx.depth.clone();
    let color_refs: Vec<&Option<ColorAttachment>> = color_attachments.iter().collect();

    let mut encoder = ctx.gpu.access.create_command_encoder();
    let mut pass = encoder.begin_render_pass(
        "pygame_batch",
        &color_refs,
        depth_attachment,
        None,
        None,
        None,
    );

    // set_clip：裁剪矩形作用于整个 pass（面级状态；须在 draw 之前设置）
    if let Some([x, y, w, h]) = ctx.scissor {
        pass.set_scissor_rect(x, y, w, h);
    }
    if !batch.tris.is_empty() {
        draw_stream(&mut pass, &batch.tris, false, ctx);
    }
    if !batch.lines.is_empty() {
        draw_stream(&mut pass, &batch.lines, true, ctx);
    }
    pass.end();
    Some(encoder.finish())
}

fn draw_stream(pass: &mut RenderPass, stream: &Stream, lines: bool, ctx: &EncodeCtx) {
    let depth = ctx.depth.is_some();
    let pipeline = if ctx.colors.len() > 1 {
        ctx.gpu
            .material
            .pipeline_mrt(ctx.gpu, &ctx.camera.bind, &ctx.formats, ctx.samples, lines)
    } else {
        ctx.gpu.material.pipeline(
            ctx.gpu,
            &ctx.camera.bind,
            ctx.formats[0],
            ctx.samples,
            lines,
            depth,
        )
    };
    pass.set_pipeline(&pipeline);
    // 顶点数据内联一次性 buffer：命令缓冲引用各自独立的数据，天然免疫
    // "同帧多次 pack 的 write_buffer 互相覆盖"（批次九实测坑）。
    // P3 优化位：append-only arena + slice 偏移（需 base RenderPass 支持）。
    let buf = Arc::new(
        ctx.gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pygame_batch_vbo"),
                contents: stream.vertex_bytes(),
                usage: wgpu::BufferUsages::VERTEX,
            }),
    );
    pass.set_vertex_buffer(0, &buf);
    let binds = ctx.gpu.binds.borrow();
    for seg in &stream.segments {
        // 相机 bind 段级（渲染统一 draw 面）：None = Auto 兜底（pack 时
        // 按目标尺寸现算的 ctx.camera.mvp）；Some = 会话 set_camera
        let seg_bind = seg_cam_bind(ctx.gpu, seg.mvp.as_ref().unwrap_or(&ctx.camera.mvp));
        pass.set_bind_group(0, &seg_bind);
        match (&seg.key, &seg.view) {
            (Some(key), _) => {
                if let Some(b) = binds.get(key) {
                    pass.set_bind_group(1, b);
                }
            }
            (None, _) => pass.set_bind_group(1, ctx.white_bind),
        }
        pass.draw(seg.start..seg.end, 0..1);
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;

    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    /// 手写 quad 顶点组（与 push_quad 同坐标同 UV，基层等价性对照组）
    fn quad_verts(x: f32, y: f32) -> [SpriteVertex; 6] {
        let vx = |px: f32, py: f32, u: f32, t: f32| SpriteVertex {
            pos: [px, py, 0.0],
            uv: [u, 1.0 - t], // v 翻后语义（与 push_quad 内翻一致）
            color: WHITE,
        };
        [
            vx(x, y, 0.0, 0.0),
            vx(x + 30.0, y, 1.0, 0.0),
            vx(x + 30.0, y + 40.0, 1.0, 1.0),
            vx(x, y, 0.0, 0.0),
            vx(x + 30.0, y + 40.0, 1.0, 1.0),
            vx(x, y + 40.0, 0.0, 1.0),
        ]
    }

    #[test]
    fn quad_family_equivalent_through_base() {
        // push_quad 与手写顶点组（同坐标）逐顶点一致——基层等价性
        let mut a = Batch::new();
        a.push_quad(10.0, 20.0, 30.0, 40.0, WHITE, None);
        let mut b = Batch::new();
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(10.0, 20.0));
        assert_eq!(a.tris.vertices.len(), b.tris.vertices.len());
        for (x, y) in a.tris.vertices.iter().zip(b.tris.vertices.iter()) {
            assert_eq!(x.pos, y.pos);
            assert_eq!(x.uv, y.uv);
            assert_eq!(x.color, y.color);
        }
        assert_eq!(a.tris.segments.len(), b.tris.segments.len());
    }

    #[test]
    fn topology_splits_streams() {
        let mut b = Batch::new();
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(0.0, 0.0));
        b.push_vertices(
            BatchTopology::Lines,
            None,
            &[
                SpriteVertex { pos: [0.0, 0.0, 0.0], uv: [0.0; 2], color: WHITE },
                SpriteVertex { pos: [1.0, 1.0, 0.0], uv: [0.0; 2], color: WHITE },
            ],
        );
        assert_eq!(b.tris.vertices.len(), 6);
        assert_eq!(b.lines.vertices.len(), 2);
    }

    #[test]
    fn same_texture_continues_segment() {
        let mut b = Batch::new();
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(0.0, 0.0));
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(50.0, 0.0));
        assert_eq!(b.tris.vertices.len(), 12);
        assert_eq!(b.tris.segments.len(), 1, "同纹理（None）续段不开新段");
    }

    #[test]
    fn empty_group_is_noop() {
        let mut b = Batch::new();
        let r = b.push_vertices(BatchTopology::Triangles, None, &[]);
        assert!(r.is_empty());
        assert!(b.is_empty());
        assert!(b.tris.segments.is_empty(), "空组不得产生空段");
    }

    #[test]
    fn push_geometry_lines_route_through_base() {
        // 线段几何经基层入线流（2 顶点一组；填充流保持空）
        let geo = starfish_gfx::geometry::shape2d::line(
            [0.0, 0.0].into(),
            [10.0, 10.0].into(),
            WHITE,
        );
        let expect = geo.vertices.len();
        let mut b = Batch::new();
        b.push_geometry(GeometryKind::Line, &geo, [0.0, 0.0], WHITE);
        assert_eq!(b.lines.vertices.len(), expect);
        assert!(b.tris.is_empty());
    }

    #[test]
    fn push_geometry_filled_degenerate_path_unchanged() {
        // 填充几何保持退化四边形实证路径（GLES 兼容）：每三角 6 顶点
        let tri = starfish_gfx::geometry::shape2d::circle(
            [100.0, 100.0].into(),
            30.0,
            3,
            WHITE,
        );
        let expect_tris = if tri.indices.is_empty() {
            tri.vertices.len() / 3
        } else {
            tri.indices.len() / 3
        };
        let mut b = Batch::new();
        b.push_geometry(GeometryKind::Filled, &tri, [0.0, 0.0], WHITE);
        assert_eq!(
            b.tris.vertices.len(),
            expect_tris * 6,
            "每三角退化展开为 6 顶点（零面积第二三角）"
        );
    }

    #[test]
    fn mvp_switch_splits_segment() {
        // 相机切换 = 段边界（渲染统一 draw 面）——同纹理不同 MVP 分两段
        let mut b = Batch::new();
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(0.0, 0.0));
        b.set_mvp(Some(Mat4::from_translation(glam::Vec3::ONE)));
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(50.0, 0.0));
        assert_eq!(b.tris.segments.len(), 2, "MVP 切换开新段");
        b.reset_mvp();
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(100.0, 0.0));
        assert_eq!(b.tris.segments.len(), 3, "回 Auto 又一段");
        // 同 MVP 续段
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(150.0, 0.0));
        assert_eq!(b.tris.segments.len(), 3, "同 MVP 同纹理续段");
    }

    #[test]
    fn model_bakes_into_vertices() {
        // 会话 transform = CPU 烘顶点（渲染统一 draw 面）——平移 +100 落进
        // 顶点 pos
        let mut b = Batch::new();
        b.set_model(Mat4::from_translation(glam::Vec3::new(100.0, 0.0, 0.0)));
        b.push_vertices(BatchTopology::Triangles, None, &quad_verts(0.0, 0.0));
        let first = b.tris.vertices.first().expect("顶点已入流");
        assert_eq!(first.pos, [100.0, 0.0, 0.0], "model 烘进顶点");
    }

    #[test]
    fn model_is_one_shot_per_draw() {
        // one-shot transform：draw 消费后自动回恒等（零泄漏——
        // 第二个 draw 忘 set 不会复用上一个物体的矩阵）
        let mut b = Batch::new();
        b.set_model(Mat4::from_translation(glam::Vec3::new(100.0, 0.0, 0.0)));
        b.push_quad(0.0, 0.0, 10.0, 10.0, WHITE, None);
        let first = b.tris.vertices.first().expect("顶点已入流");
        assert_eq!(first.pos, [100.0, 0.0, 0.0], "第一次 draw 吃到 model");
        b.push_quad(0.0, 0.0, 10.0, 10.0, WHITE, None);
        let second_v = &b.tris.vertices[6];
        assert_eq!(second_v.pos, [0.0, 0.0, 0.0], "第二次 draw 已回恒等（one-shot 消费）");
    }

    #[test]
    fn model_identity_is_zero_cost() {
        // 恒等 model 下 push_quad 与旧路径逐位一致（零拷贝快路径）
        let mut b = Batch::new();
        b.push_quad(10.0, 20.0, 30.0, 40.0, WHITE, None);
        let first = b.tris.vertices.first().expect("顶点已入流");
        assert_eq!(first.pos, [10.0, 20.0, 0.0]);
    }
}
