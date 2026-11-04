//! pygame::render::Camera —— MVP 组装（契约 ADR-4：唯一抽象，camera 即 MVP）
//!
//! 单一通用管线的变换源：2D = 正交（**中心原点、y 向上**——wgpu NDC
//! 同向，与 3D 透视/物理米制系三方同构），3D = 透视（y 向上、右手系、
//! wgpu 零一深度）。`mvp()` 是唯一上传的变换量——用户不手写矩阵。

use glam::Mat4;
use glam::camera::rh::proj::directx as proj;

/// 相机（视图 + 投影；`mvp()` 唯一上传量）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    view: Mat4,
    projection: Mat4,
}

impl Camera {
    /// 2D 正交相机：**中心原点**——`(0,0)` 屏幕中心、x 右 y 上（wgpu NDC
    /// 同向；与 3D 透视/物理米制系三方同构）。视野 = 整个目标
    /// （`(-w/2, -h/2)` 左下 .. `(w/2, h/2)` 右上）；深度关闭（画家算法
    /// 按提交序）
    pub fn ortho(width: f32, height: f32) -> Self {
        let (w, h) = (width.max(1.0) * 0.5, height.max(1.0) * 0.5);
        Self {
            view: Mat4::IDENTITY,
            projection: proj::orthographic(-w, w, -h, h, 0.0, 1.0),
        }
    }

    /// 3D 透视相机：fov 度数制（y 向视野）、右手系、相机看向 -Z；
    /// 位姿用 [`Self::with_view`] 施加
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

/// 默认相机种类（Auto 兜底形态——`init(dim=3)` 定型；**会话级
/// `set_camera` 的语义取代旧全局 Manual override**——每会话自持，
/// 互不串线）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraKind {
    /// 自动 2D（默认）：按各目标自身尺寸正交——中心原点像素坐标直入
    Auto2d,
    /// 自动 3D（`init(dim=3)` 定型）：按目标宽高比透视 × identity 视图
    ///（fov 度数制）
    Auto3d {
        fov_deg: f32,
        near: f32,
        far: f32,
    },
}

impl CameraKind {
    /// 按目标尺寸解出 MVP（Auto 隐式跟随目标）
    pub fn resolve_mvp(&self, size: (f32, f32)) -> Mat4 {
        match self {
            CameraKind::Auto2d => Camera::ortho(size.0.max(1.0), size.1.max(1.0)).mvp(),
            CameraKind::Auto3d { fov_deg, near, far } => {
                let aspect = (size.0 / size.1.max(1.0)).max(1e-4);
                Camera::perspective(*fov_deg, aspect, *near, *far).mvp()
            }
        }
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
    fn ortho_maps_center_origin_y_up() {
        let m = Camera::ortho(800.0, 600.0).mvp();
        // 中心 (0,0) → NDC (0,0)；右上角 (+w/2, +h/2) → (+1, +1)——y 向上
        let (x, y) = ndc(m, 0.0, 0.0);
        assert!(x.abs() < 1e-5 && y.abs() < 1e-5);
        let (x, y) = ndc(m, 400.0, 300.0);
        assert!((x - 1.0).abs() < 1e-5 && (y - 1.0).abs() < 1e-5);
        // 左下 (-w/2, -h/2) → (-1, -1)
        let (x, y) = ndc(m, -400.0, -300.0);
        assert!((x + 1.0).abs() < 1e-5 && (y + 1.0).abs() < 1e-5);
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
