//! 几何数据容器与形状生成器

pub mod shape2d;
pub mod shape3d;
pub mod tri;

use glam::{Mat4, Vec4};

use crate::base::gfx::ShapeVertex;

/// 生成的几何数据（顶点 + u16 索引；**索引恒存在**，统一索引绘制路径）
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub vertices: Vec<ShapeVertex>,
    pub indices: Vec<u16>,
}

impl Geometry {
    /// 从顶点与索引构建
    pub fn indexed(vertices: Vec<ShapeVertex>, indices: Vec<u16>) -> Self {
        Self { vertices, indices }
    }

    /// 矩阵变换：对全部顶点 `pos` 施加 4×4（uv 无此概念；索引不变）
    ///
    /// 低级原语——平移/旋转/缩放的组合由开发者用 glam 完成
    /// （`Mat4::from_translation` / `from_rotation_z` / `from_scale` 相乘）。
    pub fn transformed(&self, m: &Mat4) -> Geometry {
        let vertices = self
            .vertices
            .iter()
            .map(|v| {
                let p = m * Vec4::new(v.pos[0], v.pos[1], v.pos[2], 1.0);
                ShapeVertex {
                    pos: [p.x, p.y, p.z],
                    color: v.color,
                }
            })
            .collect();
        Geometry {
            vertices,
            indices: self.indices.clone(),
        }
    }

    /// 追加一个顶点，返回其在顶点数组中的下标
    pub fn push(&mut self, pos: [f32; 3], color: [f32; 4]) -> u16 {
        self.vertices.push(ShapeVertex::new(pos, color));
        (self.vertices.len() - 1) as u16
    }

    /// 追加一批索引
    pub fn extend_indices(&mut self, indices: &[u16]) {
        self.indices.extend_from_slice(indices);
    }

    /// 当前顶点数
    pub fn vertex_len(&self) -> usize {
        self.vertices.len()
    }

    /// 当前索引数
    pub fn index_len(&self) -> usize {
        self.indices.len()
    }

    /// 顶点字节流（`mesh_builder` 用）
    pub fn vertices_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.vertices)
    }

    /// 索引字节流（`with_short_indices` 用）
    pub fn index_bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.indices)
    }
}
