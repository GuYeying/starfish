//! 文本渲染标准对象工厂
//!
//! 产出**标准渲染对象**（Mesh / BindGroup / RenderPipeline），
//! 绘制与 examples/02-04 完全同构（`pass.set_pipeline` / `set_bind_group` /
//! `set_mesh` / `draw`）——没有专属 Renderer，也没有隐藏状态。
//!
//! 每个工厂函数都可被绕过：着色器可用自定义 `Shader` 替换、
//! 网格可用 `resouce.mesh_builder(TextVertex::layout(), bytes)` 自建、
//! 管线可用 `render_pipeline_builder_2d/3d` 自装配。

use std::sync::Arc;

use glam::Mat4;
use wgpu::BufferUsages;

use crate::base::render::bind_group::bind_group::BindGroup;
use crate::base::render::mesh::mesh::Mesh;
use crate::base::render::pipeline::{BlendMode, CullMode, RenderPipeline};
use crate::base::render::render_resource_access::RenderResourceAccess;
use crate::base::render::sampler_desc::SamplerDescriptor;
use crate::base::render::shader_module::shader_module::ShaderModule;
use crate::base::resources::shader::Shader;

use super::{GlyphAtlas, TextVertex};

/// 文本着色器源码（内嵌，不依赖运行时资源路径；
/// 需要 `pos3` 顶点布局，见 [`TextVertex`]）
pub const TEXT_WGSL: &str = include_str!("text.wgsl");

/// 文本着色器（vs_main / fs_main）
pub fn shader(access: &RenderResourceAccess) -> Arc<ShaderModule> {
    access
        .shader_module_builder(Shader::new(TEXT_WGSL.to_string()))
        .build(Some("text_shader"))
}

/// 图集绑定（texture 0 + filtering sampler 1）
///
/// 所有图集的 bind group layout 一致——管线可复用于任意图集。
pub fn atlas_bind_group(access: &RenderResourceAccess, atlas: &GlyphAtlas) -> BindGroup {
    let sampler = Arc::new(
        access.create_sampler("font_sampler", &SamplerDescriptor::linear_clamp()),
    );
    access
        .bind_group_builder()
        .texture(0, atlas.texture.clone())
        .sampler(1, sampler)
        .build(Some("font_atlas_bind"))
}

/// 文本网格（本地空间：基线左端锚定在**原点**）
///
/// billboard / 自由变换的基座——放置矩阵由开发者计算并写入相机 uniform，
/// 或用 [`text_mesh_tf`] 把矩阵烘焙进顶点。
pub fn text_mesh_local(
    access: &RenderResourceAccess,
    atlas: &GlyphAtlas,
    text: &str,
    scale: f32,
    color: [f32; 4],
) -> Mesh {
    let vertices = super::layout_text_local(atlas, text, scale, color);
    mesh_from_vertices(access, &vertices)
}

/// 文本网格（矩阵版）：本地布局 + 任意变换（平移/旋转/缩放自由组合，glam 合成）
pub fn text_mesh_tf(
    access: &RenderResourceAccess,
    atlas: &GlyphAtlas,
    text: &str,
    m: &Mat4,
    scale: f32,
    color: [f32; 4],
) -> Mesh {
    let vertices = super::transform_vertices(
        &super::layout_text_local(atlas, text, scale, color),
        m,
    );
    mesh_from_vertices(access, &vertices)
}

fn mesh_from_vertices(access: &RenderResourceAccess, vertices: &[TextVertex]) -> Mesh {
    access
        .mesh_builder(TextVertex::layout(), bytemuck::cast_slice(vertices).to_vec())
        .with_vertex_usages(BufferUsages::VERTEX | BufferUsages::COPY_DST)
        .build(None, None)
}

/// 文本渲染管线（2D 预设：Alpha 混合、无剔除、深度关闭）
///
/// * `mesh` 同时充当**顶点布局模板**（管线布局从示例对象推导——引擎既有设计）
/// * `sample_count` 与渲染目标 MSAA 采样数一致（`SurfaceSettings::with_msaa` 开启时传同值）
/// * 世界空间文本需要深度遮挡时，用 `render_pipeline_builder_3d` 自装配
///   （pass 须提供深度附件）
pub fn text_pipeline(
    access: &RenderResourceAccess,
    camera_bind: &BindGroup,
    atlas_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
) -> Arc<RenderPipeline> {
    let shader = shader(access);
    access
        .render_pipeline_builder_2d(&shader)
        .blend(BlendMode::Alpha)
        .cull(CullMode::None)
        .sample_count(sample_count.max(1))
        .build(&[camera_bind, atlas_bind], mesh, Some("text_pipeline"))
}

/// 文本渲染管线（3D 世界空间变体：深度测试开启，文字可被场景物体遮挡）
///
/// 与 [`text_pipeline`]（HUD/2D）的差异：深度 Standard + 写入、
/// 不剔除背面（从文字背面看为镜像——需要单面时用
/// `render_pipeline_builder_3d` 自装配改 cull）、Alpha 混合保留。
/// **pass 必须提供深度附件**（深度格式 = 设备默认深度格式）。
pub fn text_pipeline_3d(
    access: &RenderResourceAccess,
    camera_bind: &BindGroup,
    atlas_bind: &BindGroup,
    mesh: &Mesh,
    sample_count: u32,
) -> Arc<RenderPipeline> {
    let shader = shader(access);
    access
        .render_pipeline_builder_3d(&shader)
        .blend(BlendMode::Alpha)
        .cull(CullMode::None)
        .sample_count(sample_count.max(1))
        .build(&[camera_bind, atlas_bind], mesh, Some("text_pipeline_3d"))
}
