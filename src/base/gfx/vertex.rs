//! 几何顶点：pos3 + color4（2D 传 z=0，3D 传世界坐标）

use wgpu::VertexFormat;

/// 几何顶点（与 `shape.wgsl` 顶点布局对应）
///
/// 公开类型：开发者可绕过形状生成器直接构造自定义几何。
#[derive(Clone, Copy, Debug, bytemuck::Zeroable, bytemuck::Pod)]
#[repr(C)]
pub struct ShapeVertex {
    pub pos: [f32; 3],
    pub color: [f32; 4],
}

impl ShapeVertex {
    pub const fn new(pos: [f32; 3], color: [f32; 4]) -> Self {
        Self { pos, color }
    }

    /// 顶点布局（`resouce.mesh_builder` 用）
    pub fn layout() -> Vec<VertexFormat> {
        vec![VertexFormat::Float32x3, VertexFormat::Float32x4]
    }

    /// 顶点字节大小（28）
    pub const fn size() -> usize {
        std::mem::size_of::<Self>()
    }
}
