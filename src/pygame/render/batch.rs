//! 批处理执行体（契约 §六 Batch / ADR-5 即时 API × 延迟提交）
//!
//! 即时绘制的 CPU 侧积攒：顶点按**纹理切换**切段（同纹理连续绘制合并，
//! 切换纹理 = flush 边界），`encode` 时一条 render pass 走完（填充流 +
//! 线流各一次管线切换）。quad 与 mesh 同路（统一顶点 pos3+uv2+color4）。
//!
//! 纯色路径（`texture: None`）解析到 Gpu 的 1×1 白纹理——三路（精灵/
//! 图元/文字图集）同一着色器同一管线族（见 `shader/sprite.wgsl`）。
//!
//! v1 简化（P3 优化位）：顶点缓冲按需增长常驻（[`VertexPool`]），非
//! 环形复用；图元几何来自 `base::gfx::geometry::shape2d` 的 [`Geometry`]。

use std::sync::Arc;

use crate::base::gfx::geometry::Geometry;
use crate::base::render::bind_group::bind_group::BindGroup;
use crate::base::render::render_pass::attachments::{ColorAttachment, LoadOp, StoreOp};
use crate::base::render::render_pass::render_pass::RenderPass;
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

/// 一个纹理段：同纹理的连续顶点区间（纹理切换 = 段边界）
///
/// `key/view = None` 即纯色段（白纹理，encode 时解析到 [`Gpu::white`]）。
pub(crate) struct Segment {
    pub key: Option<usize>,
    pub view: Option<Arc<wgpu::TextureView>>,
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

    /// 顶点组入流（段边界 = 纹理切换；返回本组区间 [start, end)）
    fn push(
        &mut self,
        texture: Option<&Texture>,
        vs: impl IntoIterator<Item = SpriteVertex>,
    ) -> std::ops::Range<u32> {
        let vs: Vec<_> = vs.into_iter().collect();
        if vs.is_empty() {
            let end = self.vertices.len() as u32;
            return end..end;
        }
        // 段边界判定：纹理切换（含 None ↔ Some）即开新段
        let key = texture.map(Texture::cache_key);
        let need_new = match (self.segments.last(), &key) {
            (None, _) => true,
            (Some(last), k) => last.key != *k,
        };
        let start = self.vertices.len() as u32;
        self.vertices.extend(vs);
        let end = self.vertices.len() as u32;
        if need_new {
            self.segments.push(Segment {
                key,
                view: texture.map(Texture::view),
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
/// （线段列表管线）——同流内段边界只由纹理切换决定。
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
#[derive(Default)]
pub struct Batch {
    tris: Stream,
    lines: Stream,
}

impl Batch {
    pub fn new() -> Self {
        Self {
            tris: Stream::new(),
            lines: Stream::new(),
        }
    }

    /// 是否无待提交绘制
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty() && self.lines.is_empty()
    }

    /// 清空（present/flush 后由目标调用）
    pub fn clear(&mut self) {
        self.tris.clear();
        self.lines.clear();
    }

    // ── 基层接口：顶点组直入 ────────────────────────────────────────
    // quad/triangle/线段/几何 全部经此入流——上层只负责把图元展开成
    // 顶点组；纹理切段、拓扑分仓、打包上屏由流统一处理。

    /// 顶点组直入（基层接口）。
    ///
    /// - `topo`：Triangles = 每 **3** 顶点一三角（三角形列表管线）；
    ///   Lines = 每 **2** 顶点一线段。不对齐尾组会被管线丢弃——上层
    ///   展开时保证整组。
    /// - `texture`：None = 1×1 白纹理（纯色路径）。
    /// - 段边界：同流内纹理切换；返回本组流内区间（诊断/高级用法）。
    ///
    /// 便利层（`push_quad*` / `push_geometry`）全部是本接口的展开糖。
    pub fn push_vertices(
        &mut self,
        topo: BatchTopology,
        texture: Option<&Texture>,
        vertices: &[SpriteVertex],
    ) -> std::ops::Range<u32> {
        match topo {
            BatchTopology::Triangles => self.tris.push(texture, vertices.iter().copied()),
            BatchTopology::Lines => self.lines.push(texture, vertices.iter().copied()),
        }
    }

    /// 矩形 quad（x/y/w/h 像素，y 向下；全 uv）——blit 的执行原语
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
            uv: [u, t],
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
    }

    /// 任意四角 quad（transform 旋转/镜像等仿射变形的执行原语；
    /// 角序 TL/TR/BR/BL，两三角 (0,1,2)(0,2,3)）——基层展开糖
    pub fn push_quad_corners(
        &mut self,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
        color: [f32; 4],
        texture: Option<&Texture>,
    ) {
        let v = |p: [f32; 2], t: [f32; 2]| SpriteVertex {
            pos: [p[0], p[1], 0.0],
            uv: t,
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
    }

    /// base 图元几何入批（`shape2d` 家族产出；z 透传、颜色烘焙为调用色）
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
        let v = |sv: &crate::base::gfx::ShapeVertex| SpriteVertex {
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

/// 颜色附件规格（1 个 = 普通 pass；N 个 = MRT）
pub(crate) struct PassColor {
    pub view: Arc<wgpu::TextureView>,
    pub format: wgpu::TextureFormat,
}

/// 深度附件规格
pub(crate) struct PassDepth {
    pub view: Arc<wgpu::TextureView>,
    /// true = 本 pass 清深度（LoadOp::Clear(1.0)），false = 承接已有深度
    pub clear: bool,
}

/// 编码上下文（目标层拼装）
pub(crate) struct EncodeCtx<'a> {
    pub gpu: &'a Gpu,
    pub camera: &'a super::target::CameraRig,
    pub colors: Vec<PassColor>,
    pub depth: Option<PassDepth>,
    pub samples: u32,
    /// 裁剪矩形（scissor；`set_clip` 的执行形态）
    pub scissor: Option<[u32; 4]>,
    /// 白纹理 bind（纯色段直引 Gpu 本体字段；不经缓存表）
    pub white_bind: &'a BindGroup,
}

/// clip Rect → 合法 scissor（钳入目标界内；零面积 → None）
pub(crate) fn scissor_from(clip: crate::pygame::Rect, size: (u32, u32)) -> Option<[u32; 4]> {
    let x0 = clip.left.max(0) as u32;
    let y0 = clip.top.max(0) as u32;
    if x0 >= size.0 || y0 >= size.1 || clip.width <= 0 || clip.height <= 0 {
        return None;
    }
    let w = (clip.width as u32).min(size.0 - x0);
    let h = (clip.height as u32).min(size.1 - y0);
    Some([x0, y0, w, h])
}

/// 批 → 命令缓冲（一条 render pass：填充流 + 线流）。空批返回 None。
///
/// 深度格式恒 Depth24Plus（base SurfaceSettings 默认同源）；
/// 颜色目标 >1 时切 MRT 管线（双输出着色器）。
pub(crate) fn encode(batch: &Batch, ctx: &mut EncodeCtx) -> Option<wgpu::CommandBuffer> {
    if batch.is_empty() {
        return None;
    }
    // 相机 MVP 写入（queue.write_buffer：本命令提交前生效）
    ctx.gpu.queue.write_buffer(
        &ctx.camera.buf,
        0,
        bytemuck::bytes_of(&ctx.camera.mvp.to_cols_array_2d()),
    );

    // 纹理 bind group 预建（段级，按纹理指针缓存；纯色段 = 白纹理本体直引）
    for seg in batch.tris.segments.iter().chain(&batch.lines.segments) {
        if let (Some(key), Some(view)) = (&seg.key, &seg.view) {
            ctx.gpu.ensure_bind_view(*key, view.clone());
        }
    }

    let mrt = ctx.colors.len() > 1;
    let color_attachments: Vec<Option<ColorAttachment>> = ctx
        .colors
        .iter()
        .map(|c| {
            Some(ColorAttachment {
                view: c.view.clone(),
                load: LoadOp::Load, // 清屏由目标层负责（begin_frame / 透明底纹理）
                store: StoreOp::Store,
                resolve_target: None,
                depth_slice: None,
            })
        })
        .collect();
    let depth_attachment = ctx.depth.as_ref().map(|d| {
        crate::base::render::render_pass::attachments::DepthAttachment {
            view: d.view.clone(),
            load: if d.clear {
                LoadOp::Clear(1.0)
            } else {
                LoadOp::Load
            },
            store: StoreOp::Store,
            stencil_ops: None,
            depth_slice: None,
        }
    });
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
        let formats: Vec<wgpu::TextureFormat> =
            ctx.colors.iter().map(|c| c.format).collect();
        ctx.gpu
            .material
            .pipeline_mrt(ctx.gpu, &ctx.camera.bind, &formats, ctx.samples, lines)
    } else {
        ctx.gpu.material.pipeline(
            ctx.gpu,
            &ctx.camera.bind,
            ctx.colors[0].format,
            ctx.samples,
            lines,
            depth,
        )
    };
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &ctx.camera.bind);
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
            uv: [u, t],
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
        let geo = crate::base::gfx::geometry::shape2d::line(
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
        let tri = crate::base::gfx::geometry::shape2d::circle(
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
}
