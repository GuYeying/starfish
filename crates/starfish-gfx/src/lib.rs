//! 几何绘制模块：形状生成器（纯数据）+ 标准渲染对象工厂
//!
//! 与 font 模块同构的范式：**引擎只产出标准渲染对象（Mesh / BindGroup /
//! RenderPipeline），数据归开发者，绘制走 02-04 范式**——
//! 没有 ShapeRenderer，也没有 push/flush 渲染器状态。
//!
//! ```ignore
//! use starfish::base::gfx::{self, ShapeVertex};
//! use glam::Vec2;
//!
//! // 相机 uniform（04 案例同款：裸缓冲 + bind group，MVP 由开发者计算写入）
//! let camera_buffer = resouce.create_raw_buffer(Some("camera"), 64, wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST);
//! let camera_bind = resouce.bind_group_builder().uniform_raw(0, camera_buffer.clone(), 64).build(Some("camera_bind"));
//! let geometry = gfx::geometry::shape2d::circle(Vec2::ZERO, 50.0, 64, [1.0; 4]);
//! let mesh = gfx::shape_mesh(&resouce, &geometry);
//! let pipeline = gfx::fill_pipeline_2d(&resouce, &camera_bind, &mesh, 1);
//!
//! // 每帧（02-04 同款调用）：MVP = projection × view × model（glam 合成）
//! resouce.write_buffer(&camera_buffer, 0, bytemuck::bytes_of(&mvp.to_cols_array_2d()));
//! pass.set_pipeline(&pipeline);
//! pass.set_bind_group(0, &camera_bind);
//! pass.set_mesh(&mesh);
//! pass.draw_indexed(0..mesh.index_count(), 0, 0..1);
//!
//! // 实时更新：标准 mesh 写入
//! resouce.write_vertex_buffer(&mesh, geometry.vertices_bytes());
//! resouce.write_index_buffer(&mesh, geometry.index_bytes());
//! ```
//!
//! 相机 uniform 契约：group0 binding0 = 64 字节 MVP（列主序），由开发者计算写入；
//! 2D/3D 的差异只是相机矩阵——顶点统一 pos3（2D 传 z=0）。

pub mod geometry;
pub mod pipeline;

mod vertex;

pub use geometry::Geometry;
pub use pipeline::{
    fill_pipeline_2d, fill_pipeline_3d, line_pipeline_2d, line_pipeline_3d, shape_mesh, shader,
    SHAPE_WGSL,
};
pub use vertex::ShapeVertex;

