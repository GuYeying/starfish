//! pygame::render::Camera —— MVP 组装（契约 ADR-4：唯一抽象，camera 即 MVP）
//!
//! 单一通用管线的变换源：2D = 正交（像素坐标 y 向下），3D = 透视（y 向上、
//! 右手系、wgpu 零一深度）。`mvp()` 是唯一上传的变换量——用户不手写矩阵。

use glam::Mat4;
use glam::camera::rh::proj::directx as proj;

/// 相机（视图 + 投影；`mvp()` 唯一上传量）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    view: Mat4,
    projection: Mat4,
}

impl Camera {
    /// 2D 正交相机：像素坐标直入——`(0,0)` 左上、`(w,h)` 右下（y 向下，
    /// ADR-8 对齐 pygame）；深度关闭（画家算法按提交序）
    pub fn ortho(width: f32, height: f32) -> Self {
        Self {
            view: Mat4::IDENTITY,
            // y 轴翻转：bottom=height / top=0（directx = wgpu 0..1 深度约定；
            // 本管线深度关闭，z 映射不参与）
            projection: proj::orthographic(0.0, width, height, 0.0, 0.0, 1.0),
        }
    }

    /// 3D 透视相机（v2 内容先行铺路）：fov 度数制（y 向视野）、右手系、
    /// 相机看向 -Z；位姿用 [`Self::with_view`] 施加
    pub fn perspective(fov_y_degrees: f32, aspect: f32, near: f32, far: f32) -> Self {
        Self {
            view: Mat4::IDENTITY,
            projection: proj::perspective(fov_y_degrees.to_radians(), aspect, near, far),
        }
    }

    /// 替换视图矩阵（3D 位姿：`Mat4::look_at_rh` 等；2D 恒 Identity）
    pub fn with_view(mut self, view: Mat4) -> Self {
        self.view = view;
        self
    }

    /// 唯一上传的变换量：`projection × view`
    pub fn mvp(&self) -> Mat4 {
        self.projection * self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec4;

    fn ndc(m: Mat4, x: f32, y: f32) -> (f32, f32) {
        let p = m * Vec4::new(x, y, 0.0, 1.0);
        (p.x / p.w, p.y / p.w)
    }

    #[test]
    fn ortho_maps_pixels_to_ndc_y_down() {
        let m = Camera::ortho(800.0, 600.0).mvp();
        // 左上 (0,0) → NDC (-1, +1)；右下 (w,h) → (+1, -1)——y 向下
        let (x, y) = ndc(m, 0.0, 0.0);
        assert!((x + 1.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
        let (x, y) = ndc(m, 800.0, 600.0);
        assert!((x - 1.0).abs() < 1e-5 && (y + 1.0).abs() < 1e-5);
        // 中心 → 原点
        let (x, y) = ndc(m, 400.0, 300.0);
        assert!(x.abs() < 1e-5 && y.abs() < 1e-5);
    }

    #[test]
    fn perspective_is_finite_and_view_composes() {
        let c = Camera::perspective(60.0, 4.0 / 3.0, 0.1, 100.0).with_view(Mat4::look_at_rh(
            glam::Vec3::new(0.0, 0.0, 3.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        ));
        let m = c.mvp();
        assert!(m.to_cols_array().iter().all(|v| v.is_finite()));
        // with_view 后 view 生效：mvp ≠ 纯 projection
        assert_ne!(m, Camera::perspective(60.0, 4.0 / 3.0, 0.1, 100.0).mvp());
    }
}
