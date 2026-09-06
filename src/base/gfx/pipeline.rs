//! 形状渲染标准对象工厂
//!
//! 产出**标准渲染对象**（Mesh / BindGroup / RenderPipeline），
//! 绘制与 examples/02-04 完全同构。每个工厂都可绕过：
//! 顶点可自建（[`ShapeVertex`] + `mesh_builder`）、着色器可替换（[`SHAPE_WGSL`]）、
//! 管线可用 `render_pipeline_builder_2d/3d` 自装配。

use std::sync::Arc;

use wgpu::{BufferUsages, PrimitiveTopology};

use crate::base::render::bind_group::bind_group::BindGroup;
use crate::base::render::mesh::mesh::Mesh;
use crate::base::render::pipeline::{BlendMode, CullMode, RenderPipeline};
use crate::base::render::render_resource_access::RenderResourceAccess;
use crate::base::render::shader_module::shader_module::ShaderModule;
use crate::base::resources::shader::Shader;

use super::ShapeVertex;
use super::geometry::Geometry;

/// 形状着色器源码（内嵌；填充与线段两管线共用）
pub const SHAPE_WGSL: &str = include_str!("shape.wgsl");

/// 形状着色器（vs_main / fs_main）
pub fn shader(access: &RenderResourceAccess) -> Arc<ShaderModule> {
    access
        .shader_module_builder(Shader::new(SHAPE_WGSL.to_string()))
        .build(Some("shape_shader"))
}

/// 几何数据 → 标准 [`Mesh`]（带索引缓冲；顶点缓冲带 COPY_DST 支持实时更新）
///
/// 实时更新走标准写入：
/// `access.write_vertex_buffer(&mesh, geometry.vertices_bytes())` +
/// `access.write_index_buffer(&mesh, geometry.index_bytes())`。
pub fn shape_mesh(access: &RenderResourceAccess, geometry: &Geometry) -> Mesh {
    let builder = access
        .mesh_builder(ShapeVertex::layout(), geometry.vertices_bytes().to_vec())
        .with_vertex_usages(BufferUsages::VERTEX | BufferUsages::COPY_DST);
    if geometry.indices.is_empty() {
        builder.build(None, None)
    } else {
        builder
            .with_short_indices(geometry.indices.clone())
            .build(None, None)
    }
}

/// 形状填充管线（2D：像素正交；Alpha 混合；深度关闭）
pub fn fill_pipeline_2d(
    access: &RenderResourceAccess,
    camera_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
) -> Arc<RenderPipeline> {
    build(access, PrimitiveTopology::TriangleList, false, camera_bind, mesh, sample_count, "gfx_fill_2d")
}

/// 形状填充管线（3D：世界坐标；深度测试开启，可被场景遮挡）
pub fn fill_pipeline_3d(
    access: &RenderResourceAccess,
    camera_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
) -> Arc<RenderPipeline> {
    build(access, PrimitiveTopology::TriangleList, true, camera_bind, mesh, sample_count, "gfx_fill_3d")
}

/// 线段管线（2D：LineList，1px；用于描边/调试线）
pub fn line_pipeline_2d(
    access: &RenderResourceAccess,
    camera_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
) -> Arc<RenderPipeline> {
    build(access, PrimitiveTopology::LineList, false, camera_bind, mesh, sample_count, "gfx_line_2d")
}

/// 线段管线（3D：LineList，1px；深度测试开启）
pub fn line_pipeline_3d(
    access: &RenderResourceAccess,
    camera_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
) -> Arc<RenderPipeline> {
    build(access, PrimitiveTopology::LineList, true, camera_bind, mesh, sample_count, "gfx_line_3d")
}

pub(crate) fn build(
    access: &RenderResourceAccess,
    topology: PrimitiveTopology,
    depth: bool,
    camera_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
    label: &str,
) -> Arc<RenderPipeline> {
    let shader = shader(access);
    let builder = if depth {
        access.render_pipeline_builder_3d(&shader)
    } else {
        access.render_pipeline_builder_2d(&shader)
    };
    builder
        .topology(topology)
        .blend(BlendMode::Alpha)
        .cull(CullMode::None)
        .sample_count(sample_count.max(1))
        .build(&[camera_bind], mesh, Some(label))
}
